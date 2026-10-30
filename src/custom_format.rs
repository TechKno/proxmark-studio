//! User-defined Wiegand fields and parity. MSB positions are zero based.
use crate::{
    command::{CommandSpec, Kind},
    credential::{hid_header, Encoded, Target},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Layout {
    pub name: String,
    pub bits: usize,
    pub facility_start: usize,
    pub facility_bits: usize,
    pub card_start: usize,
    pub card_bits: usize,
    pub even_bit: usize,
    pub even_start: usize,
    pub even_end: usize,
    pub odd_bit: usize,
    pub odd_start: usize,
    pub odd_end: usize,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            name: "Custom 26".into(),
            bits: 26,
            facility_start: 1,
            facility_bits: 8,
            card_start: 9,
            card_bits: 16,
            even_bit: 0,
            even_start: 1,
            even_end: 13,
            odd_bit: 25,
            odd_start: 13,
            odd_end: 25,
        }
    }
}
impl Layout {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.len() > 80
            || self.name.chars().any(char::is_control)
            || !(4..=84).contains(&self.bits)
        {
            return Err("Custom layout needs a name and 4–84 bits".into());
        }
        if self.facility_bits > 32
            || self.card_bits == 0
            || self.card_bits > 64
            || self
                .facility_start
                .checked_add(self.facility_bits)
                .is_none_or(|n| n > self.bits)
            || self
                .card_start
                .checked_add(self.card_bits)
                .is_none_or(|n| n > self.bits)
        {
            return Err("Field width or position exceeds the layout".into());
        }
        let mut used = vec![false; self.bits];
        for i in (self.facility_start..self.facility_start + self.facility_bits)
            .chain(self.card_start..self.card_start + self.card_bits)
            .chain([self.even_bit, self.odd_bit])
        {
            if i >= self.bits || used[i] {
                return Err("Fields and parity bits must not overlap".into());
            }
            used[i] = true;
        }
        for (bit, start, end) in [
            (self.even_bit, self.even_start, self.even_end),
            (self.odd_bit, self.odd_start, self.odd_end),
        ] {
            if start >= end
                || end > self.bits
                || (start..end).contains(&bit)
                || (start..end).contains(&self.even_bit)
                || (start..end).contains(&self.odd_bit)
            {
                return Err(
                    "Parity ranges must contain data only, with exclusive end positions".into(),
                );
            }
        }
        Ok(())
    }
    pub fn encode(&self, fc: u32, cn: u64) -> Result<Encoded, String> {
        self.validate()?;
        if (self.facility_bits < 32 && u64::from(fc) >= (1_u64 << self.facility_bits))
            || (self.card_bits < 64 && cn >= (1_u64 << self.card_bits))
        {
            return Err("Credential exceeds custom field widths".into());
        }
        let mut bits = vec![0_u8; self.bits];
        for (start, len, value) in [
            (self.facility_start, self.facility_bits, u64::from(fc)),
            (self.card_start, self.card_bits, cn),
        ] {
            for i in 0..len {
                bits[start + i] = ((value >> (len - i - 1)) & 1) as u8;
            }
        }
        bits[self.even_bit] = bits[self.even_start..self.even_end].iter().sum::<u8>() % 2;
        bits[self.odd_bit] = 1 - bits[self.odd_start..self.odd_end].iter().sum::<u8>() % 2;
        let value = bits.iter().fold(0_u128, |a, b| (a << 1) | u128::from(*b));
        Ok(Encoded {
            binary: bits.iter().map(|b| char::from(b'0' + *b)).collect(),
            hid_raw: format!("{:024X}", hid_header(self.bits) | value),
        })
    }
    pub fn decode(&self, binary: &str) -> Result<(u32, u64, bool), String> {
        self.validate()?;
        let bits = binary.bytes().collect::<Vec<_>>();
        if bits.len() != self.bits || !bits.iter().all(|b| matches!(b, b'0' | b'1')) {
            return Err("Binary payload does not match custom length".into());
        }
        let field = |start, len| {
            bits[start..start + len]
                .iter()
                .fold(0_u64, |v, b| (v << 1) | u64::from(*b - b'0'))
        };
        let fc = field(self.facility_start, self.facility_bits) as u32;
        let cn = field(self.card_start, self.card_bits);
        Ok((fc, cn, self.encode(fc, cn)?.binary == binary))
    }
    pub fn write(&self, fc: u32, cn: u64, target: Target) -> Result<(CommandSpec, String), String> {
        let e = self.encode(fc, cn)?;
        let flag = match target {
            Target::T5577 => "",
            Target::Q5 => " --q5",
            Target::Em4305 => " --em",
        };
        Ok((
            CommandSpec::new(
                format!("lf hid clone --bin {}{flag}", e.binary),
                format!("Programme {}", self.name),
                false,
                Kind::Write,
            )?,
            e.hid_raw,
        ))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub layout: Layout,
    pub facility_code: u32,
    pub card_number: u64,
    pub target: Target,
}
