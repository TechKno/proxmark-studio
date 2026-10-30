use crate::model::{Algorithm, Channel, Communication};
use zeroize::Zeroizing;

pub struct CommandSpec {
    pub text: Zeroizing<String>,
    pub label: String,
    pub sensitive: bool,
    pub kind: Kind,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Recovery,
    Console,
    Detect,
    Device,
    Applications,
    Files,
    Settings,
    Authenticate,
    Read,
    CredentialRead,
    Write,
    Help,
}
impl CommandSpec {
    pub fn new(
        text: String,
        label: impl Into<String>,
        sensitive: bool,
        kind: Kind,
    ) -> Result<Self, String> {
        validate_line(&text)?;
        Ok(Self {
            text: Zeroizing::new(text),
            label: label.into(),
            sensitive,
            kind,
        })
    }
    pub fn simple(text: &str, kind: Kind) -> Self {
        Self::new(text.into(), text, false, kind).expect("static command")
    }
    pub fn console(text: String) -> Result<Self, String> {
        let lower = text.to_lowercase();
        let sensitive = lower.contains("key")
            || lower.contains("-k")
            || lower.contains("pwd")
            || lower.contains("pass")
            || lower.contains("auth")
            || lower.contains("default")
            || lower.contains("script")
            || lower.contains("trace")
            || lower.contains("apdu");
        // No command text in status labels. No console history is persisted.
        Self::new(text, "Console command", sensitive, Kind::Console)
    }
}

// Pipe commands use at most 254 bytes; larger commands use a real terminal.
pub fn validate_line(text: &str) -> Result<(), String> {
    let lower = text.to_lowercase();
    if regex::Regex::new(r"--schann(?:\s+|=)(?:ev2|lrp)(?:\s|$)")
        .unwrap()
        .is_match(&lower)
        || [
            "hf mfdes createmfcmapping",
            "hf mfdes createdelegateapp",
            "hf mfdes createmacfile",
            "hf mfdes intauth",
            "hf mfdes vdesign",
            "hf mfdes pc",
        ]
        .iter()
        .any(|c| lower.contains(c))
    {
        return Err("This EV2/LRP/EV3 operation is unsupported and disabled in Studio".into());
    }
    if text.trim().is_empty() {
        return Err("Enter a command".into());
    }
    if text.len() > 4096 {
        return Err(
            "Maximum 4096 UTF-8 bytes per one-shot command; larger input requires an external client."
                .into(),
        );
    }
    if text.chars().any(|c| c.is_control()) {
        return Err("Commands must be a single line without control characters".into());
    }
    Ok(())
}
pub fn hex(text: &str, bytes: usize, label: &str) -> Result<String, String> {
    let value: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if value.len() != bytes * 2 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "{label} must contain exactly {bytes} hexadecimal bytes"
        ));
    }
    Ok(value.to_uppercase())
}
pub fn payload(text: &str) -> Result<Vec<u8>, String> {
    let v: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if v.is_empty() || !v.len().is_multiple_of(2) || !v.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Enter complete hexadecimal data bytes".into());
    }
    if v.len() > 2048 {
        return Err("Guided writes accept at most 1024 bytes per operation".into());
    }
    Ok((0..v.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&v[i..i + 2], 16).unwrap())
        .collect())
}
pub fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

pub struct DesfireParams<'a> {
    pub aid: &'a str,
    pub fid: &'a str,
    pub key: &'a str,
    pub key_number: u8,
    pub algorithm: Algorithm,
    pub channel: Channel,
    pub communication: Communication,
    pub no_auth: bool,
    pub backup_file: bool,
}
impl DesfireParams<'_> {
    pub fn build(
        &self,
        kind: Kind,
        offset: u32,
        length: u32,
        data: Option<&[u8]>,
    ) -> Result<CommandSpec, String> {
        let aid = hex(self.aid, 3, "Application ID")?;
        let fid = hex(self.fid, 1, "File ID")?;
        if u8::from_str_radix(&fid, 16).unwrap() > 31 {
            return Err("DESFire file ID must be 00–1F".into());
        }
        if self.key_number > 13 {
            return Err("Key number must be 0–13".into());
        }
        if offset > 0xffffff || length > 0xffffff {
            return Err("Offset and length must fit in 24 bits".into());
        }
        let verb = match kind {
            Kind::Applications => "getaids",
            Kind::Files => "getfileids",
            Kind::Settings => "getfilesettings",
            Kind::Authenticate => "auth",
            Kind::Read => "read",
            Kind::Write => "write",
            _ => return Err("Unsupported DESFire action".into()),
        };
        if kind == Kind::Authenticate && self.no_auth {
            return Err("Enable authentication and enter a key first".into());
        }
        let mut cmd = Zeroizing::new(format!("hf mfdes {verb}"));
        if kind != Kind::Applications {
            cmd.push_str(&format!(" --aid {aid}"));
        }
        if matches!(kind, Kind::Read | Kind::Write | Kind::Settings) {
            cmd.push_str(&format!(" --fid {fid}"));
        }
        if kind == Kind::Settings {
            cmd.push_str(" -v");
        }
        if self.no_auth {
            cmd.push_str(" --no-auth");
        } else {
            if self.channel == Channel::D40
                && matches!(self.algorithm, Algorithm::Aes | Algorithm::ThreeTdea)
            {
                return Err("D40 supports DES and 2TDEA only".into());
            }
            let key = Zeroizing::new(hex(self.key, self.algorithm.key_bytes(), "Key")?);
            cmd.push_str(&format!(
                " -n {} -t {} -k {} --schann {} -m {}",
                self.key_number,
                self.algorithm.cli(),
                key.as_str(),
                self.channel.cli(),
                self.communication.cli()
            ));
        }
        if matches!(kind, Kind::Read | Kind::Write) {
            cmd.push_str(&format!(" --type data -o {offset:06X}"));
        }
        if kind == Kind::Read {
            cmd.push_str(&format!(" -l {length:06X}"));
        }
        if kind == Kind::Write {
            let data = data.ok_or("Enter data to programme")?;
            if data.is_empty() {
                return Err("Write data cannot be empty".into());
            }
            // Only standard/backup data files; --commit supports backup transaction completion.
            cmd.push_str(&format!(" -d {}", encode(data)));
            if self.backup_file {
                cmd.push_str(" --commit");
            }
        }
        CommandSpec::new(
            cmd.to_string(),
            format!("DESFire {verb}"),
            !self.no_auth,
            kind,
        )
    }
}

pub fn mfu_read(page: u32) -> Result<CommandSpec, String> {
    if page > 255 {
        return Err("Page must be 0–255".into());
    }
    CommandSpec::new(
        format!("hf mfu rdbl -b {page}"),
        format!("Read page {page}"),
        false,
        Kind::Read,
    )
}
pub fn mfu_write(page: u32, data: &[u8]) -> Result<CommandSpec, String> {
    if !(4..=129).contains(&page) {
        return Err("Guided NTAG writes are restricted to pages 4–129. Check the actual tag's data/lock boundaries first.".into());
    }
    if data.len() != 4 {
        return Err("An Ultralight / NTAG page contains exactly 4 bytes".into());
    }
    CommandSpec::new(
        format!("hf mfu wrbl -b {page} -d {}", encode(data)),
        format!("Programme page {page}"),
        false,
        Kind::Write,
    )
}
pub fn em_write(id: &str) -> Result<CommandSpec, String> {
    CommandSpec::new(
        format!("lf em 410x clone --id {}", hex(id, 5, "EM410x ID")?),
        "Programme EM410x on T5577",
        false,
        Kind::Write,
    )
}
