//! Bounded live LF acquisition and offline demodulation in explicit client sessions.
use crate::{
    capabilities::Capabilities,
    command::{CommandSpec, Kind},
    signal::Signal,
};
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Demod {
    #[default]
    None,
    AskManchester,
    AskBiphase,
    Fsk,
    Nrz,
    Psk1,
    Psk2,
}
impl Demod {
    pub const ALL: [Self; 7] = [
        Self::None,
        Self::AskManchester,
        Self::AskBiphase,
        Self::Fsk,
        Self::Nrz,
        Self::Psk1,
        Self::Psk2,
    ];
    pub fn flag(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::AskManchester => Some("--am"),
            Self::AskBiphase => Some("--ab"),
            Self::Fsk => Some("--fs"),
            Self::Nrz => Some("--nr"),
            Self::Psk1 => Some("--p1"),
            Self::Psk2 => Some("--p2"),
        }
    }
}
pub struct Request {
    pub samples: u32,
    pub demod: Demod,
    pub caps: Capabilities,
    pub simulated: bool,
    pub saved: Option<Signal>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    pub signal: Signal,
    pub bits: String,
    pub decoding: Vec<String>,
    pub analysis_error: Option<String>,
    pub simulated: bool,
    pub live: bool,
}
fn filename(path: &std::path::Path) -> Result<String, String> {
    let p = path.to_str().ok_or("Signal path is not UTF-8")?;
    if p.contains(['"', ';', '\n', '\r']) {
        return Err("Unsupported signal path".into());
    }
    Ok(format!("\"{p}\""))
}
pub fn capture_spec(r: &Request, path: &std::path::Path) -> Result<CommandSpec, String> {
    if !(512..=60000).contains(&r.samples) {
        return Err("Capture must contain 512–60000 samples".into());
    }
    r.caps.require("lf read", &["-s"])?;
    r.caps.require("data save", &["-f"])?;
    // Acquisition and export share one process: GraphBuffer is client-local.
    CommandSpec::new(
        format!("lf read -s {}; data save -f {}", r.samples, filename(path)?),
        "Capture LF waveform",
        true,
        Kind::Console,
    )
}
pub fn analysis_spec(r: &Request, path: &std::path::Path) -> Result<Option<CommandSpec>, String> {
    let Some(flag) = r.demod.flag() else {
        return Ok(None);
    };
    r.caps.require("data load", &["-f"])?;
    r.caps.require("data rawdemod", &[flag])?;
    CommandSpec::new(
        format!("data load -f {}; data rawdemod {flag}", filename(path)?),
        "Analyse captured waveform",
        true,
        Kind::Console,
    )
    .map(Some)
}
pub fn bits(raw: &str) -> Result<String, String> {
    let mut bits = String::new();
    for line in raw.lines() {
        let line = crate::parse::clean(line);
        let body = line
            .trim()
            .strip_prefix("[+]")
            .or_else(|| line.trim().strip_prefix("[=]"))
            .unwrap_or(line.trim())
            .trim();
        let compact = body.split_whitespace().collect::<String>();
        if compact.len() >= 8 && compact.bytes().all(|b| b == b'0' || b == b'1') {
            if bits.len() + compact.len() > 32768 {
                return Err("Demodulated bitstream exceeds limit".into());
            }
            bits.push_str(&compact);
        }
    }
    if bits.is_empty() {
        return Err("No structured binary demodulation result".into());
    }
    Ok(bits)
}
/// EM410x frame: nine header bits, ten 4-bit rows with even parity,
/// four even column parity bits and a zero stop bit. Never infer reader format.
pub fn em410x_frames(bits: &str) -> Vec<String> {
    let mut result = Vec::new();
    let b = bits.as_bytes();
    if b.iter().any(|v| !matches!(*v, b'0' | b'1')) {
        return result;
    }
    for f in b.windows(64) {
        if f[..9] != [b'1'; 9] || f[63] != b'0' {
            continue;
        }
        let mut value = 0u64;
        let mut columns = [0; 4];
        let mut valid = true;
        for row in 0..10 {
            let p = 9 + row * 5;
            let mut parity = 0;
            for col in 0..4 {
                let bit = f[p + col] - b'0';
                columns[col] ^= bit;
                parity ^= bit;
                value = (value << 1) | u64::from(bit);
            }
            if parity != (f[p + 4] - b'0') {
                valid = false;
            }
        }
        if (0..4).any(|col| columns[col] != f[59 + col] - b'0') {
            valid = false;
        }
        if valid {
            let id = format!("{value:010X}");
            if !result.contains(&id) {
                result.push(id);
            }
        }
    }
    result
}
pub fn text(signal: &Signal) -> String {
    signal.samples.iter().map(|v| format!("{v}\n")).collect()
}
