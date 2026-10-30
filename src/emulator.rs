//! PC image library and managed Classic emulator buffer operations.
use crate::{
    backup::{Dump, Geometry},
    capabilities::Capabilities,
    command::{CommandSpec, Kind},
    desfire_archive::Client,
    model::Evidence,
    runner::Outcome,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Image {
    pub name: String,
    pub dump: Dump,
    pub simulated: bool,
}
impl Image {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.len() > 100 {
            return Err("Image name must contain 1–100 characters".into());
        }
        validate_image(&self.dump)
    }
}
pub fn save_image(images: &mut Vec<Image>, image: Image) -> Result<(), String> {
    image.validate()?;
    if images.iter().any(|i| i.name == image.name) {
        return Err("Choose a new image name; existing images are preserved".into());
    }
    if images.len() >= 16 {
        return Err("The PC image library holds 16 images; export and remove one first".into());
    }
    images.push(image);
    Ok(())
}
/// Studio image exports retain their demo/physical provenance. Older raw dumps
/// have no such marker and are explicitly assigned the current import context.
pub fn import_image(
    bytes: &[u8],
    extension: &str,
    geometry: Geometry,
    uid: &str,
    name: &str,
    simulated: bool,
) -> Result<Image, String> {
    if bytes.len() > 65536 {
        return Err("Image exceeds the import size limit".into());
    }
    let image = if extension.eq_ignore_ascii_case("json") {
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if value.get("dump").is_some() || value.get("simulated").is_some() {
            serde_json::from_value::<Image>(value).map_err(|e| e.to_string())?
        } else {
            Image {
                name: name.trim().into(),
                dump: crate::backup::import_memory(bytes, extension, geometry, uid)?,
                simulated,
            }
        }
    } else {
        Image {
            name: name.trim().into(),
            dump: crate::backup::import_memory(bytes, extension, geometry, uid)?,
            simulated,
        }
    };
    image.validate()?;
    Ok(image)
}
pub fn validate_image(dump: &Dump) -> Result<(), String> {
    dump.validate()?;
    dump.geometry.emulator_flag()?;
    for sector in 0..dump.geometry.sectors() {
        let offset = crate::classic::trailer(sector)? as usize * 16;
        crate::classic::access_bits(&dump.bytes[offset + 6..offset + 9])
            .map_err(|e| format!("Sector {sector}: {e}"))?;
    }
    Ok(())
}
pub fn parse_buffer(raw: &str, g: Geometry) -> Result<Vec<u8>, String> {
    g.emulator_flag()?;
    let mut blocks = std::collections::BTreeMap::new();
    for line in raw.lines() {
        let line = crate::parse::clean(line);
        let f = line.split('|').map(str::trim).collect::<Vec<_>>();
        if f.len() < 4 {
            continue;
        }
        let Ok(n) = f[1].parse::<u32>() else {
            continue;
        };
        if n >= g.units() {
            return Err("Emulator output contains blocks beyond the selected geometry".into());
        }
        let text = crate::command::hex(f[2], 16, "Emulator block")?;
        let data = crate::command::payload(&text)?;
        if blocks.insert(n, data).is_some() {
            return Err("Duplicate emulator block; read-back is ambiguous".into());
        }
    }
    if blocks.len() != g.units() as usize {
        return Err("Incomplete emulator read-back; equality has not been established".into());
    }
    Ok((0..g.units())
        .flat_map(|n| blocks.remove(&n).unwrap())
        .collect())
}
pub enum Action {
    Read(Geometry),
    Load(Image),
    Simulate(Image),
    Stop,
}
pub struct Request {
    pub action: Action,
    pub caps: Capabilities,
    pub simulated: bool,
    pub acknowledged: bool,
}
fn failed(e: impl Into<String>) -> Outcome {
    Outcome::Failed(e.into())
}
fn spec(text: String, sensitive: bool) -> Result<CommandSpec, Outcome> {
    CommandSpec::new(text, "Emulator operation", sensitive, Kind::Console).map_err(failed)
}
fn check_version(c: &mut dyn Client) -> Result<(), Outcome> {
    let raw = c.run(CommandSpec::simple("hw version", Kind::Device))?;
    if !matches!(
        crate::maintenance::compatibility(&raw),
        crate::maintenance::Compatibility::Matched(_)
    ) {
        return Err(failed(
            "Matching client and firmware are required for emulator changes",
        ));
    }
    Ok(())
}
fn read(c: &mut dyn Client, g: Geometry) -> Result<Vec<u8>, Outcome> {
    let raw = c.run(spec(
        format!("hf mf eview {}", g.emulator_flag().map_err(failed)?),
        true,
    )?)?;
    parse_buffer(&raw, g).map_err(failed)
}
pub fn require_capabilities(action: &Action, caps: &Capabilities) -> Result<(), String> {
    match action {
        Action::Read(g) => caps.require("hf mf eview", &[g.emulator_flag()?])?,
        Action::Stop => caps.require("hw break", &[])?,
        Action::Load(i) | Action::Simulate(i) => {
            caps.require("hw version", &[])?;
            caps.require("hf mf eview", &[i.dump.geometry.emulator_flag()?])?;
            if matches!(action, Action::Load(_)) {
                caps.require("hf mf esetblk", &["--blk", "-d"])?;
            } else {
                caps.require("hf mf sim", &[i.dump.geometry.emulator_flag()?])?;
                caps.require("hw break", &[])?;
            }
        }
    }
    Ok(())
}
pub fn execute(c: &mut dyn Client, r: Request) -> Result<(Outcome, Evidence), Outcome> {
    require_capabilities(&r.action, &r.caps).map_err(failed)?;
    match r.action {
        Action::Read(g) => Ok((
            Outcome::Completed,
            Evidence {
                bytes: read(c, g)?,
                verification: Some(
                    "Complete device emulator buffer read; no physical card was read".into(),
                ),
                ..Default::default()
            },
        )),
        Action::Stop => {
            // RRG's break command is fire-and-forget. Do not label RF stop verified.
            c.run(CommandSpec::simple("hw break", Kind::Console))?;
            Ok((Outcome::Completed,Evidence{verification:Some("Stop request sent. The CLI does not acknowledge RF inactivity; use the device button if uncertain.".into()),..Default::default()}))
        }
        action => {
            let (image, loading) = match action {
                Action::Load(i) => (i, true),
                Action::Simulate(i) => (i, false),
                _ => unreachable!(),
            };
            if !r.acknowledged {
                return Err(failed(
                    "Review and acknowledge this emulator operation first",
                ));
            }
            image.validate().map_err(failed)?;
            if image.simulated != r.simulated {
                return Err(failed("Demo images and physical sessions cannot be mixed"));
            }
            check_version(c)?;
            let g = image.dump.geometry;
            if loading {
                for (n, block) in image.dump.bytes.as_chunks::<16>().0.iter().enumerate() {
                    c.run(spec(
                        format!(
                            "hf mf esetblk --blk {n} -d {}",
                            crate::command::encode(block)
                        ),
                        true,
                    )?)?;
                }
            }
            let actual = read(c, g)?;
            if actual != image.dump.bytes {
                return Err(Outcome::Mismatch);
            }
            if !loading {
                c.run(spec(
                    format!("hf mf sim {}", g.emulator_flag().map_err(failed)?),
                    true,
                )?)?;
            }
            Ok((if loading{Outcome::Verified}else{Outcome::Completed},Evidence{bytes:actual,verification:Some(if loading{"Every byte of the selected emulator buffer matches the PC image. No physical card was written."}else{"Buffer matched immediately before RF simulation was requested. Reader acceptance and RF activity are not verified."}.into()),..Default::default()}))
        }
    }
}
