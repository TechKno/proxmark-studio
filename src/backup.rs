//! Studio dump interchange. Complete readable memory, with metadata and integrity.
use crate::command::{encode, hex, CommandSpec, Kind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Geometry {
    Ultralight,
    #[default]
    Ntag213,
    Ntag215,
    Ntag216,
    ClassicMini,
    Classic1K,
    Classic2K,
    Classic4K,
}
impl Geometry {
    pub const ALL: [Self; 8] = [
        Self::Ultralight,
        Self::Ntag213,
        Self::Ntag215,
        Self::Ntag216,
        Self::ClassicMini,
        Self::Classic1K,
        Self::Classic2K,
        Self::Classic4K,
    ];
    pub fn units(self) -> u32 {
        match self {
            Self::Ultralight => 16,
            Self::Ntag213 => 45,
            Self::Ntag215 => 135,
            Self::Ntag216 => 231,
            Self::ClassicMini => 20,
            Self::Classic1K => 64,
            Self::Classic2K => 128,
            Self::Classic4K => 256,
        }
    }
    pub fn width(self) -> usize {
        if self.classic() {
            16
        } else {
            4
        }
    }
    pub fn classic(self) -> bool {
        matches!(
            self,
            Self::ClassicMini | Self::Classic1K | Self::Classic2K | Self::Classic4K
        )
    }
    pub fn sectors(self) -> u8 {
        match self {
            Self::ClassicMini => 5,
            Self::Classic1K => 16,
            Self::Classic2K => 32,
            Self::Classic4K => 40,
            _ => 0,
        }
    }
    pub fn emulator_flag(self) -> Result<&'static str, String> {
        match self {
            Self::ClassicMini => Ok("--mini"),
            Self::Classic1K => Ok("--1k"),
            Self::Classic2K => Ok("--2k"),
            Self::Classic4K => Ok("--4k"),
            _ => Err("Emulator images must use a Classic geometry".into()),
        }
    }
    pub fn matches_model(self, raw: &str) -> bool {
        let model = raw.to_lowercase().replace(['-', '_'], " ");
        model.contains(self.model()) || self == Self::ClassicMini && model.contains("mifare mini")
    }
    pub fn data_end(self) -> u32 {
        match self {
            Self::Ultralight => 15,
            Self::Ntag213 => 39,
            Self::Ntag215 => 129,
            Self::Ntag216 => 225,
            _ => self.units() - 1,
        }
    }
    pub fn writable(self, unit: u32) -> bool {
        if unit >= self.units() {
            return false;
        }
        if self.classic() {
            unit != 0
                && if unit < 128 {
                    unit % 4 != 3
                } else {
                    unit % 16 != 15
                }
        } else {
            (4..=self.data_end()).contains(&unit)
        }
    }
    pub fn model(self) -> &'static str {
        match self {
            Self::Ultralight => "ultralight",
            Self::Ntag213 => "ntag 213",
            Self::Ntag215 => "ntag 215",
            Self::Ntag216 => "ntag 216",
            Self::ClassicMini => "classic mini",
            Self::Classic1K => "classic 1k",
            Self::Classic2K => "classic 2k",
            Self::Classic4K => "classic 4k",
        }
    }
    pub fn read(self, unit: u32, key: &str, b: bool) -> Result<CommandSpec, String> {
        if unit >= self.units() {
            return Err("Block/page outside selected geometry".into());
        }
        if self.classic() {
            CommandSpec::new(
                format!(
                    "hf mf rdbl --blk {unit} -{} -k {}",
                    if b { "b" } else { "a" },
                    hex(key, 6, "Classic key")?
                ),
                format!("Read block {unit}"),
                true,
                Kind::Read,
            )
        } else {
            crate::command::mfu_read(unit)
        }
    }
    pub fn write(self, unit: u32, data: &[u8], key: &str, b: bool) -> Result<CommandSpec, String> {
        if !self.writable(unit) || data.len() != self.width() {
            return Err("Write denied: manufacturer, lock/configuration, sector trailer or invalid data size".into());
        }
        CommandSpec::new(
            if self.classic() {
                format!(
                    "hf mf wrbl --blk {unit} -{} -k {} -d {}",
                    if b { "b" } else { "a" },
                    hex(key, 6, "Classic key")?,
                    encode(data)
                )
            } else {
                format!("hf mfu wrbl -b {unit} -d {}", encode(data))
            },
            format!("Programme data unit {unit}"),
            self.classic(),
            Kind::Write,
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Dump {
    pub schema: u32,
    pub geometry: Geometry,
    pub uid: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
}
impl Dump {
    pub fn new(geometry: Geometry, uid: &str, bytes: Vec<u8>) -> Result<Self, String> {
        let mut d = Self {
            schema: 1,
            geometry,
            uid: uid.to_owned(),
            bytes,
            sha256: String::new(),
        };
        d.sha256 = d.digest();
        d.validate()?;
        Ok(d)
    }
    pub fn digest(&self) -> String {
        let mut h = Sha256::new();
        h.update(format!("{}|{:?}|{}|", self.schema, self.geometry, self.uid));
        h.update(&self.bytes);
        format!("{:x}", h.finalize())
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || self.bytes.len() != self.geometry.units() as usize * self.geometry.width()
        {
            return Err("Dump schema or memory size does not match its geometry".into());
        }
        validate_uid(&self.uid)?;
        if self.digest() != self.sha256 {
            return Err("Dump integrity check failed".into());
        }
        Ok(())
    }
    pub fn differences(&self, other: &Self) -> Result<Vec<usize>, String> {
        self.validate()?;
        other.validate()?;
        if self.geometry != other.geometry {
            return Err("Cannot compare different geometries".into());
        }
        Ok(self
            .bytes
            .iter()
            .zip(&other.bytes)
            .enumerate()
            .filter_map(|(i, (a, b))| (a != b).then_some(i))
            .collect())
    }
}
pub fn validate_uid(s: &str) -> Result<(), String> {
    if ![8, 14, 20].contains(&s.len()) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Enter a 4, 7 or 10 byte tag UID without spaces".into());
    }
    Ok(())
}
pub fn uid(output: &str) -> Option<String> {
    let re =
        regex::Regex::new(r"(?im)\bUID\s*:\s*((?:[0-9a-f]{2}[ :\t]*){4,10})(?:$|[^0-9a-f :\t])")
            .unwrap();
    re.captures(output)
        .map(|c| {
            c[1].chars()
                .filter(char::is_ascii_hexdigit)
                .collect::<String>()
                .to_uppercase()
        })
        .filter(|s| validate_uid(s).is_ok())
}
/// Refuse ambiguous or locked NTAGs rather than guessing lock bit mappings.
pub fn check_locks(g: Geometry, page2: &[u8], dynamic: Option<&[u8]>) -> Result<(), String> {
    if g.classic() {
        return Ok(());
    }
    if page2.len() != 4 || page2[2..4] != [0, 0] {
        return Err("Static lock bytes are set or unreadable; guided write disabled".into());
    }
    if g != Geometry::Ultralight && dynamic.is_none_or(|b| b.len() != 4 || b[..3] != [0, 0, 0]) {
        return Err("Dynamic lock bytes are set or unreadable; guided write disabled".into());
    }
    Ok(())
}
pub struct Step {
    pub read: CommandSpec,
    pub write: Option<CommandSpec>,
    pub expected: Option<Vec<u8>>,
}
pub struct Batch {
    pub geometry: Geometry,
    pub uid: String,
    pub steps: Vec<Step>,
    pub writing: bool,
}
/// Import complete RRG mfu/mfc block JSON without accepting a partial dump as complete.
pub fn import_json(input: &[u8], geometry: Geometry) -> Result<Dump, String> {
    let value: serde_json::Value = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    if value.get("schema").is_some() {
        let d: Dump = serde_json::from_value(value).map_err(|e| e.to_string())?;
        d.validate()?;
        return Ok(d);
    }
    let filetype = value
        .get("FileType")
        .and_then(|v| v.as_str())
        .ok_or("Missing dump format")?;
    let supported = if geometry.classic() {
        ["mfcard", "mfc v2", "mfc v3"].contains(&filetype)
    } else {
        filetype == "mfu"
    };
    if !supported {
        return Err("RRG file type does not match the selected geometry".into());
    }
    let uid = value
        .pointer("/Card/UID")
        .and_then(|v| v.as_str())
        .ok_or("Missing RRG card UID")?;
    let blocks = value
        .get("blocks")
        .and_then(|v| v.as_object())
        .ok_or("Missing RRG memory blocks")?;
    if blocks.len() != geometry.units() as usize {
        return Err("Partial or differently sized RRG dump: select the correct geometry".into());
    }
    let mut bytes = vec![];
    for unit in 0..geometry.units() {
        let raw = blocks
            .get(&unit.to_string())
            .and_then(|v| v.as_str())
            .ok_or("Missing RRG block")?;
        let raw = hex(raw, geometry.width(), "RRG block")?;
        for i in (0..raw.len()).step_by(2) {
            bytes.push(u8::from_str_radix(&raw[i..i + 2], 16).unwrap());
        }
    }
    Dump::new(geometry, &uid.to_uppercase(), bytes)
}

pub fn import_memory(
    bytes: &[u8],
    extension: &str,
    geometry: Geometry,
    uid: &str,
) -> Result<Dump, String> {
    let expected = geometry.units() as usize * geometry.width();
    match extension {
        "bin" | "dump" => {
            let data = if bytes.len() == expected {
                bytes
            } else if !geometry.classic() && bytes.len() == expected + 56 {
                &bytes[56..]
            } else {
                return Err(
                    "Binary size does not match raw memory or an RRG 56-byte MFU header".into(),
                );
            };
            Dump::new(geometry, uid, data.to_vec())
        }
        "eml" => {
            let text =
                std::str::from_utf8(bytes).map_err(|_| "EML must be hexadecimal UTF-8 text")?;
            let mut data = vec![];
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                let raw = hex(line, geometry.width(), "EML row")?;
                for n in (0..raw.len()).step_by(2) {
                    data.push(u8::from_str_radix(&raw[n..n + 2], 16).unwrap());
                }
            }
            Dump::new(geometry, uid, data)
        }
        "json" => import_json(bytes, geometry),
        _ => Err("Use .json, .bin or .eml memory files".into()),
    }
}
impl Dump {
    pub fn eml(&self) -> Result<String, String> {
        self.validate()?;
        Ok(self
            .bytes
            .chunks(self.geometry.width())
            .map(encode)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n")
    }
}
