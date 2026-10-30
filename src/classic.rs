//! Per-sector read/write keys. Secret material is never serialised with references.
use crate::{backup::Geometry, command, vault};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use zeroize::Zeroizing;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum KeyType {
    A,
    B,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub sector: u8,
    pub read_type: KeyType,
    pub read_alias: String,
    pub write_type: KeyType,
    pub write_alias: String,
}
pub struct SectorKey {
    pub read_type: KeyType,
    pub read_key: Zeroizing<String>,
    pub write_type: KeyType,
    pub write_key: Zeroizing<String>,
}
#[derive(Default)]
pub struct KeyMap {
    pub sectors: BTreeMap<u8, SectorKey>,
}
pub fn sector(block: u32) -> Result<u8, String> {
    if block >= 256 {
        return Err("Classic block is outside 4K memory".into());
    }
    Ok(if block < 128 {
        block / 4
    } else {
        32 + (block - 128) / 16
    } as u8)
}
pub fn trailer(sector: u8) -> Result<u32, String> {
    if sector >= 40 {
        return Err("Sector must be 0–39".into());
    }
    Ok(if sector < 32 {
        u32::from(sector) * 4 + 3
    } else {
        128 + u32::from(sector - 32) * 16 + 15
    })
}
fn key_type(s: &str) -> Result<KeyType, String> {
    match s.trim() {
        "A" | "a" => Ok(KeyType::A),
        "B" | "b" => Ok(KeyType::B),
        _ => Err("Key type must be A or B".into()),
    }
}
impl KeyMap {
    pub fn parse(input: &str, g: Geometry) -> Result<Self, String> {
        if !g.classic() {
            return Err("Key maps apply only to Classic".into());
        }
        let mut map = Self::default();
        for (index, line) in input.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let parts = line.split(',').map(str::trim).collect::<Vec<_>>();
            if parts.len() != 5 {
                return Err(format!(
                    "Key-map row {} needs sector,read type,read key,write type,write key",
                    index + 1
                ));
            }
            let n = parts[0]
                .parse::<u8>()
                .map_err(|_| "Invalid sector number")?;
            if n >= g.sectors() {
                return Err("Sector exceeds selected geometry".into());
            }
            if map.sectors.contains_key(&n) {
                return Err("Duplicate sector in key map".into());
            }
            map.sectors.insert(
                n,
                SectorKey {
                    read_type: key_type(parts[1])?,
                    read_key: Zeroizing::new(command::hex(parts[2], 6, "Read key")?),
                    write_type: key_type(parts[3])?,
                    write_key: Zeroizing::new(command::hex(parts[4], 6, "Write key")?),
                },
            );
        }
        Ok(map)
    }
    pub fn resolve(refs: &[Reference], g: Geometry) -> Result<Self, String> {
        let mut map = Self::default();
        for r in refs {
            if r.sector >= g.sectors() || !g.classic() || map.sectors.contains_key(&r.sector) {
                return Err("Invalid/duplicate sector reference".into());
            }
            map.sectors.insert(
                r.sector,
                SectorKey {
                    read_type: r.read_type,
                    write_type: r.write_type,
                    read_key: vault::load(&r.read_alias, 6)?,
                    write_key: vault::load(&r.write_alias, 6)?,
                },
            );
        }
        Ok(map)
    }
    pub fn get(&self, block: u32) -> Result<&SectorKey, String> {
        self.sectors
            .get(&sector(block)?)
            .ok_or_else(|| format!("No keys supplied for sector {}", sector(block).unwrap()))
    }
}
/// Decode and validate complement redundancy in Classic access bytes 6–8.
pub fn access_bits(bytes: &[u8]) -> Result<[u8; 4], String> {
    if bytes.len() != 3 {
        return Err("Access conditions require three bytes".into());
    }
    let mut out = [0; 4];
    for (i, v) in out.iter_mut().enumerate() {
        let c1 = (bytes[1] >> (4 + i)) & 1;
        let c2 = (bytes[2] >> i) & 1;
        let c3 = (bytes[2] >> (4 + i)) & 1;
        if ((bytes[0] >> i) & 1) == c1
            || ((bytes[0] >> (4 + i)) & 1) == c2
            || ((bytes[1] >> i) & 1) == c3
        {
            return Err("Access-bit complements are invalid; trailer write refused".into());
        }
        *v = (c1 << 2) | (c2 << 1) | c3;
    }
    Ok(out)
}

pub fn encode_access(conditions: [u8; 4]) -> Result<[u8; 3], String> {
    if conditions.iter().any(|v| *v > 7) {
        return Err("Access conditions must be 0–7".into());
    }
    let mut c1 = 0;
    let mut c2 = 0;
    let mut c3 = 0;
    for (i, v) in conditions.iter().enumerate() {
        c1 |= ((v >> 2) & 1) << i;
        c2 |= ((v >> 1) & 1) << i;
        c3 |= (v & 1) << i;
    }
    Ok([
        ((!c2 & 15) << 4) | (!c1 & 15),
        (c1 << 4) | (!c3 & 15),
        (c3 << 4) | c2,
    ])
}
pub struct TrailerRequest {
    pub uid: String,
    pub geometry: Geometry,
    pub sector: u8,
    pub current_type: KeyType,
    pub current_key: Zeroizing<String>,
    pub new_a: Zeroizing<String>,
    pub new_b: Zeroizing<String>,
    pub access: [u8; 3],
    pub gpb: u8,
    pub capabilities: crate::capabilities::Capabilities,
}
impl TrailerRequest {
    pub fn validate(&self) -> Result<(), String> {
        crate::backup::validate_uid(&self.uid)?;
        if !self.geometry.classic() || self.sector >= self.geometry.sectors() {
            return Err("Sector outside Classic geometry".into());
        }
        command::hex(&self.current_key, 6, "Current key")?;
        command::hex(&self.new_a, 6, "New key A")?;
        command::hex(&self.new_b, 6, "New key B")?;
        let acl = access_bits(&self.access)?;
        if ![1, 3].contains(&acl[3]) {
            return Err("Guided trailer changes keep trailer condition 001 or 011 so keys/access settings remain manageable".into());
        }
        Ok(())
    }
}
pub fn programme_trailer(
    c: &mut dyn crate::desfire_archive::Client,
    r: TrailerRequest,
) -> Result<(), crate::runner::Outcome> {
    use crate::{command::Kind, runner::Outcome};
    let failure = |s: &str| Outcome::Failed(s.into());
    r.validate().map_err(Outcome::Failed)?;
    c.inspect("hf mf wrbl", &["--blk", "-k", "-d", "-a", "-b"])?;
    c.inspect("hf mf rdbl", &["--blk", "-k", "-a", "-b"])?;
    crate::desfire_archive::bind(c, &r.uid)?;
    let info = c.run(command::CommandSpec::simple("hf mf info", Kind::Detect))?;
    if !info.to_lowercase().contains(r.geometry.model()) {
        return Err(failure("Detected Classic geometry differs"));
    }
    let block = trailer(r.sector).map_err(Outcome::Failed)?;
    let before = c.run(
        r.geometry
            .read(block, &r.current_key, r.current_type == KeyType::B)
            .map_err(Outcome::Failed)?,
    )?;
    let e = zeroize::Zeroizing::new(crate::parse::evidence(&before, &Kind::Read).bytes);
    if e.len() != 16 {
        return Err(failure("Incomplete current trailer read"));
    }
    let acl = access_bits(&e[6..9]).map_err(Outcome::Failed)?;
    if !((acl[3] == 1 && r.current_type == KeyType::A)
        || (acl[3] == 3 && r.current_type == KeyType::B))
    {
        return Err(failure(
            "Current trailer conditions do not permit this key to change all trailer fields",
        ));
    }
    let payload = Zeroizing::new(format!(
        "{}{}{:02X}{}",
        r.new_a.as_str(),
        command::encode(&r.access),
        r.gpb,
        r.new_b.as_str()
    ));
    let spec = command::CommandSpec::new(
        format!(
            "hf mf wrbl --blk {block} -{} -k {} -d {}",
            if r.current_type == KeyType::A {
                "a"
            } else {
                "b"
            },
            r.current_key.as_str(),
            payload.as_str()
        ),
        format!("Change Classic sector {} trailer", r.sector),
        true,
        Kind::Write,
    )
    .map_err(Outcome::Failed)?;
    crate::desfire_archive::bind(c, &r.uid)?;
    c.run(spec)?;
    for (key, key_type) in [(&r.new_a, KeyType::A), (&r.new_b, KeyType::B)] {
        if key_type == KeyType::B && access_bits(&r.access).map_err(Outcome::Failed)?[3] == 1 {
            continue;
        }
        let raw = c.run(
            r.geometry
                .read(block, key, key_type == KeyType::B)
                .map_err(Outcome::Failed)?,
        )?;
        let read = zeroize::Zeroizing::new(crate::parse::evidence(&raw, &Kind::Read).bytes);
        if read.len() != 16 || read[6..9] != r.access || read[9] != r.gpb {
            return Err(Outcome::Mismatch);
        }
        if key_type == KeyType::A && access_bits(&r.access).map_err(Outcome::Failed)?[3] == 1 {
            let expected = command::hex(&r.new_b, 6, "New key B").map_err(Outcome::Failed)?;
            if command::encode(&read[10..16]) != expected {
                return Err(Outcome::Mismatch);
            }
        }
    }
    crate::desfire_archive::bind(c, &r.uid)?;
    Ok(())
}
pub struct TrailerEditor {
    pub sector: u8,
    pub new_a: Zeroizing<String>,
    pub new_b: Zeroizing<String>,
    pub conditions: [u8; 4],
    pub gpb: u8,
    pub message: String,
}
impl Default for TrailerEditor {
    fn default() -> Self {
        Self {
            sector: 0,
            new_a: Zeroizing::new(String::new()),
            new_b: Zeroizing::new(String::new()),
            conditions: [0, 0, 0, 1],
            gpb: 0x69,
            message: String::new(),
        }
    }
}
impl TrailerEditor {
    pub fn ui(
        &mut self,
        ui: &mut eframe::egui::Ui,
        uid: &str,
        geometry: Geometry,
        key: &str,
        key_type: KeyType,
    ) -> Option<TrailerRequest> {
        use eframe::egui;
        ui.heading("Controlled sector-trailer management");
        ui.label("Changes keys and access conditions. Keys are never included in result logs. The selected current key must have trailer-write permission. Guided changes retain a manageable trailer policy (001 using key A or 011 using key B); manufacturer/UID writes remain unavailable.");
        ui.horizontal(|ui| {
            ui.label("Sector");
            ui.add(
                egui::DragValue::new(&mut self.sector)
                    .range(0..=geometry.sectors().saturating_sub(1)),
            );
            ui.label("GPB");
            ui.add(egui::DragValue::new(&mut self.gpb));
        });
        ui.horizontal(|ui| {
            ui.label("New key A");
            ui.add(egui::TextEdit::singleline(&mut *self.new_a).password(true));
            ui.label("New key B");
            ui.add(egui::TextEdit::singleline(&mut *self.new_b).password(true));
        });
        ui.horizontal_wrapped(|ui| {
            for n in 0..3 {
                ui.label(format!("Data group {n} C1C2C3 (decimal)"));
                ui.add(egui::DragValue::new(&mut self.conditions[n]).range(0..=7));
            }
        });
        egui::ComboBox::from_label("New trailer policy")
            .selected_text(if self.conditions[3] == 3 {
                "011 • key B controls updates"
            } else {
                "001 • key A controls updates"
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.conditions[3], 1, "001 • key A controls updates");
                ui.selectable_value(&mut self.conditions[3], 3, "011 • key B controls updates");
            });
        if let Ok(bytes) = encode_access(self.conditions) {
            ui.monospace(format!("Encoded access bytes: {}", command::encode(&bytes)));
        }
        if ui
            .button("Review trailer change & fresh key verification…")
            .clicked()
        {
            let result = (|| {
                let request = TrailerRequest {
                    uid: uid.to_uppercase(),
                    geometry,
                    sector: self.sector,
                    current_type: key_type,
                    current_key: Zeroizing::new(key.into()),
                    new_a: self.new_a.clone(),
                    new_b: self.new_b.clone(),
                    access: encode_access(self.conditions)?,
                    gpb: self.gpb,
                    capabilities: Default::default(),
                };
                request.validate()?;
                Ok::<_, String>(request)
            })();
            match result {
                Ok(r) => return Some(r),
                Err(e) => self.message = e,
            }
        }
        ui.label(&self.message);
        None
    }
}
