use crate::{command::Kind, model::Evidence};
use regex::Regex;
use std::sync::LazyLock;

pub fn clean(line: &str) -> String {
    static ANSI: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap());
    ANSI.replace_all(line, "")
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect()
}
fn body(line: &str) -> &str {
    let s = line.trim();
    if s.starts_with('[') {
        s.split_once(']').map(|(_, s)| s.trim()).unwrap_or(s)
    } else {
        s
    }
}
pub fn log_line(line: &str) -> String {
    let l = clean(line);
    let low = l.to_lowercase();
    if [
        "key", "password", "pwd", "session", "rnd", "random", "-k ", "--trkey", "authenc",
        "authmac",
    ]
    .iter()
    .any(|w| low.contains(w))
    {
        "[security] Key or session material withheld".into()
    } else {
        l
    }
}
static NORMAL_DEBUG_LOG_LEVEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^debug log level[. ]+\d+\s+\(\s*(?:none|error|info|debug|extended)\s*\)$").unwrap()
});
pub fn evidence(output: &str, kind: &Kind) -> Evidence {
    let mut e = Evidence {
        uid: crate::backup::uid(output),
        version_mismatch: output.to_lowercase().contains("firmware does not match")
            || output.to_lowercase().contains("device / fw mismatch")
            || matches!(
                crate::maintenance::compatibility(output),
                crate::maintenance::Compatibility::Mismatch { .. }
            ),
        ..Evidence::default()
    };
    let aid_re = Regex::new(r"^([0-9a-fA-F]{6})(?:\s|,|$)").unwrap();
    let file_re = Regex::new(r"(?i)^File ID:\s*([0-9a-f]{2})(?:\s|$)").unwrap();
    let data_re=Regex::new(r"^\s*([0-9a-fA-F]{1,8})(?:/0x[0-9a-fA-F]+)?\s*\|\s*((?:[0-9a-fA-F]{2}\s+)*[0-9a-fA-F]{2})\s*\|").unwrap();
    let em_re = Regex::new(r"(?i)EM 410x ID\s*[:=]?\s*([0-9a-f]{10})(?:\s|$)").unwrap();
    if *kind == Kind::Settings {
        e.desfire_file = crate::desfire_archive::file_settings(output, 0).ok();
    }
    let mut in_aids = false;
    let value = Regex::new(
        r"(?i)^(?:Value:|Read file 0x[0-9a-f]{2} value:)\s*(-?\d+)\s*\(0x([0-9a-f]{8})\)$",
    )
    .unwrap();
    let rights=Regex::new(r"^\[[01.]{4}\] (?:CMK|AMK) (?:Configuration changeable|is changeable|required for create/delete|required for (?:AID|FID) list / GetKeySettings)\s*:\s*(?:YES|NO(?: \(frozen\))?)$").unwrap();
    let count = Regex::new(r"^Key cnt\.\.\.\. (?:[0-9]|1[0-4])$").unwrap();
    for raw in output.lines() {
        let line = clean(raw);
        let b = body(&line);
        let low = b.to_lowercase();
        // RRG also tries alternative lengths/layouts. Their parity diagnostic must not
        // override the selected format's independent local parity check on fresh raw data.
        if *kind == Kind::CredentialRead && low == "parity tests failed" {
            continue;
        }
        // This reports the selected logging verbosity, not an operation error.
        let diagnostic_level = line.trim_start().starts_with("[#]")
            && low.starts_with("debug log level")
            && NORMAL_DEBUG_LOG_LEVEL.is_match(&low);
        // CLI exit status alone is NOT command success. RRG can exit zero after errors.
        if !diagnostic_level
            && ((low.contains("cloning") && low.contains("fail"))
                || line.contains("[!!]")
                || line.contains("[-]")
                || [
                    "error",
                    "failed",
                    "timed out",
                    "timeout",
                    "unknown command",
                    "cannot open",
                    "can't open",
                    "not found",
                    "no tag",
                    "no card",
                    "denied",
                    "invalid",
                    "wrong",
                    "no response",
                    "aborted",
                    "not supported",
                    "offline",
                ]
                .iter()
                .any(|w| low.contains(w)))
        {
            e.reported_error = true;
        }
        if low.contains("selected and authenticated succes") {
            e.authenticated = true;
        }
        if (low.contains("device") && (low.contains("proxmark") || low.contains("pm3")))
            || (*kind == Kind::Device
                && (low.starts_with("communicating with pm3 over ")
                    || low.starts_with("communicating with proxmark3 over ")))
        {
            e.device_seen = true;
        }
        if low.contains("uid:")
            || low.contains("uid :")
            || low.contains("valid") && low.contains("found")
            || low.contains("em 410x id")
        {
            e.tag_seen = true;
        }
        if *kind == Kind::Applications {
            if low.contains("aid list") {
                in_aids = true;
                continue;
            }
            if in_aids {
                if let Some(c) = aid_re.captures(b) {
                    e.aids.push(c[1].to_uppercase());
                }
            }
        }
        if *kind == Kind::Files {
            if let Some(c) = file_re.captures(b) {
                e.files.push(c[1].to_uppercase());
            }
        }
        if *kind == Kind::Settings
            && [
                "file type",
                "file comm",
                "access rights",
                "file size",
                "read.",
                "write.",
                "change.",
                "read/write",
                "file number",
            ]
            .iter()
            .any(|p| low.contains(p))
        {
            e.settings.push(b.to_owned());
        }
        if *kind == Kind::CredentialRead {
            if let Some(raw) = low.strip_prefix("raw:") {
                match crate::credential::normalise_raw(raw.trim()) {
                    Ok(value) => {
                        if !e.hid_raw.contains(&value) {
                            e.hid_raw.push(value);
                        }
                    }
                    Err(_) => e.reported_error = true,
                }
            }
        }
        if matches!(*kind, Kind::Read | Kind::Settings) {
            // Strictly structured, non-secret value/rights observations may leave the private output buffer.

            if let Some(c) = value.captures(b) {
                e.details
                    .push(format!("Value: {} (0x{})", &c[1], c[2].to_uppercase()));
            }

            if rights.is_match(b) || count.is_match(b) {
                e.details.push(b.into());
            }
        }
        if *kind == Kind::Read {
            if let Some(c) = data_re.captures(b) {
                for byte in c[2].split_whitespace() {
                    if let Ok(v) = u8::from_str_radix(byte, 16) {
                        e.bytes.push(v);
                    }
                }
            }
            if let Some(c) = em_re.captures(b) {
                e.bytes = (0..10)
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&c[1][i..i + 2], 16).unwrap())
                    .collect();
            }
        }
    }
    e.aids.sort();
    e.aids.dedup();
    e.files.sort();
    e.files.dedup();
    e
}
