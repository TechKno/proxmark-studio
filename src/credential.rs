//! Studio's small, explicit Wiegand codec. Bit positions are numbered from the MSB.
//! These layouts are interoperable format facts, checked against upstream RRG.
use crate::command::{CommandSpec, Kind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Format {
    #[default]
    H10301,
    H10306,
    H10304,
    H10302,
    C1k35s,
    C1k48s,
}
impl Format {
    pub const ALL: [Self; 6] = [
        Self::H10301,
        Self::H10306,
        Self::H10304,
        Self::H10302,
        Self::C1k35s,
        Self::C1k48s,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::H10301 => "H10301",
            Self::H10306 => "H10306",
            Self::H10304 => "H10304",
            Self::H10302 => "H10302",
            Self::C1k35s => "C1k35s",
            Self::C1k48s => "C1k48s",
        }
    }
    pub fn bits(self) -> usize {
        match self {
            Self::H10301 => 26,
            Self::H10306 => 34,
            Self::H10304 | Self::H10302 => 37,
            Self::C1k35s => 35,
            Self::C1k48s => 48,
        }
    }
    pub fn facility_max(self) -> u32 {
        match self {
            Self::H10301 => 255,
            Self::H10302 => 0,
            Self::C1k35s => 4095,
            Self::C1k48s => 4194303,
            _ => 65535,
        }
    }
    pub fn card_max(self) -> u64 {
        match self {
            Self::H10304 => 524287,
            Self::H10302 => (1_u64 << 35) - 1,
            Self::C1k35s => 1048575,
            Self::C1k48s => 8388607,
            _ => 65535,
        }
    }
    fn facility_bits(self) -> usize {
        match self {
            Self::H10301 => 8,
            Self::H10302 => 0,
            Self::C1k35s => 12,
            Self::C1k48s => 22,
            _ => 16,
        }
    }
    fn header(self) -> u128 {
        hid_header(self.bits())
    }
    fn parity_ranges(self) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        if self.bits() == 37 {
            (1..19, 18..36)
        } else {
            let mid = self.bits() / 2;
            (1..mid, mid..self.bits() - 1)
        }
    }
    fn corporate(self) -> bool {
        matches!(self, Self::C1k35s | Self::C1k48s)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Credential {
    pub format: Format,
    pub facility_code: u32,
    pub card_number: u64,
}
impl Default for Credential {
    fn default() -> Self {
        Self {
            format: Format::H10301,
            facility_code: 0,
            card_number: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoded {
    pub binary: String,
    pub hid_raw: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub credential: Credential,
    pub parity_valid: bool,
    pub encoded: Encoded,
}
impl Credential {
    pub fn from_decimal(format: Format, fc: &str, cn: &str) -> Result<Self, String> {
        if !fc.trim().bytes().all(|b| b.is_ascii_digit())
            || !cn.trim().bytes().all(|b| b.is_ascii_digit())
        {
            return Err("Facility code and card number must be unsigned decimal integers".into());
        }
        let value = Self {
            format,
            facility_code: fc
                .trim()
                .parse()
                .map_err(|_| "Enter a valid facility code")?,
            card_number: cn.trim().parse().map_err(|_| "Enter a valid card number")?,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.facility_code > self.format.facility_max() {
            return Err(format!(
                "{} facility code range: 0–{}",
                self.format.name(),
                self.format.facility_max()
            ));
        }
        if self.card_number > self.format.card_max() {
            return Err(format!(
                "{} card number range: 0–{}",
                self.format.name(),
                self.format.card_max()
            ));
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Encoded, String> {
        self.validate()?;
        if self.format.corporate() {
            let shift = if self.format == Format::C1k35s {
                21
            } else {
                24
            };
            let value =
                (u128::from(self.facility_code) << shift) | (u128::from(self.card_number) << 1);
            let value = corporate_parity(self.format, value);
            let binary = format!("{value:0width$b}", width = self.format.bits());
            return Ok(render(
                self.format,
                &binary.bytes().map(|b| b - b'0').collect::<Vec<_>>(),
            ));
        }
        let mut bits = vec![0_u8; self.format.bits()];
        let split = 1 + self.format.facility_bits();
        put(&mut bits[1..split], u64::from(self.facility_code));
        let end = bits.len() - 1;
        put(&mut bits[split..end], self.card_number);
        let (even, odd) = self.format.parity_ranges();
        bits[0] = bits[even].iter().sum::<u8>() % 2;
        bits[end] = 1 - (bits[odd].iter().sum::<u8>() % 2);
        Ok(render(self.format, &bits))
    }
}
fn put(bits: &mut [u8], value: u64) {
    let len = bits.len();
    for (i, b) in bits.iter_mut().enumerate() {
        *b = ((value >> (len - i - 1)) & 1) as u8;
    }
}
fn render(format: Format, bits: &[u8]) -> Encoded {
    let binary: String = bits.iter().map(|b| char::from(b'0' + *b)).collect();
    let value = bits
        .iter()
        .fold(0_u128, |value, b| (value << 1) | u128::from(*b));
    Encoded {
        binary,
        hid_raw: format!("{:024X}", format.header() | value),
    }
}
pub fn decode_binary(format: Format, input: &str) -> Result<Decoded, String> {
    let s: String = input.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if s.len() != format.bits() || !s.bytes().all(|b| matches!(b, b'0' | b'1')) {
        return Err(format!(
            "{} requires exactly {} binary digits, including both parity bits",
            format.name(),
            format.bits()
        ));
    }
    let bits: Vec<u8> = s.bytes().map(|b| b - b'0').collect();
    if format.corporate() {
        let value = u128::from_str_radix(&s, 2).unwrap();
        let (shift, parities) = if format == Format::C1k35s {
            (21, (1_u128 << 34) | (1 << 33) | 1)
        } else {
            (24, (1_u128 << 47) | (1 << 46) | 1)
        };
        return Ok(Decoded {
            credential: Credential {
                format,
                facility_code: ((value >> shift) & u128::from(format.facility_max())) as u32,
                card_number: ((value >> 1) & u128::from(format.card_max())) as u64,
            },
            parity_valid: corporate_parity(format, value & !parities) == value,
            encoded: render(format, &bits),
        });
    }
    let split = 1 + format.facility_bits();
    let end = bits.len() - 1;
    let take = |slice: &[u8]| {
        slice
            .iter()
            .fold(0_u64, |value, b| (value << 1) | u64::from(*b))
    };
    let credential = Credential {
        format,
        facility_code: take(&bits[1..split]) as u32,
        card_number: take(&bits[split..end]),
    };
    let (even, odd) = format.parity_ranges();
    let parity_valid = (bits[0] + bits[even].iter().sum::<u8>()).is_multiple_of(2)
        && !(bits[end] + bits[odd].iter().sum::<u8>()).is_multiple_of(2);
    Ok(Decoded {
        credential,
        parity_valid,
        encoded: render(format, &bits),
    })
}
pub fn normalise_raw(input: &str) -> Result<String, String> {
    let s: String = input
        .trim()
        .strip_prefix("0x")
        .or_else(|| input.trim().strip_prefix("0X"))
        .unwrap_or(input.trim())
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    if s.is_empty() || s.len() > 24 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("HID raw data must contain 1–24 hexadecimal digits".into());
    }
    let value = u128::from_str_radix(&s, 16).map_err(|_| "Invalid HID raw data")?;
    Ok(format!("{value:024X}"))
}
pub fn decode_hid(format: Format, input: &str) -> Result<Decoded, String> {
    let raw = normalise_raw(input)?;
    let value = u128::from_str_radix(&raw, 16).unwrap();
    let mask = (1_u128 << format.bits()) - 1;
    if value & !mask != format.header() {
        return Err(format!(
            "HID header/bit length does not match {} ({} bits)",
            format.name(),
            format.bits()
        ));
    }
    decode_binary(
        format,
        &format!("{:0width$b}", value & mask, width = format.bits()),
    )
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Target {
    #[default]
    T5577,
    Q5,
    Em4305,
}
impl Target {
    pub const ALL: [Self; 3] = [Self::T5577, Self::Q5, Self::Em4305];
    pub fn name(self) -> &'static str {
        match self {
            Self::T5577 => "T5577 / T55x7",
            Self::Q5 => "Q5 / T5555",
            Self::Em4305 => "EM4305 / EM4469",
        }
    }
    fn flag(self) -> &'static str {
        match self {
            Self::T5577 => "",
            Self::Q5 => " --q5",
            Self::Em4305 => " --em",
        }
    }
}
pub fn read_command() -> CommandSpec {
    CommandSpec::simple("lf hid reader", Kind::CredentialRead)
}
pub fn write_command(value: &Credential, target: Target) -> Result<CommandSpec, String> {
    value.validate()?;
    CommandSpec::new(
        format!(
            "lf hid clone -w {} --fc {} --cn {}{}",
            value.format.name(),
            value.facility_code,
            value.card_number,
            target.flag()
        ),
        format!("Programme HID {} on {}", value.format.name(), target.name()),
        false,
        Kind::Write,
    )
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialProfile {
    pub name: String,
    pub credential: Credential,
    #[serde(default)]
    pub target: Target,
}
impl CredentialProfile {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.len() > 80
            || self.name.chars().any(char::is_control)
        {
            return Err(
                "Credential profile name must be 1–80 bytes without control characters".into(),
            );
        }
        self.credential.validate()
    }
}

/// HID sentinel encoding, including extended lengths and the special 48-bit header.
pub fn hid_header(bits: usize) -> u128 {
    match bits {
        37 => 0,
        48 => 1_u128 << 48,
        38..=84 => (0x09e00000_u128 << 64) | (1_u128 << bits),
        _ => (1_u128 << 37) | (1_u128 << bits),
    }
}
fn corporate_parity(format: Format, mut value: u128) -> u128 {
    let (mid_mask, second_mask, high_mask, shift) = if format == Format::C1k35s {
        (1_u32, 3_u32, 3_u32, 1)
    } else {
        (0x1b6d, 0x36db, 0x7fff, 14)
    };
    let parity = |v: u32| (v.count_ones() & 1) as u128;
    value |=
        parity((((value >> 32) as u32) & mid_mask) ^ ((value as u32) & 0xb6db6db6)) << (32 + shift);
    value |= 1 - parity((((value >> 32) as u32) & second_mask) ^ ((value as u32) & 0x6db6db6c));
    value |= (1 - parity((((value >> 32) as u32) & high_mask) ^ (value as u32))) << (33 + shift);
    value
}
/// Ambiguities are intentional: a raw 37-bit credential can match several layouts.
pub fn candidates(input: &str, binary: bool) -> Vec<Decoded> {
    Format::ALL
        .into_iter()
        .filter_map(|f| {
            if binary {
                decode_binary(f, input)
            } else {
                decode_hid(f, input)
            }
            .ok()
        })
        .filter(|d| d.parity_valid)
        .collect()
}
