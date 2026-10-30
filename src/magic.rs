//! Explicit magic-card destination checks and full Classic 1K clone verification.
use crate::{
    backup::{self, Dump, Geometry},
    classic,
    command::{self, CommandSpec, Kind},
    desfire_archive::Client,
    emulator::Image,
    model::Evidence,
    recovery,
    runner::Outcome,
};
use zeroize::Zeroizing;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Generation {
    Gen1a,
    Gen2,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub uid: String,
    pub generation: Generation,
    pub block0: Vec<u8>,
    pub simulated: bool,
}
pub enum Action {
    Inspect {
        generation: Generation,
        key: Zeroizing<String>,
    },
    Clone {
        source: Image,
        target: Target,
        key: Zeroizing<String>,
        acknowledged: bool,
    },
}
pub struct Request {
    pub action: Action,
    pub simulated: bool,
}
fn fail(s: impl Into<String>) -> Outcome {
    Outcome::Failed(s.into())
}
pub fn generation(raw: &str) -> Result<Generation, String> {
    let lines = raw
        .lines()
        .map(crate::parse::clean)
        .filter(|s| s.contains("Magic capabilities..."))
        .collect::<Vec<_>>();
    let one = lines.iter().any(|s| s.contains("Gen 1a"));
    let two = lines.iter().any(|s| s.contains("Gen 2 / CUID"));
    if lines
        .iter()
        .any(|s| s.contains("Gen 4") || s.contains("Gen 3") || s.contains("Write Once"))
        || one == two
    {
        return Err("Magic generation is ambiguous or unsupported by this adapter".into());
    }
    Ok(if one {
        Generation::Gen1a
    } else {
        Generation::Gen2
    })
}
pub fn validate_source(image: &Image) -> Result<(), String> {
    image.validate()?;
    let d = &image.dump;
    if d.geometry != Geometry::Classic1K {
        return Err("Magic cloning currently requires a complete Classic 1K image".into());
    }
    if d.uid.len() != 8
        || command::encode(&d.bytes[..4]) != d.uid
        || d.bytes[..4].iter().fold(0, |a, b| a ^ b) != d.bytes[4]
    {
        return Err("Source manufacturer block UID/BCC does not match its metadata".into());
    }
    if d.bytes[5] != 8 {
        return Err("Source must identify as a four-byte-UID Classic 1K card".into());
    }
    for s in 0..16 {
        let n = classic::trailer(s)? as usize * 16;
        let access = classic::access_bits(&d.bytes[n + 6..n + 9])?;
        if !matches!(access[3], 1 | 3) {
            return Err(format!(
                "Sector {s} uses an unsupported trailer policy; clone refused"
            ));
        }
    }
    Ok(())
}
fn backdoor_read(c: &mut dyn Client, n: u32) -> Result<Vec<u8>, Outcome> {
    let raw = c.run(recovery::spec(format!("hf mf cgetblk --blk {n}"))?)?;
    let bytes = crate::parse::evidence(&raw, &Kind::Read).bytes;
    if bytes.len() != 16 {
        return Err(fail("Incomplete magic-card read"));
    }
    Ok(bytes)
}
fn read(c: &mut dyn Client, g: Generation, n: u32, k: &str) -> Result<Vec<u8>, Outcome> {
    match g {
        Generation::Gen1a => backdoor_read(c, n),
        Generation::Gen2 => recovery::block(c, n, k, false),
    }
}
pub fn inspect(
    c: &mut dyn Client,
    g: Generation,
    key: &str,
    simulated: bool,
) -> Result<Target, Outcome> {
    let raw = c.run(CommandSpec::simple("hf 14a info", Kind::Detect))?;
    if !Geometry::Classic1K.matches_model(&raw) {
        return Err(fail("Destination is not identified as Classic 1K"));
    }
    if generation(&raw).map_err(fail)? != g {
        return Err(fail(
            "Detected magic generation differs from your selection",
        ));
    }
    let uid = backup::uid(&raw).ok_or_else(|| fail("No stable destination UID"))?;
    let b = read(c, g, 0, key)?;
    if uid.len() != 8 || command::encode(&b[..4]) != uid {
        return Err(fail("Destination UID differs from its manufacturer block"));
    }
    Ok(Target {
        uid,
        generation: g,
        block0: b,
        simulated,
    })
}
pub fn execute(c: &mut dyn Client, r: Request) -> Result<(Outcome, Evidence), Outcome> {
    c.inspect("hf 14a info", &[])?;
    c.inspect("hw version", &[])?;
    let g = match &r.action {
        Action::Inspect { generation, .. } => *generation,
        Action::Clone { target, .. } => target.generation,
    };
    if g == Generation::Gen1a {
        c.inspect("hf mf cgetblk", &["--blk"])?;
    } else {
        c.inspect("hf mf rdbl", &["--blk", "-a", "-k"])?;
    }
    match r.action {
        Action::Inspect { generation, key } => {
            let key = if generation == Generation::Gen2 {
                command::hex(&key, 6, "Destination key A").map_err(fail)?
            } else {
                String::new()
            };
            let target = inspect(c, generation, &key, r.simulated)?;
            Ok((Outcome::Completed,Evidence{uid:Some(target.uid.clone()),magic_target:Some(target),verification:Some("Destination generation and manufacturer block observed; no write performed.".into()),..Default::default()}))
        }
        Action::Clone {
            source,
            target,
            key,
            acknowledged,
        } => {
            if !acknowledged {
                return Err(fail("Review the full-card overwrite first"));
            }
            validate_source(&source).map_err(fail)?;
            if source.simulated != r.simulated || target.simulated != r.simulated {
                return Err(fail("Demo and physical contexts cannot be mixed"));
            }
            let key = if g == Generation::Gen2 {
                command::hex(&key, 6, "Destination key A").map_err(fail)?
            } else {
                String::new()
            };
            if g == Generation::Gen1a {
                c.inspect("hf mf csetblk", &["--blk", "-d"])?;
            } else {
                c.inspect("hf mf wrbl", &["--blk", "-a", "-k", "-d"])?;
            }
            recovery::version(c)?;
            let actual = inspect(c, g, &key, r.simulated)?;
            if actual != target {
                return Err(fail("Destination changed since inspection"));
            }
            if target.uid == source.dump.uid {
                return Err(fail("Source and destination currently have the same UID. Use a distinct blank for the guided clone"));
            }
            if g == Generation::Gen2 {
                for s in 0..16 {
                    let trailer =
                        recovery::block(c, classic::trailer(s).map_err(fail)?, &key, false)?;
                    let a = classic::access_bits(&trailer[6..9]).map_err(fail)?;
                    if a != [0, 0, 0, 1] {
                        return Err(fail("Gen 2 destination must have transport access conditions in every sector"));
                    }
                }
            }
            // Data before each trailer, and manufacturer block last. After block 0 the
            // expected UID changes exactly once; no subsequent operation uses old identity.
            for n in (1..64).chain(std::iter::once(0)) {
                recovery::identity(c, &target.uid, Geometry::Classic1K)?;
                let expected = &source.dump.bytes[n as usize * 16..n as usize * 16 + 16];
                let text = if g == Generation::Gen1a {
                    format!("hf mf csetblk --blk {n} -d {}", command::encode(expected))
                } else {
                    let active_key = if n == 0 {
                        command::encode(&source.dump.bytes[48..54])
                    } else {
                        key.clone()
                    };
                    format!(
                        "hf mf wrbl --blk {n} -a -k {active_key} -d {}",
                        command::encode(expected)
                    )
                };
                c.run(recovery::spec(text)?)?;
                if g == Generation::Gen1a && backdoor_read(c, n)? != expected {
                    return Err(Outcome::Mismatch);
                }
            }
            recovery::identity(c, &source.dump.uid, Geometry::Classic1K)?;
            for n in 0..64 {
                let s = classic::sector(n).map_err(fail)?;
                let t = classic::trailer(s).map_err(fail)? as usize * 16;
                let a = command::encode(&source.dump.bytes[t..t + 6]);
                let b = command::encode(&source.dump.bytes[t + 10..t + 16]);
                let expected = &source.dump.bytes[n as usize * 16..n as usize * 16 + 16];
                let observed = match g {
                    Generation::Gen1a => backdoor_read(c, n)?,
                    Generation::Gen2 => match recovery::block(c, n, &a, false) {
                        Ok(v) => v,
                        Err(Outcome::Cancelled) => return Err(Outcome::Cancelled),
                        Err(Outcome::TimedOut) => return Err(Outcome::TimedOut),
                        Err(_) => recovery::block(c, n, &b, true)?,
                    },
                };
                if g == Generation::Gen1a || n as usize * 16 != t {
                    if observed != expected {
                        return Err(Outcome::Mismatch);
                    }
                } else {
                    if observed[6..10] != expected[6..10] {
                        return Err(Outcome::Mismatch);
                    }
                    let acl = classic::access_bits(&observed[6..9]).map_err(fail)?;
                    if matches!(acl[3], 0..=2) {
                        if observed[10..16] != expected[10..16] {
                            return Err(Outcome::Mismatch);
                        }
                    } else {
                        recovery::block(c, n, &b, true)?;
                    }
                    recovery::block(c, n, &a, false)?;
                }
                recovery::identity(c, &source.dump.uid, Geometry::Classic1K)?;
            }
            Ok((Outcome::Verified,Evidence{uid:Some(source.dump.uid.clone()),verification:Some(if g==Generation::Gen1a{"All 64 blocks match by fresh backdoor reads, including UID and trailers. Access-reader acceptance is untested."}else{"Readable blocks, UID and access bytes match; trailer keys independently confirmed by authentication/readable-key comparison. Access-reader acceptance is untested."}.into()),..Default::default()}))
        }
    }
}
pub fn image(dump: Dump, simulated: bool) -> Image {
    Image {
        name: "Classic source".into(),
        dump,
        simulated,
    }
}
