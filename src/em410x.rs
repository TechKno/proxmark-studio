//! Reader-side interpretations of an EM410x 40-bit identifier.
//! An RF identifier does not encode its reader's output mode. These are explicit
//! interpretations, not detected HID formats or proof of controller acceptance.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReaderFormat {
    #[default]
    Wiegand26,
    Wiegand34,
    Customer8Id32,
}
impl ReaderFormat {
    pub const ALL: [Self; 3] = [Self::Wiegand26, Self::Wiegand34, Self::Customer8Id32];
    pub fn name(self) -> &'static str {
        match self {
            Self::Wiegand26 => "Wiegand 26 · low 24 bits · 8 + 16",
            Self::Wiegand34 => "Wiegand 34 · low 32 bits · 16 + 16",
            Self::Customer8Id32 => "Customer/version 8 + card ID 32",
        }
    }
    pub fn explanation(self) -> &'static str {
        match self {
            Self::Wiegand26 => "Facility is bits 16–23; card number is bits 0–15. Equivalent to RRG DEZ 3.5C. The reader generates Wiegand parity; it was not read as a HID credential.",
            Self::Wiegand34 => "Facility is bits 16–31; card number is bits 0–15. Equivalent to RRG DEZ 5.5. The reader output mode must be known separately.",
            Self::Customer8Id32 => "The top byte is the customer/version code; the remaining 32 bits are the card ID. Some systems call the customer byte a facility code.",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub id: String,
    pub format: ReaderFormat,
    pub facility: u32,
    pub number: u64,
    pub decimal40: u64,
    pub decimal32: u32,
    pub decimal24: u32,
}
pub fn decode(id: &str, format: ReaderFormat) -> Result<Decoded, String> {
    let id = crate::command::hex(id, 5, "EM410x ID")?;
    let value = u64::from_str_radix(&id, 16).map_err(|_| "Invalid EM410x identifier")?;
    let (facility, number) = match format {
        ReaderFormat::Wiegand26 => (((value >> 16) & 0xff) as u32, value & 0xffff),
        ReaderFormat::Wiegand34 => (((value >> 16) & 0xffff) as u32, value & 0xffff),
        ReaderFormat::Customer8Id32 => ((value >> 32) as u32, value & 0xffff_ffff),
    };
    Ok(Decoded {
        id,
        format,
        facility,
        number,
        decimal40: value,
        decimal32: value as u32,
        decimal24: (value & 0xff_ffff) as u32,
    })
}
/// The hint filters interpretations; it never changes bytes or selects a format.
pub fn matching_formats(id: &str, known_facility: u32) -> Result<Vec<Decoded>, String> {
    ReaderFormat::ALL
        .into_iter()
        .map(|f| decode(id, f))
        .collect::<Result<Vec<_>, _>>()
        .map(|v| {
            v.into_iter()
                .filter(|d| d.facility == known_facility)
                .collect()
        })
}
pub fn known_facility(value: &str) -> Result<Option<u32>, String> {
    let s = value.trim();
    if s.is_empty() {
        return Ok(None);
    }
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Enter a decimal facility code".into());
    }
    s.parse::<u32>()
        .ok()
        .filter(|v| *v <= 65535)
        .map(Some)
        .ok_or_else(|| "Facility hint must be 0–65535".into())
}
pub fn summary(id: &str, selected: Option<ReaderFormat>) -> String {
    match decode(id, selected.unwrap_or_default()) {
        Ok(d) => format!(
            "Facility {} · card {}\n{} · {}",
            d.facility,
            d.number,
            d.format.name(),
            if selected.is_some() {
                "user-selected interpretation"
            } else {
                "preview; reader format unconfirmed"
            }
        ),
        Err(e) => e,
    }
}
