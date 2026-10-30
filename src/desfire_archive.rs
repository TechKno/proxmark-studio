//! Readable DESFire content/settings archives. Keys are supplied separately, never archived.
use crate::{
    command::{self, CommandSpec, Kind},
    model::{Algorithm, Channel, Communication},
    runner::Outcome,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;
pub trait Client {
    fn inspect(&self, _command: &str, _flags: &[&str]) -> Result<(), Outcome> {
        Ok(())
    }
    fn run(&mut self, spec: CommandSpec) -> Result<Zeroizing<String>, Outcome>;
}
#[derive(Clone)]
pub struct Auth {
    pub key: Zeroizing<String>,
    pub key_number: u8,
    pub algorithm: Algorithm,
    pub channel: Channel,
    pub kdf: String,
    pub kdf_input: String,
    pub no_auth: bool,
}
impl Auth {
    pub fn spec(
        &self,
        verb: &str,
        aid: &str,
        fid: Option<u8>,
        extra: &str,
        kind: Kind,
        mode: Communication,
    ) -> Result<CommandSpec, String> {
        command::hex(aid, 3, "AID")?;
        if self.key_number > 13 {
            return Err("Key number must be 0–13".into());
        }
        if self.channel == Channel::D40
            && matches!(self.algorithm, Algorithm::Aes | Algorithm::ThreeTdea)
        {
            return Err("D40 supports DES/2TDEA only".into());
        }
        let mut text = Zeroizing::new(format!("hf mfdes {verb}"));
        if !matches!(verb, "getaids" | "getappnames") {
            text.push_str(&format!(" --aid {aid}"));
        }
        if let Some(fid) = fid {
            if fid > 31 {
                return Err("File ID exceeds 1F".into());
            }
            text.push_str(&format!(" --fid {fid:02X}"));
        }
        if self.no_auth {
            text.push_str(" --no-auth");
        } else {
            let key = Zeroizing::new(command::hex(&self.key, self.algorithm.key_bytes(), "Key")?);
            text.push_str(&format!(
                " -n {} -t {} -k {} --schann {} -m {}",
                self.key_number,
                self.algorithm.cli(),
                key.as_str(),
                self.channel.cli(),
                mode.cli()
            ));
        }
        if !["none", "AN10922", "gallagher"].contains(&self.kdf.as_str()) {
            return Err("Unsupported KDF".into());
        }
        text.push_str(&format!(" --kdf {} -c native", self.kdf));
        if self.kdf != "none" {
            let raw = self.kdf_input.split_whitespace().collect::<String>();
            if raw.is_empty()
                || raw.len() > 62
                || !raw.len().is_multiple_of(2)
                || !raw.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("KDF input must be 1–31 bytes".into());
            }
            text.push_str(&format!(" -i {raw}"));
        }
        text.push(' ');
        text.push_str(extra);
        CommandSpec::new(
            text.to_string(),
            format!("DESFire {verb} {aid}"),
            true,
            kind,
        )
    }
    pub fn zero(algorithm: Algorithm, channel: Channel) -> Self {
        Self {
            key: Zeroizing::new("00".repeat(algorithm.key_bytes())),
            key_number: 0,
            algorithm,
            channel,
            kdf: "none".into(),
            kdf_input: String::new(),
            no_auth: false,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyRef {
    pub aid: String,
    pub key_number: u8,
    pub algorithm: Algorithm,
    pub channel: Channel,
    pub alias: String,
}
#[derive(Clone)]
pub struct Credentials {
    pub fallback: Auth,
    pub keys: BTreeMap<(String, u8), Auth>,
}
impl Credentials {
    pub fn resolve(fallback: Auth, refs: &[KeyRef]) -> Result<Self, String> {
        let mut keys = BTreeMap::new();
        for r in refs {
            let aid = command::hex(&r.aid, 3, "Key-reference AID")?;
            if r.key_number > 13 || keys.contains_key(&(aid.clone(), r.key_number)) {
                return Err("Invalid or duplicate DESFire key reference".into());
            }
            keys.insert(
                (aid, r.key_number),
                Auth {
                    key: crate::vault::load(&r.alias, r.algorithm.key_bytes())?,
                    key_number: r.key_number,
                    algorithm: r.algorithm,
                    channel: r.channel,
                    kdf: fallback.kdf.clone(),
                    kdf_input: fallback.kdf_input.clone(),
                    no_auth: false,
                },
            );
        }
        Ok(Self { fallback, keys })
    }
    pub fn get(&self, aid: &str, n: u8) -> Result<Auth, String> {
        self.keys
            .get(&(aid.into(), n))
            .cloned()
            .or_else(|| {
                (self.fallback.key_number == n && !self.fallback.no_auth)
                    .then(|| self.fallback.clone())
            })
            .ok_or_else(|| format!("No supplied key {n} for application {aid}"))
    }
    pub fn directory(&self, aid: &str) -> Auth {
        self.keys
            .get(&(aid.into(), 0))
            .cloned()
            .unwrap_or_else(|| self.fallback.clone())
    }
    pub fn file(&self, aid: &str, f: &File, write: bool) -> Result<Auth, String> {
        let shifts = if write { [8, 4] } else { [12, 4] };
        let rights = shifts.map(|s| ((f.rights >> s) & 15) as u8);
        if rights.contains(&14) {
            let mut a = self.directory(aid);
            a.no_auth = f.mode == Communication::Plain || a.no_auth;
            return Ok(a);
        }
        for n in rights {
            if n < 14 {
                if let Ok(a) = self.get(aid, n) {
                    return Ok(a);
                }
            }
        }
        Err(format!(
            "No supplied key satisfies {} access for file {:02X}",
            if write { "write" } else { "read" },
            f.id
        ))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub id: u8,
    #[serde(default)]
    pub iso_id: Option<u16>,
    pub kind: u8,
    pub mode: Communication,
    pub rights: u16,
    pub size: u32,
    pub lower: i32,
    pub upper: i32,
    pub limited: u8,
    pub record_size: u32,
    pub max_records: u32,
    pub records: Vec<Vec<u8>>,
    pub data: Vec<u8>,
    pub value: Option<i32>,
}
impl File {
    pub fn empty(id: u8) -> Self {
        Self {
            id,
            iso_id: None,
            kind: 0,
            mode: Communication::Plain,
            rights: 0xeeee,
            size: 0,
            lower: 0,
            upper: 0,
            limited: 0,
            record_size: 0,
            max_records: 0,
            records: vec![],
            data: vec![],
            value: None,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.iso_id.is_some_and(|id| id == 0 || id == 0xffff)
            || self.iso_id.is_some() && self.kind == 2
        {
            return Err("Invalid ISO file ID/type".into());
        }
        if self.kind <= 1 && (!self.records.is_empty() || self.value.is_some())
            || self.kind == 2 && (!self.data.is_empty() || !self.records.is_empty())
            || self.kind >= 3 && (!self.data.is_empty() || self.value.is_some())
        {
            return Err("Payload fields disagree with file type".into());
        }
        if self.id > 31 || self.kind > 4 {
            return Err("Unsupported DESFire file type/ID (EV2/EV3/TMAC excluded)".into());
        }
        match self.kind {
            0 | 1 => {
                if self.size > 1024 * 1024 || self.data.len() != self.size as usize {
                    return Err("Incomplete data file".into());
                }
            }
            2 => {
                if self.lower > self.upper
                    || self.limited > 1
                    || self.value.is_none_or(|v| v < self.lower || v > self.upper)
                {
                    return Err("Invalid/incomplete value file".into());
                }
            }
            3 | 4 => {
                if self.record_size == 0
                    || self.record_size > 65536
                    || self.max_records == 0
                    || self.max_records > 0xffffff
                    || self.records.len() > self.max_records as usize
                    || self
                        .records
                        .iter()
                        .any(|r| r.len() != self.record_size as usize)
                {
                    return Err("Invalid/incomplete record file".into());
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    pub fn equivalent_settings(&self, other: &Self) -> bool {
        self.iso_id == other.iso_id
            && self.kind == other.kind
            && self.mode == other.mode
            && self.rights == other.rights
            && match self.kind {
                0 | 1 => self.size == other.size,
                2 => {
                    self.lower == other.lower
                        && self.upper == other.upper
                        && self.limited == other.limited
                }
                3 | 4 => {
                    self.record_size == other.record_size && self.max_records == other.max_records
                }
                _ => false,
            }
    }
    pub fn create_extra(&self, free: bool) -> Result<(String, String), String> {
        self.validate()?;
        let rights = if free { 0xeeee } else { self.rights };
        let mut extra = format!(" --amode {} --rawrights {rights:04X}", self.mode.cli());
        if let Some(id) = self.iso_id {
            if self.kind == 2 || id == 0 || id == 0xffff {
                return Err("ISO ID is not valid for this file type".into());
            }
            extra.push_str(&format!(" --isofid {id:04X}"));
        }
        let verb = match self.kind {
            0 | 1 => {
                extra.push_str(&format!(
                    " --size {:06X}{}",
                    self.size,
                    if self.kind == 1 { " --backup" } else { "" }
                ));
                "createfile"
            }
            2 => {
                extra.push_str(&format!(
                    " --lower {:08X} --upper {:08X} --value {:08X} --lcredit {}",
                    self.lower as u32,
                    self.upper as u32,
                    self.value.unwrap() as u32,
                    self.limited
                ));
                "createvaluefile"
            }
            3 | 4 => {
                extra.push_str(&format!(
                    " --size {:06X} --maxrecord {:06X}{}",
                    self.record_size,
                    self.max_records,
                    if self.kind == 4 { " --cyclic" } else { "" }
                ));
                "createrecordfile"
            }
            _ => return Err("Unsupported file type".into()),
        };
        Ok((verb.into(), extra))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Application {
    pub aid: String,
    pub key_settings: u8,
    pub key_settings2: u8,
    pub key_versions: Vec<u8>,
    pub algorithm: Algorithm,
    pub iso_id: Option<String>,
    pub df_name: Option<String>,
    pub files: Vec<File>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub schema: u32,
    pub uid: String,
    pub applications: Vec<Application>,
    pub sha256: String,
}
impl Archive {
    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(&(self.schema, &self.uid, &self.applications))
            .expect("serialisable archive");
        format!("{:x}", Sha256::digest(bytes))
    }
    pub fn seal(&mut self) {
        self.sha256 = self.digest();
    }
    pub fn validate(&self) -> Result<(), String> {
        crate::backup::validate_uid(&self.uid)?;
        if self.schema != 1 || self.applications.len() > 28 || self.sha256 != self.digest() {
            return Err("DESFire archive integrity/schema failure".into());
        }
        let mut aids = BTreeSet::new();
        for a in &self.applications {
            let id = command::hex(&a.aid, 3, "AID")?;
            if id != a.aid || id == "000000" {
                return Err(
                    "Archive application IDs must be canonical and exclude the PICC root".into(),
                );
            }
            if !aids.insert(a.aid.clone())
                || a.files.len() > 32
                || a.key_settings2 & 0x10 != 0
                || a.key_settings2 & 15 > 14
                || a.key_settings2 & 15 == 0
                || a.key_versions.len() != usize::from(a.key_settings2 & 15)
            {
                return Err("Invalid/unsupported application settings".into());
            }
            let expected_algo = match a.algorithm {
                Algorithm::Des | Algorithm::TwoTdea => 0,
                Algorithm::ThreeTdea => 1,
                Algorithm::Aes => 2,
            };
            if a.key_settings2 >> 6 != expected_algo {
                return Err("Application algorithm/settings disagree".into());
            }
            if let Some(id) = &a.iso_id {
                command::hex(id, 2, "ISO ID")?;
            }
            if let Some(n) = &a.df_name {
                if n.is_empty()
                    || n.len() > 32
                    || !n.len().is_multiple_of(2)
                    || !n.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err("Invalid DF name".into());
                }
            }
            let mut files = BTreeSet::new();
            for f in &a.files {
                f.validate()?;
                f.create_extra(false)?;
                if f.iso_id.is_some() && a.key_settings2 & 0x20 == 0 {
                    return Err("ISO file ID requires the application ISO flag".into());
                }
                if !files.insert(f.id) {
                    return Err("Duplicate file ID".into());
                }
            }
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = crate::storage::read_bounded(path, 8 * 1024 * 1024)?;
        let a: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        a.validate()?;
        Ok(a)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        crate::storage::write_private(
            path,
            &serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
    }
}
fn err(e: impl Into<String>) -> Outcome {
    Outcome::Failed(e.into())
}
fn command(
    a: &Auth,
    verb: &str,
    aid: &str,
    fid: Option<u8>,
    extra: &str,
    kind: Kind,
    mode: Communication,
) -> Result<CommandSpec, Outcome> {
    a.spec(verb, aid, fid, extra, kind, mode).map_err(err)
}
pub fn bind(c: &mut dyn Client, uid: &str) -> Result<(), Outcome> {
    crate::backup::validate_uid(uid).map_err(err)?;
    let raw = c.run(CommandSpec::simple("hf 14a info", Kind::Detect))?;
    if crate::backup::uid(&raw).as_deref() != Some(uid) {
        return Err(err("Target UID changed or unreadable"));
    }
    Ok(())
}
pub fn raw_blob(raw: &str, label: &str) -> Result<Vec<u8>, String> {
    let clean = raw
        .lines()
        .map(crate::parse::clean)
        .collect::<Vec<_>>()
        .join("\n");
    let re = regex::Regex::new(&format!(
        r"(?i){}\[(\d+)\]:\s*((?:[0-9a-f]{{2}}\s*)+)",
        regex::escape(label)
    ))
    .unwrap();
    let all = re.captures_iter(&clean).collect::<Vec<_>>();
    if all.len() != 1 {
        return Err(format!("Expected one structured {label} result"));
    }
    let hex = all[0][2].split_whitespace().collect::<String>();
    let n = all[0][1]
        .parse::<usize>()
        .map_err(|_| "Invalid result size")?;
    if hex.len() != n * 2 {
        return Err("Incomplete metadata bytes".into());
    }
    Ok((0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect())
}
pub fn file_settings(raw: &str, id: u8) -> Result<File, String> {
    let b = raw_blob(raw, "settings")?;
    if b.len() < 4 || b[0] > 4 || b[1] & !3 != 0 {
        return Err("Unsupported/extended file settings; no partial archive claimed".into());
    }
    let mut f = File::empty(id);
    f.kind = b[0];
    f.mode = match b[1] {
        0 => Communication::Plain,
        1 => Communication::Mac,
        3 => Communication::Encrypt,
        _ => return Err("Unknown file communication mode".into()),
    };
    f.rights = u16::from_le_bytes([b[2], b[3]]);
    let u24 = |i| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], 0]);
    match f.kind {
        0 | 1 => {
            if b.len() != 7 {
                return Err("Unexpected data settings size".into());
            }
            f.size = u24(4);
        }
        2 => {
            if b.len() != 17 {
                return Err("Unexpected value settings size".into());
            }
            f.lower = i32::from_le_bytes(b[4..8].try_into().unwrap());
            f.upper = i32::from_le_bytes(b[8..12].try_into().unwrap());
            f.limited = b[16];
        }
        3 | 4 => {
            if b.len() != 13 {
                return Err("Unexpected record settings size".into());
            }
            f.record_size = u24(4);
            f.max_records = u24(7);
            let count = u24(10);
            if count > f.max_records || count > 100000 {
                return Err("Invalid record count".into());
            }
            f.records = vec![vec![]; count as usize];
        }
        _ => unreachable!(),
    }
    Ok(f)
}
pub fn app_settings(raw: &str) -> Result<(u8, u8), String> {
    let b = raw_blob(raw, "GetKeySettings")?;
    if b.len() != 2 || b[1] & 0x10 != 0 || b[1] & 15 == 0 || b[1] & 15 > 14 {
        return Err("Extended/invalid application key settings are unsupported".into());
    }
    Ok((b[0], b[1]))
}
pub fn ids(raw: &str, applications: bool) -> Result<Vec<String>, Outcome> {
    let kind = if applications {
        Kind::Applications
    } else {
        Kind::Files
    };
    let e = crate::parse::evidence(raw, &kind);
    let lower = raw.to_lowercase();
    if applications && !lower.contains("aid list") && !lower.contains("no applications") {
        return Err(err("Unrecognised application inventory"));
    }
    if !applications && !lower.contains("file id") && !lower.contains("no files") {
        return Err(err("Unrecognised file inventory"));
    }
    Ok(if applications { e.aids } else { e.files })
}
fn inventory(
    c: &mut dyn Client,
    creds: &Credentials,
    aid: &str,
    apps: bool,
) -> Result<Vec<String>, Outcome> {
    let a = creds.directory(aid);
    let raw = c.run(command(
        &a,
        if apps { "getaids" } else { "getfileids" },
        aid,
        None,
        "",
        if apps {
            Kind::Applications
        } else {
            Kind::Files
        },
        Communication::Plain,
    )?)?;
    ids(&raw, apps)
}
fn iso_files(raw: &str) -> Result<BTreeMap<u8, Option<u16>>, String> {
    let re =
        regex::Regex::new(r"(?i)^\s*(?:\[\+\]\s*)?([0-9a-f]{2})\s*\|\s*([0-9a-f]{4}|n/a|)\s*\|")
            .unwrap();
    let mut map = BTreeMap::new();
    for line in raw.lines() {
        let line = crate::parse::clean(line);
        if let Some(c) = re.captures(&line) {
            let id = u8::from_str_radix(&c[1], 16).unwrap();
            let iso = if c[2].is_empty() || c[2].eq_ignore_ascii_case("n/a") {
                None
            } else {
                Some(u16::from_str_radix(&c[2], 16).unwrap())
            };
            if map.insert(id, iso).is_some() {
                return Err("Duplicate ISO file mapping".into());
            }
        }
    }
    Ok(map)
}
fn settings(c: &mut dyn Client, a: &Auth, aid: &str, id: u8) -> Result<File, Outcome> {
    let raw = c.run(command(
        a,
        "getfilesettings",
        aid,
        Some(id),
        "-v",
        Kind::Settings,
        Communication::Plain,
    )?)?;
    let mut f = file_settings(&raw, id).map_err(err)?;
    let listing = c.run(command(
        a,
        "lsfiles",
        aid,
        None,
        "",
        Kind::Settings,
        Communication::Plain,
    )?)?;
    let map = iso_files(&listing).map_err(err)?;
    f.iso_id = *map
        .get(&id)
        .ok_or_else(|| err("File ISO-ID mapping is incomplete"))?;
    Ok(f)
}

pub fn value(raw: &str) -> Result<i32, String> {
    let clean = raw
        .lines()
        .map(crate::parse::clean)
        .collect::<Vec<_>>()
        .join("\n");
    let re = regex::Regex::new(
        r"(?im)(?:Value:|Read file 0x[0-9a-f]{2} value:)\s*(-?\d+)\s*\(0x[0-9a-f]{8}\)",
    )
    .unwrap();
    let all = re.captures_iter(&clean).collect::<Vec<_>>();
    if all.len() != 1 {
        return Err("Missing or ambiguous value read".into());
    }
    all[0][1].parse().map_err(|_| "Value overflow".into())
}
fn read_file(c: &mut dyn Client, a: &Auth, aid: &str, f: &File) -> Result<File, Outcome> {
    let mut out = f.clone();
    out.data.clear();
    match f.kind {
        0 | 1 => {
            if f.size > 1024 * 1024 {
                return Err(err("File exceeds archive limit"));
            }
            for offset in (0..f.size).step_by(192) {
                let length = (f.size - offset).min(192);
                let raw = c.run(command(
                    a,
                    "read",
                    aid,
                    Some(f.id),
                    &format!("--type data -o {offset:06X} -l {length:06X}"),
                    Kind::Read,
                    f.mode,
                )?)?;
                let e = crate::parse::evidence(&raw, &Kind::Read);
                if e.bytes.len() != length as usize {
                    return Err(err("Incomplete file data; backup refused"));
                }
                out.data.extend(e.bytes);
            }
        }
        2 => {
            let raw = c.run(command(
                a,
                "value",
                aid,
                Some(f.id),
                "-o get",
                Kind::Read,
                f.mode,
            )?)?;
            out.value = Some(value(&raw).map_err(err)?);
        }
        3 | 4 => {
            out.records.clear();
            if f.record_size > 65536 {
                return Err(err("Record exceeds archive limit"));
            }
            for n in 0..f.records.len() {
                let raw = c.run(command(
                    a,
                    "read",
                    aid,
                    Some(f.id),
                    &format!("--type record -o {n:06X} -l 000001"),
                    Kind::Read,
                    f.mode,
                )?)?;
                let e = crate::parse::evidence(&raw, &Kind::Read);
                if e.bytes.len() != f.record_size as usize {
                    return Err(err("Incomplete record data"));
                }
                out.records.push(e.bytes);
            }
        }
        _ => return Err(err("Unsupported file type")),
    }
    out.validate().map_err(err)?;
    Ok(out)
}
fn text_df_name(hex: &str) -> Result<(), Outcome> {
    let bytes = command::payload(hex).map_err(err)?;
    if bytes.iter().any(|b| !b.is_ascii_graphic()) {
        return Err(err("The selected RRG adapter cannot verify binary ISO DF names losslessly; no mutation sent"));
    }
    Ok(())
}
fn names(raw: &str) -> Result<BTreeMap<String, (String, String)>, String> {
    let re = regex::Regex::new(
        r"(?i)AID: ([0-9a-f]{6}) ISO file id: ([0-9a-f]{4}) ISO DF name\[(\d+)\]: (.*)$",
    )
    .unwrap();
    let mut out = BTreeMap::new();
    for line in raw.lines() {
        let line = crate::parse::clean(line);
        if let Some(c) = re.captures(&line) {
            let n = c[3].parse::<usize>().map_err(|_| "Invalid DF name")?;
            let name = c[4].as_bytes();
            if n != name.len() || n > 16 || name.iter().any(|b| !b.is_ascii_graphic()) {
                return Err("Non-text/truncated ISO DF name cannot be archived losslessly".into());
            }
            if out
                .insert(
                    c[1].to_uppercase(),
                    (c[2].to_uppercase(), command::encode(name)),
                )
                .is_some()
            {
                return Err("Duplicate ISO application metadata".into());
            }
        }
    }
    Ok(out)
}
pub fn capture(c: &mut dyn Client, uid: &str, creds: &Credentials) -> Result<Archive, Outcome> {
    bind(c, uid)?;
    let aids = inventory(c, creds, "000000", true)?;
    let a = creds.directory("000000");
    let raw = c.run(command(
        &a,
        "getappnames",
        "000000",
        None,
        "",
        Kind::Settings,
        Communication::Plain,
    )?)?;
    let iso = names(&raw).map_err(err)?;
    let mut applications = vec![];
    let iso_id_re = regex::Regex::new(r"(?im)File ID:\s*([0-9a-f]{4})\b").unwrap();
    for aid in &aids {
        bind(c, uid)?;
        let a = creds.directory(aid);
        let raw = c.run(command(
            &a,
            "getkeysettings",
            aid,
            None,
            "-v",
            Kind::Settings,
            Communication::Plain,
        )?)?;
        let (ks1, ks2) = app_settings(&raw).map_err(err)?;
        let algorithm = match ks2 >> 6 {
            0 => {
                if a.algorithm == Algorithm::TwoTdea {
                    Algorithm::TwoTdea
                } else {
                    Algorithm::Des
                }
            }
            1 => Algorithm::ThreeTdea,
            2 => Algorithm::Aes,
            _ => return Err(err("Unsupported application algorithm")),
        };
        let (iso_id, df_name) = if ks2 & 0x20 != 0 {
            let (i, n) = iso
                .get(aid)
                .ok_or_else(|| err("ISO app metadata incomplete"))?;
            (Some(i.clone()), (!n.is_empty()).then(|| n.clone()))
        } else {
            (None, None)
        };
        let iso_raw = c.run(command(
            &a,
            "getfileisoids",
            aid,
            None,
            "",
            Kind::Files,
            Communication::Plain,
        )?)?;
        let iso_list = iso_id_re
            .captures_iter(&iso_raw)
            .map(|c| u16::from_str_radix(&c[1], 16).unwrap())
            .collect::<BTreeSet<_>>();
        let ids = inventory(c, creds, aid, false)?;
        let mut files = vec![];
        for id in &ids {
            bind(c, uid)?;
            let id = u8::from_str_radix(id, 16).map_err(|_| err("Invalid file ID"))?;
            let f = settings(c, &a, aid, id)?;
            let reader = creds.file(aid, &f, false).map_err(err)?;
            files.push(read_file(c, &reader, aid, &f)?);
            bind(c, uid)?;
        }
        if inventory(c, creds, aid, false)? != ids {
            return Err(err("File inventory changed during capture"));
        }
        let mapped = files
            .iter()
            .filter_map(|f| f.iso_id)
            .collect::<BTreeSet<_>>();
        if mapped != iso_list {
            return Err(err("ISO file-ID inventories disagree"));
        }
        let mut key_versions = vec![];
        for n in 0..ks2 & 15 {
            let raw = c.run(command(
                &a,
                "getkeyversions",
                aid,
                None,
                &format!("--keynum {n:02X} -v"),
                Kind::Settings,
                Communication::Plain,
            )?)?;
            let v = raw_blob(&raw, "GetKeyVersion").map_err(err)?;
            if v.len() != 1 {
                return Err(err("Extended key versions unsupported"));
            }
            key_versions.push(v[0]);
        }
        applications.push(Application {
            aid: aid.clone(),
            key_settings: ks1,
            key_settings2: ks2,
            key_versions,
            algorithm,
            iso_id,
            df_name,
            files,
        });
    }
    if inventory(c, creds, "000000", true)? != aids {
        return Err(err("Application inventory changed during capture"));
    }
    bind(c, uid)?;
    let mut out = Archive {
        schema: 1,
        uid: uid.into(),
        applications,
        sha256: String::new(),
    };
    out.seal();
    out.validate().map_err(err)?;
    Ok(out)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub schema: u32,
    pub archive_hash: String,
    pub target_uid: String,
    pub completed: BTreeSet<String>,
    pub created: BTreeSet<String>,
    pub pending: Option<String>,
}
impl Journal {
    pub fn save(&self, path: &Path) -> Result<(), Outcome> {
        crate::storage::write_private(
            path,
            &serde_json::to_vec_pretty(self).map_err(|_| err("Cannot encode restore journal"))?,
        )
        .map_err(err)
    }
    pub fn start(archive: &Archive, uid: &str, path: &Path) -> Result<Self, Outcome> {
        if path.exists() {
            let bytes = crate::storage::read_bounded(path, 1024 * 1024).map_err(err)?;
            let j: Self = serde_json::from_slice(&bytes).map_err(|_| err("Invalid journal"))?;
            if j.schema != 1 || j.archive_hash != archive.sha256 || j.target_uid != uid {
                return Err(err("Journal belongs to a different archive or target"));
            }
            Ok(j)
        } else {
            let j = Self {
                schema: 1,
                archive_hash: archive.sha256.clone(),
                target_uid: uid.into(),
                completed: BTreeSet::new(),
                created: BTreeSet::new(),
                pending: None,
            };
            j.save(path)?;
            Ok(j)
        }
    }
}
fn mutate(
    c: &mut dyn Client,
    uid: &str,
    j: &mut Journal,
    path: &Path,
    label: &str,
    spec: CommandSpec,
) -> Result<(), Outcome> {
    bind(c, uid)?;
    j.pending = Some(label.into());
    j.save(path)?;
    c.run(spec)?;
    bind(c, uid)?;
    Ok(())
}
fn done(j: &mut Journal, path: &Path, label: &str) -> Result<(), Outcome> {
    j.completed.insert(label.into());
    if j.pending.as_deref() == Some(label) || label == "complete" {
        j.pending = None;
    }
    j.save(path)
}
fn authenticated(c: &mut dyn Client, a: &Auth, aid: &str) -> Result<bool, Outcome> {
    let spec = command(
        a,
        "auth",
        aid,
        None,
        "",
        Kind::Authenticate,
        Communication::Plain,
    )?;
    match c.run(spec) {
        Ok(raw) => Ok(crate::parse::evidence(&raw, &Kind::Authenticate).authenticated),
        Err(Outcome::Failed(_)) => Ok(false),
        Err(other) => Err(other),
    }
}

fn restore_auth(
    c: &mut dyn Client,
    creds: &Credentials,
    a: &Application,
    j: &Journal,
) -> Result<Auth, Outcome> {
    let desired = creds.get(&a.aid, 0).map_err(err)?;
    if authenticated(c, &desired, &a.aid)? {
        return Ok(desired);
    }
    if j.created.contains(&a.aid) {
        let initial = Auth::zero(a.algorithm, desired.channel);
        if authenticated(c, &initial, &a.aid)? {
            return Ok(initial);
        }
    }
    Err(err(
        "Neither supplied nor freshly-created application key authenticates; no mutation sent",
    ))
}
fn access_for_restore(
    c: &mut dyn Client,
    creds: &Credentials,
    a: &Application,
    f: &File,
    write: bool,
    j: &Journal,
) -> Result<Auth, Outcome> {
    let desired = creds.file(&a.aid, f, write).map_err(err)?;
    if desired.no_auth || authenticated(c, &desired, &a.aid)? {
        return Ok(desired);
    }
    if j.created.contains(&a.aid) {
        let mut initial = Auth::zero(a.algorithm, desired.channel);
        initial.key_number = desired.key_number;
        if authenticated(c, &initial, &a.aid)? {
            return Ok(initial);
        }
    }
    Err(err(
        "Supplied access key does not authenticate; no data mutation sent",
    ))
}
struct RestoreLock {
    _file: std::fs::File,
}
impl RestoreLock {
    fn acquire(path: &Path) -> Result<Self, Outcome> {
        let lock_path = path.with_extension("restore.lock");
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let file = options
            .open(lock_path)
            .map_err(|_| err("Restore journal is locked or its folder is unavailable"))?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(err("Another restore owns this journal"));
            }
        }
        Ok(Self { _file: file })
    }
}
pub fn restore(
    c: &mut dyn Client,
    archive: &Archive,
    uid: &str,
    creds: &Credentials,
    journal_path: &Path,
) -> Result<Journal, Outcome> {
    archive.validate().map_err(err)?;
    let _lock = RestoreLock::acquire(journal_path)?;
    crate::backup::validate_uid(uid).map_err(err)?;
    // Validate every destination key and size before creating anything.
    for a in &archive.applications {
        if let Some(name) = &a.df_name {
            text_df_name(name)?;
        }
        for n in 0..a.key_settings2 & 15 {
            let key = creds.get(&a.aid, n).map_err(err)?;
            command::hex(&key.key, a.algorithm.key_bytes(), "Destination key").map_err(err)?;
            if key.algorithm != a.algorithm || key.kdf != "none" {
                return Err(err("Restore requires explicit non-diversified destination keys of the application algorithm"));
            }
        }
        for f in &a.files {
            creds.file(&a.aid, f, false).map_err(err)?;
            if matches!(f.kind, 3 | 4) && f.record_size > 1024 {
                return Err(err(
                    "Configured RRG adapter supports record restores up to 1024 bytes",
                ));
            }
        }
    }
    for (verb, flags) in [
        ("createapp", vec!["--ks1", "--ks2", "--dstalgo"]),
        ("createfile", vec!["--size", "--rawrights", "--amode"]),
        (
            "createvaluefile",
            vec!["--lower", "--upper", "--value", "--lcredit"],
        ),
        ("createrecordfile", vec!["--size", "--maxrecord"]),
        ("changekey", vec!["--oldkey", "--newkey", "--newver"]),
        ("chkeysettings", vec!["-d"]),
        ("chfilesettings", vec!["--rawrights", "--amode"]),
        ("read", vec!["--type", "-o", "-l"]),
        ("write", vec!["--type", "-o", "-d", "--commit"]),
        ("clearrecfile", vec!["--fid"]),
        ("value", vec!["-o", "-d"]),
        ("lsfiles", vec![]),
    ] {
        c.inspect(&format!("hf mfdes {verb}"), &flags)?;
    }
    bind(c, uid)?;
    let mut j = Journal::start(archive, uid, journal_path)?;
    j.completed.remove("complete");
    j.save(journal_path)?;
    let existing = inventory(c, creds, "000000", true)?;
    if existing
        .iter()
        .any(|id| !archive.applications.iter().any(|a| &a.aid == id))
    {
        return Err(err(
            "Destination contains additional applications; restore will not delete them implicitly",
        ));
    }
    for app in &archive.applications {
        if existing.contains(&app.aid) {
            let access = restore_auth(c, creds, app, &j)?;
            let raw = c.run(command(
                &access,
                "getkeysettings",
                &app.aid,
                None,
                "-v",
                Kind::Settings,
                Communication::Plain,
            )?)?;
            if app_settings(&raw).map_err(err)?.1 != app.key_settings2 {
                return Err(err("Existing destination key settings differ"));
            }
            let raw = c.run(command(
                &access,
                "getfileids",
                &app.aid,
                None,
                "",
                Kind::Files,
                Communication::Plain,
            )?)?;
            for id in ids(&raw, false)? {
                let fid = u8::from_str_radix(&id, 16).map_err(|_| err("Invalid file ID"))?;
                let expected = app.files.iter().find(|f| f.id == fid).ok_or_else(|| {
                    err("Destination has extra files; no implicit deletion performed")
                })?;
                let mut current = settings(c, &access, &app.aid, fid)?;
                current.rights = expected.rights;
                if !expected.equivalent_settings(&current) {
                    return Err(err(
                        "Destination file geometry/type differs; no writes sent",
                    ));
                }
            }
        }
    }
    for a in &archive.applications {
        let app_step = format!("app:{}", a.aid);
        let current = inventory(c, creds, "000000", true)?;
        if !current.contains(&a.aid) {
            let root = creds.directory("000000");
            let mut extra = format!(
                "--ks1 0F --ks2 {:02X} --dstalgo {}",
                a.key_settings2,
                a.algorithm.cli()
            );
            if let Some(id) = &a.iso_id {
                extra.push_str(&format!(" --fid {id}"));
            }
            if let Some(name) = &a.df_name {
                extra.push_str(&format!(" --dfhex {name}"));
            }
            j.created.insert(a.aid.clone());
            j.save(journal_path)?;
            mutate(
                c,
                uid,
                &mut j,
                journal_path,
                &app_step,
                command(
                    &root,
                    "createapp",
                    &a.aid,
                    None,
                    &extra,
                    Kind::Write,
                    Communication::Plain,
                )?,
            )?;
            if !inventory(c, creds, "000000", true)?.contains(&a.aid) {
                return Err(Outcome::Mismatch);
            }
        }
        let mut auth = restore_auth(c, creds, a, &j)?;
        let raw = c.run(command(
            &auth,
            "getkeysettings",
            &a.aid,
            None,
            "-v",
            Kind::Settings,
            Communication::Plain,
        )?)?;
        if app_settings(&raw).map_err(err)?.1 != a.key_settings2 {
            return Err(err(
                "Existing application has incompatible key settings; no overwrite performed",
            ));
        }
        done(&mut j, journal_path, &app_step)?;
        for f in &a.files {
            let label = format!("file:{}:{:02X}", a.aid, f.id);
            let current_raw = c.run(command(
                &auth,
                "getfileids",
                &a.aid,
                None,
                "",
                Kind::Files,
                Communication::Plain,
            )?)?;
            if !ids(&current_raw, false)?.contains(&format!("{:02X}", f.id)) {
                let (verb, extra) = f.create_extra(true).map_err(err)?;
                mutate(
                    c,
                    uid,
                    &mut j,
                    journal_path,
                    &label,
                    command(
                        &auth,
                        &verb,
                        &a.aid,
                        Some(f.id),
                        &extra,
                        Kind::Write,
                        Communication::Plain,
                    )?,
                )?;
            }
            let mut now = settings(c, &auth, &a.aid, f.id)?;
            let mut normalized = now.clone();
            normalized.rights = f.rights;
            if !f.equivalent_settings(&normalized) {
                return Err(err(
                    "Existing file geometry/type differs; refusing destructive replacement",
                ));
            }
            let read_auth = access_for_restore(c, creds, a, &now, false, &j)?;
            let observed = read_file(c, &read_auth, &a.aid, &now)?;
            if observed.data != f.data || observed.value != f.value || observed.records != f.records
            {
                let write_auth = access_for_restore(c, creds, a, &now, true, &j)?;
                match f.kind {
                    0 | 1 => {
                        for (n, chunk) in f.data.chunks(192).enumerate() {
                            mutate(
                                c,
                                uid,
                                &mut j,
                                journal_path,
                                &label,
                                command(
                                    &write_auth,
                                    "write",
                                    &a.aid,
                                    Some(f.id),
                                    &format!(
                                        "--type data -o {:06X} -d {}{}",
                                        n * 192,
                                        command::encode(chunk),
                                        if f.kind == 1 { " --commit" } else { "" }
                                    ),
                                    Kind::Write,
                                    f.mode,
                                )?,
                            )?;
                        }
                    }
                    2 => {
                        let delta =
                            i64::from(f.value.unwrap()) - i64::from(observed.value.unwrap());
                        if delta.unsigned_abs() > i32::MAX as u64 {
                            return Err(err(
                                "Value adjustment exceeds safe signed amount; no value change sent",
                            ));
                        }
                        mutate(
                            c,
                            uid,
                            &mut j,
                            journal_path,
                            &label,
                            command(
                                &write_auth,
                                "value",
                                &a.aid,
                                Some(f.id),
                                &format!(
                                    "-o {} -d {:08X}",
                                    if delta >= 0 { "credit" } else { "debit" },
                                    delta.unsigned_abs()
                                ),
                                Kind::Write,
                                f.mode,
                            )?,
                        )?;
                    }
                    3 | 4 => {
                        mutate(
                            c,
                            uid,
                            &mut j,
                            journal_path,
                            &label,
                            command(
                                &write_auth,
                                "clearrecfile",
                                &a.aid,
                                Some(f.id),
                                "",
                                Kind::Write,
                                f.mode,
                            )?,
                        )?;
                        for record in f.records.iter().rev() {
                            mutate(
                                c,
                                uid,
                                &mut j,
                                journal_path,
                                &label,
                                command(
                                    &write_auth,
                                    "write",
                                    &a.aid,
                                    Some(f.id),
                                    &format!(
                                        "--type record -o 000000 -d {} --commit",
                                        command::encode(record)
                                    ),
                                    Kind::Write,
                                    f.mode,
                                )?,
                            )?;
                        }
                    }
                    _ => return Err(err("Unsupported file")),
                }
            }
            now = settings(c, &auth, &a.aid, f.id)?;
            let mut reader = access_for_restore(c, creds, a, &now, false, &j)?;
            let after = read_file(c, &reader, &a.aid, &now)?;
            if after.data != f.data || after.value != f.value || after.records != f.records {
                return Err(Outcome::Mismatch);
            }
            if now.rights != f.rights {
                mutate(
                    c,
                    uid,
                    &mut j,
                    journal_path,
                    &label,
                    command(
                        &auth,
                        "chfilesettings",
                        &a.aid,
                        Some(f.id),
                        &format!("--amode {} --rawrights {:04X}", f.mode.cli(), f.rights),
                        Kind::Write,
                        Communication::Encrypt,
                    )?,
                )?;
            }
            now = settings(c, &auth, &a.aid, f.id)?;
            if !f.equivalent_settings(&now) {
                return Err(Outcome::Mismatch);
            }
            reader = creds.file(&a.aid, &now, false).map_err(err)?;
            // For newly created apps final supplied keys may not yet be installed; payload was verified before access lockdown.
            if !j.created.contains(&a.aid) {
                let verified = read_file(c, &reader, &a.aid, &now)?;
                if verified.data != f.data
                    || verified.value != f.value
                    || verified.records != f.records
                {
                    return Err(Outcome::Mismatch);
                }
            }
            done(&mut j, journal_path, &label)?;
        }
        // Set non-master keys first, then master key, re-authenticating freshly with each new key.
        let count = a.key_settings2 & 15;
        let order = (1..count).chain(std::iter::once(0));
        for n in order {
            let desired = creds.get(&a.aid, n).map_err(err)?;
            let label = format!("key:{}:{n}", a.aid);
            let has_key = authenticated(c, &desired, &a.aid)?;
            let version_raw = c.run(command(
                &auth,
                "getkeyversions",
                &a.aid,
                None,
                &format!("--keynum {n:02X} -v"),
                Kind::Settings,
                Communication::Plain,
            )?)?;
            let version = raw_blob(&version_raw, "GetKeyVersion").map_err(err)?;
            if !has_key || version != vec![a.key_versions[n as usize]] {
                if !j.created.contains(&a.aid) {
                    return Err(err(
                        "An existing destination key differs; use the explicit key-change workflow",
                    ));
                }
                let old = if has_key {
                    desired.key.clone()
                } else {
                    Zeroizing::new("00".repeat(a.algorithm.key_bytes()))
                };
                let extra=Zeroizing::new(format!("--newkeyno {n} --oldalgo {} --oldkey {} --newalgo {} --newkey {} --newver {:02X}",a.algorithm.cli(),old.as_str(),a.algorithm.cli(),desired.key.as_str(),a.key_versions[n as usize]));
                mutate(
                    c,
                    uid,
                    &mut j,
                    journal_path,
                    &label,
                    command(
                        &auth,
                        "changekey",
                        &a.aid,
                        None,
                        &extra,
                        Kind::Write,
                        Communication::Encrypt,
                    )?,
                )?;
                if !authenticated(c, &desired, &a.aid)? {
                    return Err(Outcome::Mismatch);
                }
            }
            if n == 0 {
                auth = desired;
            }
            let raw = c.run(command(
                &auth,
                "getkeyversions",
                &a.aid,
                None,
                &format!("--keynum {n:02X} -v"),
                Kind::Settings,
                Communication::Plain,
            )?)?;
            if raw_blob(&raw, "GetKeyVersion").map_err(err)? != vec![a.key_versions[n as usize]] {
                return Err(Outcome::Mismatch);
            }
            done(&mut j, journal_path, &label)?;
        }
        let label = format!("policy:{}", a.aid);
        let raw = c.run(command(
            &auth,
            "getkeysettings",
            &a.aid,
            None,
            "-v",
            Kind::Settings,
            Communication::Plain,
        )?)?;
        if app_settings(&raw).map_err(err)?.0 != a.key_settings {
            mutate(
                c,
                uid,
                &mut j,
                journal_path,
                &label,
                command(
                    &auth,
                    "chkeysettings",
                    &a.aid,
                    None,
                    &format!("-d {:02X}", a.key_settings),
                    Kind::Write,
                    Communication::Encrypt,
                )?,
            )?;
        }
        let raw = c.run(command(
            &auth,
            "getkeysettings",
            &a.aid,
            None,
            "-v",
            Kind::Settings,
            Communication::Plain,
        )?)?;
        if app_settings(&raw).map_err(err)? != (a.key_settings, a.key_settings2) {
            return Err(Outcome::Mismatch);
        }
        done(&mut j, journal_path, &label)?;
    }
    bind(c, uid)?;
    let fresh = capture(c, uid, creds)?;
    if fresh.applications != archive.applications {
        return Err(Outcome::Mismatch);
    }
    done(&mut j, journal_path, "complete")?;
    Ok(j)
}
pub enum Operation {
    Admin(Box<Mutation>),
    Capture { path: PathBuf },
    Restore { archive: Archive, journal: PathBuf },
    Verify { archive: Archive },
}
pub struct Request {
    pub uid: String,
    pub creds: Credentials,
    pub operation: Operation,
    pub capabilities: crate::capabilities::Capabilities,
}

pub struct Mutation {
    pub admin: crate::desfire_admin::Admin,
    pub auth: Auth,
    pub aid: String,
    pub fid: u8,
    pub write: CommandSpec,
}
pub fn verify_mutation(c: &mut dyn Client, uid: &str, m: Mutation) -> Result<(), Outcome> {
    use crate::desfire_admin::Action;
    let a = &m.admin;
    let mut creds = Credentials {
        fallback: m.auth.clone(),
        keys: BTreeMap::new(),
    };
    creds
        .keys
        .insert((m.aid.clone(), m.auth.key_number), m.auth.clone());
    for (verb, flags) in [
        ("getaids", vec![]),
        ("getappnames", vec![]),
        ("getfileids", vec![]),
        ("getkeysettings", vec!["-v"]),
        ("getkeyversions", vec!["--keynum", "-v"]),
        ("getfilesettings", vec!["--fid", "-v"]),
        ("lsfiles", vec![]),
        ("auth", vec!["-k"]),
        ("read", vec!["--type", "-o", "-l"]),
        ("value", vec!["-o"]),
    ] {
        c.inspect(&format!("hf mfdes {verb}"), &flags)?;
    }
    bind(c, uid)?;
    let before = if matches!(
        a.action,
        Action::WriteData
            | Action::WriteRecord
            | Action::Credit
            | Action::Debit
            | Action::LimitedCredit
            | Action::ClearValue
            | Action::ClearRecords
    ) {
        let s = settings(c, &m.auth, &m.aid, m.fid)?;
        Some(read_file(c, &m.auth, &m.aid, &s)?)
    } else {
        None
    };
    if let Some(f) = &before {
        match a.action {
            Action::WriteData => {
                let bytes = command::payload(&a.data).map_err(err)?;
                if !matches!(f.kind, 0 | 1)
                    || a.offset
                        .checked_add(bytes.len() as u32)
                        .is_none_or(|n| n > f.size)
                {
                    return Err(err("Write does not fit the detected data file"));
                }
            }
            Action::WriteRecord => {
                if !matches!(f.kind, 3 | 4)
                    || command::payload(&a.data).map_err(err)?.len() != f.record_size as usize
                    || a.update_record
                        .is_some_and(|n| n as usize >= f.records.len())
                    || (a.update_record.is_none()
                        && f.kind == 3
                        && f.records.len() >= f.max_records as usize)
                {
                    return Err(err(
                        "Record type, length, index or capacity does not permit the write",
                    ));
                }
            }
            Action::Credit | Action::Debit | Action::LimitedCredit | Action::ClearValue => {
                if a.action == Action::LimitedCredit && f.limited & 1 == 0 {
                    return Err(err("Limited credit is not enabled for this file"));
                }
                if f.kind != 2 {
                    return Err(err("Select a value file for this operation"));
                }
                let new = if a.action == Action::ClearValue {
                    i64::from(f.lower)
                } else {
                    i64::from(f.value.unwrap())
                        + if a.action == Action::Debit {
                            -i64::from(a.value)
                        } else {
                            i64::from(a.value)
                        }
                };
                if new < i64::from(f.lower) || new > i64::from(f.upper) {
                    return Err(err("Value would exceed detected file limits"));
                }
            }
            Action::ClearRecords if !matches!(f.kind, 3 | 4) => {
                return Err(err("Select a record file"))
            }
            _ => {}
        }
    }
    if a.action == Action::CreateApp && !a.df_name.is_empty() {
        text_df_name(&a.df_name)?;
    }
    c.run(m.write)?;
    bind(c, uid)?;
    match a.action {
        Action::CreateApp => {
            if !inventory(c, &creds, "000000", true)?.contains(&m.aid) {
                return Err(Outcome::Mismatch);
            }
            let fresh = Auth::zero(
                a.algorithm,
                if matches!(a.algorithm, Algorithm::Aes | Algorithm::ThreeTdea) {
                    Channel::Ev1
                } else {
                    m.auth.channel
                },
            );
            let raw = c.run(command(
                &fresh,
                "getkeysettings",
                &m.aid,
                None,
                "-v",
                Kind::Settings,
                Communication::Plain,
            )?)?;
            let ks = app_settings(&raw).map_err(err)?;
            let top = match a.algorithm {
                Algorithm::Des | Algorithm::TwoTdea => 0,
                Algorithm::ThreeTdea => 0x40,
                Algorithm::Aes => 0x80,
            };
            let expected2 = top | a.numkeys | if a.iso_id.is_empty() { 0 } else { 0x20 };
            if ks
                != (
                    u8::from_str_radix(&a.ks1, 16).map_err(|_| err("Invalid key settings"))?,
                    expected2,
                )
            {
                return Err(Outcome::Mismatch);
            }
            if !a.iso_id.is_empty() {
                let root = creds.directory("000000");
                let raw = c.run(command(
                    &root,
                    "getappnames",
                    "000000",
                    None,
                    "",
                    Kind::Settings,
                    Communication::Plain,
                )?)?;
                let expected = (
                    command::hex(&a.iso_id, 2, "ISO ID").map_err(err)?,
                    a.df_name
                        .split_whitespace()
                        .collect::<String>()
                        .to_uppercase(),
                );
                if names(&raw).map_err(err)?.get(&m.aid) != Some(&expected) {
                    return Err(Outcome::Mismatch);
                }
            }
        }
        Action::DeleteApp => {
            if inventory(c, &creds, "000000", true)?.contains(&m.aid) {
                return Err(Outcome::Mismatch);
            }
        }
        Action::DeleteFile => {
            if inventory(c, &creds, &m.aid, false)?.contains(&format!("{:02X}", m.fid)) {
                return Err(Outcome::Mismatch);
            }
        }
        Action::ChangeKey => {
            let mut fresh = m.auth.clone();
            fresh.key = a.newkey.clone();
            fresh.algorithm = a.algorithm;
            fresh.key_number = a.newkeyno;
            fresh.no_auth = false;
            fresh.kdf = "none".into();
            fresh.kdf_input.clear();
            if !authenticated(c, &fresh, &m.aid)? {
                return Err(Outcome::Mismatch);
            }
            let raw = c.run(command(
                &fresh,
                "getkeyversions",
                &m.aid,
                None,
                &format!("--keynum {:02X} -v", a.newkeyno),
                Kind::Settings,
                Communication::Plain,
            )?)?;
            if raw_blob(&raw, "GetKeyVersion").map_err(err)?
                != vec![u8::from_str_radix(&a.version, 16).map_err(|_| err("Invalid key version"))?]
            {
                return Err(Outcome::Mismatch);
            }
        }
        Action::ChangeKeySettings => {
            let raw = c.run(command(
                &m.auth,
                "getkeysettings",
                &m.aid,
                None,
                "-v",
                Kind::Settings,
                Communication::Plain,
            )?)?;
            if app_settings(&raw).map_err(err)?.0
                != u8::from_str_radix(&a.ks1, 16).map_err(|_| err("Invalid key settings"))?
            {
                return Err(Outcome::Mismatch);
            }
        }
        Action::ChangeRights => {
            let f = settings(c, &m.auth, &m.aid, m.fid)?;
            if f.rights
                != u16::from_str_radix(&a.rights, 16).map_err(|_| err("Invalid access rights"))?
                || f.mode != a.file_mode
            {
                return Err(Outcome::Mismatch);
            }
        }
        Action::CreateData
        | Action::CreateBackup
        | Action::CreateValue
        | Action::CreateLinear
        | Action::CreateCyclic => {
            if !inventory(c, &creds, &m.aid, false)?.contains(&format!("{:02X}", m.fid)) {
                return Err(Outcome::Mismatch);
            }
            let f = settings(c, &m.auth, &m.aid, m.fid)?;
            let kind = match a.action {
                Action::CreateData => 0,
                Action::CreateBackup => 1,
                Action::CreateValue => 2,
                Action::CreateLinear => 3,
                _ => 4,
            };
            if f.kind != kind
                || f.rights
                    != u16::from_str_radix(&a.rights, 16).map_err(|_| err("Invalid rights"))?
                || f.mode != a.file_mode
            {
                return Err(Outcome::Mismatch);
            }
            if kind <= 1 && f.size != a.size
                || kind == 2
                    && (f.lower != a.lower || f.upper != a.upper || f.limited != a.limited_credit)
                || kind >= 3 && (f.record_size != a.size || f.max_records != a.records)
            {
                return Err(Outcome::Mismatch);
            }
            if kind == 2 && read_file(c, &m.auth, &m.aid, &f)?.value != Some(a.value) {
                return Err(Outcome::Mismatch);
            }
        }
        Action::WriteData
        | Action::WriteRecord
        | Action::Credit
        | Action::Debit
        | Action::LimitedCredit
        | Action::ClearValue
        | Action::ClearRecords => {
            let old = before.as_ref().unwrap();
            let f = settings(c, &m.auth, &m.aid, m.fid)?;
            let after = read_file(c, &m.auth, &m.aid, &f)?;
            match a.action {
                Action::WriteData => {
                    let mut expected = old.data.clone();
                    let bytes = command::payload(&a.data).map_err(err)?;
                    expected[a.offset as usize..a.offset as usize + bytes.len()]
                        .copy_from_slice(&bytes);
                    if after.data != expected {
                        return Err(Outcome::Mismatch);
                    }
                }
                Action::WriteRecord => {
                    let mut expected = old.records.clone();
                    let bytes = command::payload(&a.data).map_err(err)?;
                    if let Some(n) = a.update_record {
                        expected[n as usize] = bytes;
                    } else {
                        expected.insert(0, bytes);
                        expected.truncate(old.max_records as usize);
                    }
                    if after.records != expected {
                        return Err(Outcome::Mismatch);
                    }
                }
                Action::ClearRecords => {
                    if !after.records.is_empty() {
                        return Err(Outcome::Mismatch);
                    }
                }
                _ => {
                    let expected = if a.action == Action::ClearValue {
                        old.lower
                    } else {
                        old.value
                            .unwrap()
                            .checked_add(if a.action == Action::Debit {
                                -a.value
                            } else {
                                a.value
                            })
                            .ok_or_else(|| err("Value overflow"))?
                    };
                    if after.value != Some(expected) {
                        return Err(Outcome::Mismatch);
                    }
                }
            }
        }
        _ => return Err(err("This action is not a mutation")),
    }
    bind(c, uid)?;
    Ok(())
}

#[derive(Default)]
pub struct Workspace {
    pub path: String,
    pub journal: String,
    pub references: String,
    pub loaded: Option<Archive>,
    pub message: String,
}
impl Workspace {
    pub fn ui(
        &mut self,
        ui: &mut eframe::egui::Ui,
        uid: &str,
        fallback: Auth,
        caps: &crate::capabilities::Capabilities,
    ) -> Option<(Request, bool)> {
        use eframe::egui;
        ui.heading("DESFire application archive");
        ui.label("Back up all readable applications, file settings, payloads and key versions. Secret keys, immutable UID/manufacturing data and PICC master-key transfer are excluded. Supply keys separately. Unsupported metadata stops capture rather than silently omitting it.");
        ui.horizontal(|ui| {
            ui.label("Archive JSON");
            ui.text_edit_singleline(&mut self.path);
            if ui.button("Load / inspect archive").clicked() {
                match Archive::load(Path::new(&self.path)) {
                    Ok(a) => {
                        self.loaded = Some(a);
                        self.message = "Archive integrity and content sizes verified".into()
                    }
                    Err(e) => self.message = e,
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Restore journal JSON");
            ui.text_edit_singleline(&mut self.journal);
        });
        ui.label("Optional OS-vault references (JSON array): aid, key_number, algorithm, channel, alias. The authentication fields above provide the fallback key. Restore needs every destination application key and uses raw keys (KDF none).");
        ui.add(egui::TextEdit::multiline(&mut self.references).desired_rows(3));
        if let Some(a) = &self.loaded {
            ui.label(format!(
                "Source UID {} • {} applications • SHA-256 {}",
                a.uid,
                a.applications.len(),
                a.sha256
            ));
            for app in &a.applications {
                ui.collapsing(format!("AID {} • {:?} • {} keys",app.aid,app.algorithm,app.key_settings2&15),|ui|{for f in &app.files{ui.monospace(format!("File {:02X}: type {}, mode {}, rights {:04X}, data {} bytes, {} records",f.id,f.kind,f.mode.cli(),f.rights,f.data.len(),f.records.len()));}});
            }
        }
        let mut operation = None;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Capture and verify archive").clicked() {
                operation = Some(Operation::Capture {
                    path: PathBuf::from(&self.path),
                })
            }
            if ui
                .add_enabled(
                    self.loaded.is_some(),
                    egui::Button::new("Verify current card against archive"),
                )
                .clicked()
            {
                operation = Some(Operation::Verify {
                    archive: self.loaded.clone().unwrap(),
                })
            }
            if ui
                .add_enabled(
                    self.loaded.is_some(),
                    egui::Button::new("Review restore / resume…"),
                )
                .clicked()
            {
                operation = Some(Operation::Restore {
                    archive: self.loaded.clone().unwrap(),
                    journal: PathBuf::from(&self.journal),
                })
            }
        });
        if let Some(operation) = operation {
            let restore = matches!(operation, Operation::Restore { .. });
            let built = (|| {
                crate::backup::validate_uid(uid)?;
                if self.path.trim().is_empty() || restore && self.journal.trim().is_empty() {
                    return Err("Enter archive and restore-journal paths".into());
                }
                if restore && Path::new(&self.path) == Path::new(&self.journal) {
                    return Err("Archive and journal must be different files".into());
                }
                let refs = if self.references.trim().is_empty() {
                    vec![]
                } else {
                    serde_json::from_str::<Vec<KeyRef>>(&self.references)
                        .map_err(|e| e.to_string())?
                };
                let creds = Credentials::resolve(fallback, &refs)?;
                Ok::<_, String>(Request {
                    uid: uid.to_uppercase(),
                    creds,
                    operation,
                    capabilities: caps.clone(),
                })
            })();
            match built {
                Ok(r) => return Some((r, restore)),
                Err(e) => self.message = e,
            }
        }
        ui.label(&self.message);
        None
    }
}
