//! Synthetic DESFire card state for tests/simulation. Stores hashes of synthetic keys only.
use crate::{
    desfire_archive::{Application, Archive, File},
    model::{Algorithm, Communication},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};
#[derive(Clone, Serialize, Deserialize)]
pub struct Card {
    pub archive: Archive,
    pub hashes: BTreeMap<String, String>,
    pub operations: usize,
    pub fail_after: Option<usize>,
    pub ignore_writes: bool,
}
fn hash(key: &str) -> String {
    format!("{:x}", Sha256::digest(key.to_ascii_uppercase()))
}
impl Default for Card {
    fn default() -> Self {
        let mut data = File::empty(1);
        data.size = 4;
        data.data = vec![0xde, 0xad, 0xbe, 0xef];
        let app = Application {
            aid: "112233".into(),
            key_settings: 15,
            key_settings2: 0x81,
            key_versions: vec![0],
            algorithm: Algorithm::Aes,
            iso_id: None,
            df_name: None,
            files: vec![data],
        };
        let mut archive = Archive {
            schema: 1,
            uid: "04112233445566".into(),
            applications: vec![app],
            sha256: String::new(),
        };
        archive.seal();
        let mut hashes = BTreeMap::new();
        hashes.insert("112233:0".into(), hash(&"00".repeat(16)));
        hashes.insert("000000:0".into(), hash(&"00".repeat(16)));
        Self {
            archive,
            hashes,
            operations: 0,
            fail_after: None,
            ignore_writes: false,
        }
    }
}
fn opt<'a>(t: &'a [&str], name: &str) -> Option<&'a str> {
    t.iter()
        .position(|s| *s == name)
        .and_then(|i| t.get(i + 1))
        .copied()
}
fn num(t: &[&str], name: &str, base: u32, default: u32) -> Result<u32, String> {
    opt(t, name)
        .map(|v| u32::from_str_radix(v, base).map_err(|_| format!("Invalid {name}")))
        .unwrap_or(Ok(default))
}
fn bytes(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid simulated data".into());
    }
    Ok((0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect())
}
fn u24(n: u32) -> Vec<u8> {
    n.to_le_bytes()[..3].to_vec()
}
pub fn settings_bytes(f: &File) -> Vec<u8> {
    let mut b = vec![
        f.kind,
        match f.mode {
            Communication::Plain => 0,
            Communication::Mac => 1,
            Communication::Encrypt => 3,
        },
    ];
    b.extend(f.rights.to_le_bytes());
    match f.kind {
        0 | 1 => b.extend(u24(f.size)),
        2 => {
            b.extend(f.lower.to_le_bytes());
            b.extend(f.upper.to_le_bytes());
            b.extend(0_i32.to_le_bytes());
            b.push(f.limited);
        }
        3 | 4 => {
            b.extend(u24(f.record_size));
            b.extend(u24(f.max_records));
            b.extend(u24(f.records.len() as u32));
        }
        _ => {}
    }
    b
}
fn blob(label: &str, b: &[u8]) -> String {
    format!(
        "[+] {label}[{}]: {}",
        b.len(),
        b.iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    )
}
fn table(b: &[u8]) -> String {
    b.chunks(16)
        .enumerate()
        .map(|(n, b)| {
            format!(
                "[=] {n:04X} | {} | ....",
                b.iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
impl Card {
    pub fn run(&mut self, text: &str) -> Result<String, String> {
        self.operations += 1;
        if self.fail_after == Some(self.operations) {
            return Err("Injected interruption".into());
        }
        if text == "hf 14a info" {
            return Ok(format!(
                "[+] UID: {}",
                self.archive
                    .uid
                    .as_bytes()
                    .chunks(2)
                    .map(|c| std::str::from_utf8(c).unwrap())
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        if text == "hw version" {
            return Ok("Client: Iceman/master/abcdef0\nOS: Iceman/master/abcdef0".into());
        }
        let t = text.split_whitespace().collect::<Vec<_>>();
        let verb = *t.get(2).ok_or("Malformed command")?;
        let aid = opt(&t, "--aid").unwrap_or("000000").to_ascii_uppercase();
        let n = num(&t, "-n", 10, 0)?;
        let supplied = opt(&t, "-k").unwrap_or("");
        let no_auth = t.contains(&"--no-auth");
        if !no_auth {
            let auth_aid = if verb == "createapp" {
                "000000"
            } else {
                aid.as_str()
            };
            if self.hashes.get(&format!("{auth_aid}:{n}")) != Some(&hash(supplied)) {
                return Err("Authentication failed".into());
            }
        }
        if verb == "auth" {
            return Ok("[+] Application selected and authenticated succesfully".into());
        }
        if verb == "getaids" {
            return Ok(if self.archive.applications.is_empty() {
                "[+] There is no applications on the card".into()
            } else {
                format!(
                    "[+] --- AID list ( {} found )\n{}",
                    self.archive.applications.len(),
                    self.archive
                        .applications
                        .iter()
                        .map(|a| format!("[+] {} Example", a.aid))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            });
        }
        if verb == "getappnames" {
            return Ok(self
                .archive
                .applications
                .iter()
                .filter_map(|a| {
                    a.iso_id.as_ref().map(|id| {
                        let name = bytes(a.df_name.as_deref().unwrap_or("")).unwrap();
                        format!(
                            "[+] AID: {} ISO file id: {id} ISO DF name[{}]: {}",
                            a.aid,
                            name.len(),
                            String::from_utf8_lossy(&name)
                        )
                    })
                })
                .collect::<Vec<_>>()
                .join("\n"));
        }
        let mutating = [
            "createapp",
            "deleteapp",
            "createfile",
            "createvaluefile",
            "createrecordfile",
            "deletefile",
            "write",
            "value",
            "clearrecfile",
            "chfilesettings",
            "changekey",
            "chkeysettings",
        ]
        .contains(&verb)
            && !(verb == "value" && opt(&t, "-o") == Some("get"));
        if mutating && self.ignore_writes {
            return Ok("[+] Simulated completion without state change".into());
        }
        if verb == "createapp" {
            if self.archive.applications.iter().any(|a| a.aid == aid) {
                return Err("Application exists".into());
            }
            let ks2 = if let Some(v) = opt(&t, "--ks2") {
                u8::from_str_radix(v, 16).map_err(|_| "Invalid settings")?
            } else {
                let top = match opt(&t, "--dstalgo").unwrap_or("des") {
                    "aes" => 0x80,
                    "3tdea" => 0x40,
                    _ => 0,
                };
                top | num(&t, "--numkeys", 10, 14)? as u8
                    | if opt(&t, "--fid").is_some() { 0x20 } else { 0 }
            };
            let algorithm = match ks2 >> 6 {
                0 => Algorithm::Des,
                1 => Algorithm::ThreeTdea,
                2 => Algorithm::Aes,
                _ => return Err("Unsupported algorithm".into()),
            };
            let count = ks2 & 15;
            let a = Application {
                aid: aid.clone(),
                key_settings: num(&t, "--ks1", 16, 15)? as u8,
                key_settings2: ks2,
                key_versions: vec![0; count as usize],
                algorithm,
                iso_id: opt(&t, "--fid").map(String::from),
                df_name: opt(&t, "--dfhex").map(String::from),
                files: vec![],
            };
            for n in 0..count {
                self.hashes.insert(
                    format!("{aid}:{n}"),
                    hash(&"00".repeat(algorithm.key_bytes())),
                );
            }
            self.archive.applications.push(a);
            self.archive.applications.sort_by(|a, b| a.aid.cmp(&b.aid));
            return Ok("[+] Application created".into());
        }
        if verb == "deleteapp" {
            self.archive.applications.retain(|a| a.aid != aid);
            return Ok("[+] Application deleted".into());
        }
        let app = self
            .archive
            .applications
            .iter_mut()
            .find(|a| a.aid == aid)
            .ok_or("Application not found")?;
        if verb == "getkeysettings" {
            return Ok(blob(
                "GetKeySettings",
                &[app.key_settings, app.key_settings2],
            ));
        }
        if verb == "getkeyversions" {
            let n = num(&t, "--keynum", 16, 0)? as usize;
            return Ok(blob(
                "GetKeyVersion",
                &[*app.key_versions.get(n).ok_or("Unknown key")?],
            ));
        }
        if verb == "getfileisoids" {
            let ids = app
                .files
                .iter()
                .filter_map(|f| f.iso_id)
                .map(|id| format!("[+] File ID: {id:04X}"))
                .collect::<Vec<_>>();
            return Ok(if ids.is_empty() {
                "[+] There are no files in the application".into()
            } else {
                format!("[+] File ISO ID list\n{}", ids.join("\n"))
            });
        }
        if verb == "lsfiles" {
            return Ok(app
                .files
                .iter()
                .map(|f| {
                    format!(
                        "[+] {:02X} | {} | 0x{:02X} file | plain | {:04X}",
                        f.id,
                        f.iso_id.map(|id| format!("{id:04X}")).unwrap_or_default(),
                        f.kind,
                        f.rights
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"));
        }
        if verb == "getfileids" {
            return Ok(if app.files.is_empty() {
                "[+] There are no files in the application".into()
            } else {
                format!(
                    "[+] ---- File ID list ----\n{}",
                    app.files
                        .iter()
                        .map(|f| format!("[+] File ID: {:02X}", f.id))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            });
        }
        if verb == "chkeysettings" {
            app.key_settings = num(&t, "-d", 16, 15)? as u8;
            return Ok("[+] Key settings changed".into());
        }
        if verb == "changekey" {
            let keyno = num(&t, "--newkeyno", 10, 0)? as u8;
            let key = opt(&t, "--newkey").ok_or("Missing new key")?;
            self.hashes.insert(format!("{aid}:{keyno}"), hash(key));
            app.key_versions[keyno as usize] = num(&t, "--newver", 16, 0)? as u8;
            return Ok("[+] Key changed".into());
        }
        let fid = num(&t, "--fid", 16, 1)? as u8;
        if verb == "deletefile" {
            app.files.retain(|f| f.id != fid);
            return Ok("[+] File deleted".into());
        }
        if ["createfile", "createvaluefile", "createrecordfile"].contains(&verb) {
            if app.files.iter().any(|f| f.id == fid) {
                return Err("File exists".into());
            }
            let mut f = File::empty(fid);
            f.mode = match opt(&t, "--amode").unwrap_or("plain") {
                "mac" => Communication::Mac,
                "encrypt" => Communication::Encrypt,
                _ => Communication::Plain,
            };
            f.iso_id = opt(&t, "--isofid")
                .map(|v| u16::from_str_radix(v, 16).map_err(|_| "Invalid ISO ID"))
                .transpose()?;
            f.rights = num(&t, "--rawrights", 16, 0xeeee)? as u16;
            f.kind = match verb {
                "createfile" => u8::from(t.contains(&"--backup")),
                "createvaluefile" => 2,
                _ => {
                    if t.contains(&"--cyclic") {
                        4
                    } else {
                        3
                    }
                }
            };
            match f.kind {
                0 | 1 => {
                    f.size = num(&t, "--size", 16, 4)?;
                    f.data = vec![0; f.size as usize];
                }
                2 => {
                    f.lower = num(&t, "--lower", 16, 0)? as i32;
                    f.upper = num(&t, "--upper", 16, 1000000)? as i32;
                    f.value = Some(num(&t, "--value", 16, 0)? as i32);
                    f.limited = num(&t, "--lcredit", 10, 0)? as u8;
                }
                _ => {
                    f.record_size = num(&t, "--size", 16, 4)?;
                    f.max_records = num(&t, "--maxrecord", 16, 8)?;
                }
            }
            app.files.push(f);
            app.files.sort_by_key(|f| f.id);
            return Ok("[+] File created".into());
        }
        let f = app
            .files
            .iter_mut()
            .find(|f| f.id == fid)
            .ok_or("File not found")?;
        if verb == "getfilesettings" {
            return Ok(blob("App file settings", &settings_bytes(f)));
        }
        if verb == "chfilesettings" {
            f.rights = num(&t, "--rawrights", 16, 0xeeee)? as u16;
            f.mode = match opt(&t, "--amode").unwrap_or("plain") {
                "mac" => Communication::Mac,
                "encrypt" => Communication::Encrypt,
                _ => Communication::Plain,
            };
            return Ok("[+] File settings changed".into());
        }
        if verb == "clearrecfile" {
            f.records.clear();
            return Ok("[+] Records cleared".into());
        }
        if verb == "value" {
            let op = opt(&t, "-o").unwrap_or("get");
            let current = f.value.ok_or("Not a value file")?;
            let amount = num(&t, "-d", 16, 0)? as i32;
            let new = match op {
                "get" => return Ok(format!("[+] Value: {current} (0x{:08X})", current as u32)),
                "credit" | "limcredit" => current.checked_add(amount),
                "debit" => current.checked_sub(amount),
                "clear" => Some(f.lower),
                _ => return Err("Unknown value operation".into()),
            }
            .ok_or("Value overflow")?;
            if new < f.lower || new > f.upper {
                return Err("Value out of bounds".into());
            }
            f.value = Some(new);
            return Ok("[+] Value updated and committed".into());
        }
        if verb == "read" {
            let offset = num(&t, "-o", 16, 0)? as usize;
            let count = num(&t, "-l", 16, 0)? as usize;
            return if opt(&t, "--type") == Some("record") {
                let end = if count == 0 {
                    f.records.len()
                } else {
                    offset.checked_add(count).ok_or("Range overflow")?
                };
                let selected = f.records.get(offset..end).ok_or("Record range invalid")?;
                Ok(table(&selected.concat()))
            } else {
                let end = if count == 0 {
                    f.data.len()
                } else {
                    offset.checked_add(count).ok_or("Range overflow")?
                };
                Ok(table(f.data.get(offset..end).ok_or("Read range invalid")?))
            };
        }
        if verb == "write" {
            let b = bytes(opt(&t, "-d").ok_or("Missing data")?)?;
            if opt(&t, "--type") == Some("record") {
                if b.len() != f.record_size as usize {
                    return Err("Wrong record length".into());
                }
                if let Some(n) = opt(&t, "--updaterec") {
                    let n = n.parse::<usize>().map_err(|_| "Bad record index")?;
                    *f.records.get_mut(n).ok_or("Missing record")? = b;
                } else {
                    if f.kind == 3 && f.records.len() >= f.max_records as usize {
                        return Err("Linear file full".into());
                    }
                    f.records.insert(0, b);
                    f.records.truncate(f.max_records as usize);
                }
            } else {
                let offset = num(&t, "-o", 16, 0)? as usize;
                let target = f
                    .data
                    .get_mut(offset..offset + b.len())
                    .ok_or("Write range invalid")?;
                target.copy_from_slice(&b);
            }
            return Ok("[+] Data written and committed".into());
        }
        Err(format!("Unknown mock DESFire verb {verb}"))
    }
}
pub fn respond(cmd: &str) -> Option<String> {
    if !Path::new(".mock-pm3-desfire").exists()
        || !(cmd.starts_with("hf mfdes ") || cmd == "hf 14a info")
    {
        return None;
    }
    let path = Path::new(".mock-pm3-desfire-state.json");
    let mut card = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Card>(&b).ok())
        .unwrap_or_default();
    let out = match card.run(cmd) {
        Ok(s) => s,
        Err(e) => format!("[!!] {e}"),
    };
    let _ = crate::storage::write_private(path, &serde_json::to_vec(&card).unwrap());
    Some(out)
}
