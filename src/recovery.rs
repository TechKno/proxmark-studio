//! Guided Classic key discovery. Secret output never becomes a persistent log.
use crate::{
    backup::{self, Dump, Geometry},
    classic,
    command::{self, CommandSpec, Kind},
    desfire_archive::Client,
    model::Evidence,
    runner::Outcome,
};
use std::collections::BTreeMap;
use zeroize::Zeroizing;
#[derive(Clone, PartialEq)]
pub struct Key {
    pub value: Zeroizing<String>,
    pub verified: bool,
}
impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Key")
            .field("value", &"[redacted]")
            .field("verified", &self.verified)
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Keys {
    pub uid: String,
    pub geometry: Geometry,
    pub simulated: bool,
    pub sectors: BTreeMap<u8, [Option<Key>; 2]>,
}
impl Keys {
    pub fn counts(&self) -> (usize, usize) {
        let keys = self.sectors.values().flatten().flatten();
        keys.fold((0, 0), |(n, v), k| (n + 1, v + usize::from(k.verified)))
    }
    pub fn map(&self) -> Result<Zeroizing<String>, String> {
        let mut result = Zeroizing::new(String::new());
        for (&s, keys) in &self.sectors {
            let (t, k) = keys
                .iter()
                .enumerate()
                .find_map(|(i, k)| {
                    k.as_ref()
                        .filter(|k| k.verified)
                        .map(|k| (if i == 0 { "A" } else { "B" }, k))
                })
                .ok_or_else(|| format!("Sector {s} has no confirmed key"))?;
            result.push_str(&format!(
                "{s},{t},{},{t},{}\n",
                k.value.as_str(),
                k.value.as_str()
            ));
        }
        Ok(result)
    }
    pub fn complete(&self) -> bool {
        self.sectors.len() == self.geometry.sectors() as usize
            && self
                .sectors
                .values()
                .all(|v| v.iter().all(|k| k.as_ref().is_some_and(|k| k.verified)))
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Method {
    Dictionary,
    Automatic,
}
pub struct Request {
    pub uid: String,
    pub geometry: Geometry,
    pub method: Method,
    pub seed: Zeroizing<String>,
    pub sector: u8,
    pub key_b: bool,
    pub dictionary: Vec<u8>,
    pub simulated: bool,
    pub acknowledged: bool,
}
fn fail(e: impl Into<String>) -> Outcome {
    Outcome::Failed(e.into())
}
pub fn spec(text: String) -> Result<CommandSpec, Outcome> {
    CommandSpec::new(text, "Private Classic operation", true, Kind::Console).map_err(fail)
}
pub fn identity(c: &mut dyn Client, expected: &str, g: Geometry) -> Result<(), Outcome> {
    let raw = c.run(CommandSpec::simple("hf 14a info", Kind::Detect))?;
    let uid = backup::uid(&raw).ok_or_else(|| fail("No stable Classic UID observed"))?;
    if uid != expected {
        return Err(fail("Card changed; operation stopped"));
    }
    if !g.matches_model(&raw) {
        return Err(fail(
            "Detected Classic geometry differs from the selected geometry",
        ));
    }
    Ok(())
}
pub fn version(c: &mut dyn Client) -> Result<(), Outcome> {
    let raw = c.run(CommandSpec::simple("hw version", Kind::Device))?;
    if !matches!(
        crate::maintenance::compatibility(&raw),
        crate::maintenance::Compatibility::Matched(_)
    ) {
        return Err(fail("Matching client and firmware are required"));
    }
    Ok(())
}
pub fn block(c: &mut dyn Client, n: u32, key: &str, b: bool) -> Result<Vec<u8>, Outcome> {
    let raw = c.run(spec(format!(
        "hf mf rdbl --blk {n} {} -k {key}",
        if b { "-b" } else { "-a" }
    ))?)?;
    let bytes = crate::parse::evidence(&raw, &Kind::Read).bytes;
    if bytes.len() != 16 {
        return Err(fail("Incomplete Classic block read"));
    }
    Ok(bytes)
}
pub fn parse_keys(raw: &str, uid: String, g: Geometry, simulated: bool) -> Result<Keys, String> {
    let mut sectors: BTreeMap<u8, [Option<Key>; 2]> = BTreeMap::new();
    for line in raw.lines() {
        let line = crate::parse::clean(line);
        let line = line.trim().strip_prefix("[+]").unwrap_or(&line).trim();
        let f = line.split('|').map(str::trim).collect::<Vec<_>>();
        if f.len() != 6 {
            continue;
        }
        let Ok(s) = f[0].parse::<u8>() else { continue };
        if s >= g.sectors() || f[1].parse::<u32>().ok() != Some(classic::trailer(s)?) {
            return Err("Invalid key-table sector/block".into());
        }
        let mut keys = [None, None];
        for i in 0..2 {
            let marker = f[3 + i * 2];
            if marker == "0" {
                continue;
            }
            if marker.len() != 1 || !"1DKUNHSARC".contains(marker) {
                return Err("Unknown key recovery result marker".into());
            }
            keys[i] = Some(Key {
                value: Zeroizing::new(command::hex(f[2 + i * 2], 6, "Recovered key")?),
                verified: false,
            });
        }
        if let Some(previous) = sectors.get_mut(&s) {
            for i in 0..2 {
                match (&previous[i], &keys[i]) {
                    (Some(a), Some(b)) if a != b => return Err("Conflicting recovered keys".into()),
                    (None, Some(_)) => previous[i] = keys[i].clone(),
                    _ => {}
                }
            }
        } else {
            sectors.insert(s, keys);
        }
    }
    if sectors.is_empty() {
        return Err("No structured recovered-key table returned".into());
    }
    Ok(Keys {
        uid,
        geometry: g,
        simulated,
        sectors,
    })
}
pub fn dictionary(input: &[u8]) -> Result<Vec<u8>, String> {
    if input.len() > 1024 * 1024 {
        return Err("Dictionary exceeds 1 MiB".into());
    }
    let text = std::str::from_utf8(input).map_err(|_| "Dictionary must be UTF-8")?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let key = command::hex(line, 6, "Dictionary key")?;
        out.extend_from_slice(key.as_bytes());
        out.push(b'\n');
    }
    if out.is_empty() {
        return Err("Dictionary contains no keys".into());
    }
    Ok(out)
}
pub fn recover(c: &mut dyn Client, r: Request) -> Result<(Outcome, Evidence), Outcome> {
    if !r.acknowledged {
        return Err(fail("Review key recovery first"));
    }
    if !r.geometry.classic() || r.sector >= r.geometry.sectors() {
        return Err(fail("Invalid Classic geometry/sector"));
    }
    let uid = r.uid.trim().to_uppercase();
    backup::validate_uid(&uid).map_err(fail)?;
    let flag = r.geometry.emulator_flag().map_err(fail)?;
    let cmd = if r.method == Method::Automatic {
        "hf mf autopwn"
    } else {
        "hf mf fchk"
    };
    c.inspect(cmd, &[flag])?;
    c.inspect("hf mf rdbl", &["--blk", "-k", "-a", "-b"])?;
    c.inspect("hf 14a info", &[])?;
    c.inspect("hw version", &[])?;
    let mut line = format!("{cmd} {flag}");
    if r.method == Method::Automatic {
        c.inspect(cmd, &["--ns"])?;
        line.push_str(" --ns");
    }
    if !r.seed.trim().is_empty() {
        let key = command::hex(&r.seed, 6, "Known key").map_err(fail)?;
        c.inspect(cmd, &["-k"])?;
        line.push_str(&format!(" -k {key}"));
        if r.method == Method::Automatic {
            c.inspect(cmd, &["-s", if r.key_b { "-b" } else { "-a" }])?;
            line.push_str(&format!(
                " -s {} {}",
                r.sector,
                if r.key_b { "-b" } else { "-a" }
            ));
        }
    }
    let temp = tempfile::Builder::new()
        .prefix("pm3-keys-")
        .tempdir_in(std::env::temp_dir())
        .map_err(|e| fail(e.to_string()))?;
    if !r.dictionary.is_empty() {
        c.inspect(cmd, &["-f"])?;
        let path = temp.path().join("keys.dic");
        let mut bytes = Zeroizing::new(dictionary(&r.dictionary).map_err(fail)?);
        crate::storage::write_private(&path, &bytes).map_err(fail)?;
        bytes.clear();
        let path = path
            .to_str()
            .ok_or_else(|| fail("Dictionary path is not UTF-8"))?;
        if path.contains(['"', ';']) {
            return Err(fail("Unsupported dictionary path"));
        }
        line.push_str(&format!(" -f \"{path}\""));
    }
    version(c)?;
    identity(c, &uid, r.geometry)?;
    let raw =
        c.run(CommandSpec::new(line, "Recover Classic keys", true, Kind::Recovery).map_err(fail)?)?;
    let mut keys = parse_keys(&raw, uid.clone(), r.geometry, r.simulated).map_err(fail)?;
    identity(c, &uid, r.geometry)?;
    for (&s, pair) in &mut keys.sectors {
        let n = classic::trailer(s).map_err(fail)?;
        let first = if s < 32 {
            u32::from(s) * 4
        } else {
            128 + u32::from(s - 32) * 16
        };
        let mut trailer = None;
        for (i, k) in pair.iter_mut().enumerate() {
            if let Some(k) = k {
                match block(c, first, &k.value, i == 1) {
                    Ok(_) => k.verified = true,
                    Err(Outcome::Cancelled) => return Err(Outcome::Cancelled),
                    Err(Outcome::TimedOut) => return Err(Outcome::TimedOut),
                    Err(_) => {}
                }
                if i == 0 && k.verified {
                    trailer = block(c, n, &k.value, false).ok();
                }
            }
        }
        if let (Some(t), Some(b)) = (trailer, pair[1].as_mut()) {
            if classic::access_bits(&t[6..9]).is_ok_and(|v| matches!(v[3], 0..=2))
                && command::encode(&t[10..16]) == *b.value
            {
                b.verified = true;
            }
        }
        identity(c, &uid, r.geometry)?;
    }
    let (n, v) = keys.counts();
    if n == 0 {
        return Err(fail("No keys recovered"));
    }
    Ok((
        Outcome::Completed,
        Evidence {
            uid: Some(uid),
            verification: Some(format!(
                "{n} keys found; {v} independently confirmed. {}",
                if keys.complete() {
                    "All sectors have confirmed A/B key values."
                } else {
                    "Incomplete recovery; a complete clone is not ready."
                }
            )),
            recovered: Some(keys),
            ..Default::default()
        },
    ))
}
pub fn capture(
    c: &mut dyn Client,
    keys: Keys,
    simulated: bool,
) -> Result<(Outcome, Evidence), Outcome> {
    if keys.simulated != simulated || !keys.complete() {
        return Err(fail(
            "A complete confirmed key set in this session is required",
        ));
    }
    let g = keys.geometry;
    let mut bytes = Vec::new();
    identity(c, &keys.uid, g)?;
    for n in 0..g.units() {
        let s = classic::sector(n).map_err(fail)?;
        let pair = &keys.sectors[&s];
        let a = pair[0].as_ref().unwrap();
        let b = pair[1].as_ref().unwrap();
        let mut row = match block(c, n, &a.value, false) {
            Ok(v) => v,
            Err(Outcome::Cancelled) => return Err(Outcome::Cancelled),
            Err(Outcome::TimedOut) => return Err(Outcome::TimedOut),
            Err(_) => block(c, n, &b.value, true)?,
        };
        if n == classic::trailer(s).map_err(fail)? {
            classic::access_bits(&row[6..9]).map_err(fail)?;
            row[..6].copy_from_slice(&command::payload(&a.value).map_err(fail)?);
            row[10..16].copy_from_slice(&command::payload(&b.value).map_err(fail)?);
        }
        bytes.extend(row);
        identity(c, &keys.uid, g)?;
    }
    let dump = Dump::new(g, &keys.uid, bytes.clone()).map_err(fail)?;
    Ok((Outcome::Completed,Evidence{uid:Some(keys.uid),bytes,memory_dump:Some(dump),verification:Some("Complete readable Classic image captured; trailer keys supplied from independently confirmed recovery.".into()),..Default::default()}))
}
