//! Guided workflows share the same guarded runner as the expert tools.
//! A source snapshot is independent of the last command and destination checks.
use crate::{
    backup::{self, Batch, Dump, Geometry, Step},
    capabilities::Capabilities,
    command::{self, CommandSpec, Kind},
    credential::{self, Credential, Target},
    model::Evidence,
    runner::{Job, JobResult, Outcome},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Basic,
    Advanced,
    NetAccess,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hint {
    #[default]
    Automatic,
    Lf,
    Hf,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Content {
    Em410x(String),
    Em410xMapped {
        id: String,
        format: crate::em410x::ReaderFormat,
    },
    Hid {
        raw: String,
        credential: Option<Credential>,
    },
    Lf(crate::lf::Credential),
    Memory(Dump),
    Identified {
        family: String,
        uid: Option<String>,
        reason: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Card {
    pub content: Content,
    pub captured_at: u64,
    pub simulated: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SavedCard {
    pub name: String,
    pub card: Card,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub at: u64,
    pub action: String,
    pub outcome: String,
    pub detail: String,
    pub simulated: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Destination {
    T55 { target: Target, block0: String },
    Memory { geometry: Geometry, uid: String },
}
#[derive(Clone, Debug)]
pub enum Action {
    Read(Hint),
    Check(Card),
    Write {
        card: Card,
        destination: Destination,
        acknowledged: bool,
    },
}
pub struct Request {
    pub action: Action,
    pub caps: Capabilities,
    pub simulated: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Route {
    #[default]
    Home,
    Read,
    Clone,
    Write,
    Library,
    History,
    Setup,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StepView {
    #[default]
    Source,
    Target,
    Review,
    Result,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    Read,
    Check,
    Write,
}
#[derive(Default)]
pub struct Workflow {
    pub source: Option<Card>,
    pub destination: Option<Destination>,
    pub step: StepView,
    pub acknowledged: bool,
    pub pending: Option<Pending>,
    pub result: Option<Outcome>,
}
impl Workflow {
    pub fn set_source(&mut self, card: Card) {
        self.source = Some(card);
        self.invalidate();
        self.step = StepView::Source;
    }
    pub fn invalidate(&mut self) {
        self.destination = None;
        self.acknowledged = false;
        self.result = None;
    }
    pub fn receive(&mut self, outcome: &Outcome, evidence: &Evidence) {
        match self.pending.take() {
            Some(Pending::Read) if *outcome == Outcome::Completed => {
                if let Some(card) = &evidence.card {
                    self.set_source(card.clone());
                }
            }
            Some(Pending::Check) => {
                self.destination = evidence.destination.clone();
                self.acknowledged = false;
                self.step = if self.destination.is_some() {
                    StepView::Review
                } else {
                    StepView::Target
                };
            }
            Some(Pending::Write) => {
                self.result = Some(outcome.clone());
                self.destination = None;
                self.acknowledged = false;
                self.step = StepView::Result;
            }
            _ => {}
        }
    }
}
pub trait Client {
    fn run(&mut self, spec: CommandSpec) -> (Outcome, Zeroizing<String>);
    fn execute(&mut self, job: Job) -> JobResult;
    fn stage(&mut self, message: &str);
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
impl Card {
    pub fn em_identity(&self) -> Option<(&str, Option<crate::em410x::ReaderFormat>)> {
        match &self.content {
            Content::Em410x(id) => Some((id, None)),
            Content::Em410xMapped { id, format } => Some((id, Some(*format))),
            _ => None,
        }
    }
    pub fn select_em_format(
        &mut self,
        format: Option<crate::em410x::ReaderFormat>,
    ) -> Result<(), String> {
        let (id, _) = self
            .em_identity()
            .ok_or("This is not an EM410x credential")?;
        let id = crate::command::hex(id, 5, "EM410x ID")?;
        self.content = if let Some(format) = format {
            Content::Em410xMapped { id, format }
        } else {
            Content::Em410x(id)
        };
        Ok(())
    }
    pub fn label(&self) -> String {
        match &self.content {
            Content::Em410x(_) | Content::Em410xMapped { .. } => "EM410x · 125 kHz".into(),
            Content::Hid { credential, .. } => format!(
                "HID Prox · {}",
                credential
                    .as_ref()
                    .map(|c| c.format.name())
                    .unwrap_or("choose format")
            ),
            Content::Lf(c) => format!(
                "{} · 125 kHz",
                if c.family == crate::lf::Family::IoProx {
                    "ioProx"
                } else {
                    "AWID"
                }
            ),
            Content::Memory(d) => d.geometry.model().to_uppercase(),
            Content::Identified { family, .. } => family.clone(),
        }
    }
    pub fn summary(&self) -> String {
        match &self.content {
            Content::Em410x(id) => crate::em410x::summary(id, None),
            Content::Em410xMapped { id, format } => crate::em410x::summary(id, Some(*format)),
            Content::Hid { raw, credential } => credential
                .as_ref()
                .map(|c| {
                    format!(
                        "{} · facility {} · card {}",
                        c.format.name(),
                        c.facility_code,
                        c.card_number
                    )
                })
                .unwrap_or_else(|| format!("Raw {raw} · format not selected")),
            Content::Lf(c) => format!(
                "{} bits · facility {} · card {} · version {}",
                c.bits(),
                c.facility,
                c.number,
                c.version
            ),
            Content::Memory(d) => format!("UID {} · {} bytes captured", d.uid, d.bytes.len()),
            Content::Identified { uid, reason, .. } => format!(
                "{}\n{reason}",
                uid.as_ref().map(|v| format!("UID {v}")).unwrap_or_default()
            ),
        }
    }
    pub fn scope(&self) -> String {
        match &self.content {
        Content::Memory(d)=>format!("{} user-data pages, each followed by a fresh read. UID, manufacturer, locks and configuration are excluded.",(0..d.geometry.units()).filter(|u|d.geometry.writable(*u)).count()),
        Content::Identified{reason,..}=>reason.clone(),
        Content::Em410x(_)|Content::Em410xMapped{..}=>"Full 40-bit EM410x ID matches a separate read. Reader-format selection only changes the displayed numbers; it never converts the fob into HID or proves access-reader acceptance.".into(),
        Content::Hid{..}=>"Format, facility code, card number, parity and raw encoding match a separate HID read. Access-reader acceptance and physical LF identity are not established.".into(),
        _=>"Credential encoding, checked by a separate reader operation. Reader acceptance and physical LF identity are not established.".into(),
    }
    }
    pub fn validate(&self) -> Result<(), String> {
        match &self.content {
            Content::Em410x(id) | Content::Em410xMapped { id, .. } => {
                command::hex(id, 5, "EM410x ID").map(|_| ())
            }
            Content::Hid { raw, credential } => {
                let raw = credential::normalise_raw(raw)?;
                if let Some(c) = credential {
                    if c.encode()?.hid_raw != raw {
                        return Err(
                            "Selected format/fields do not match the captured raw credential"
                                .into(),
                        );
                    }
                }
                Ok(())
            }
            Content::Lf(c) => c.write().map(|_| ()),
            Content::Memory(d) => d.validate(),
            Content::Identified {
                family,
                uid,
                reason,
            } => {
                if family.len() > 120 || reason.len() > 1000 {
                    return Err("Invalid card summary".into());
                }
                if let Some(uid) = uid {
                    backup::validate_uid(uid)?;
                }
                Ok(())
            }
        }
    }
    pub fn writable(&self) -> Result<(), String> {
        self.validate()?;
        match &self.content {
        Content::Identified{reason,..}=>Err(reason.clone()),
        Content::Hid{credential:None,..}=>Err("Select an unambiguous intended format before copying. Other formats remain available in Advanced.".into()),
        Content::Memory(d) if d.geometry==Geometry::Ultralight=>Err("Original Ultralight requires explicit model selection in Advanced → Card memory.".into()),
        Content::Memory(d) if d.geometry.classic()=>Err("Classic copying needs the known-key map in Advanced → Card memory.".into()),
        _=>Ok(()),
    }
    }
    pub fn select_format(&mut self, format: credential::Format) -> Result<(), String> {
        let Content::Hid { raw, credential } = &mut self.content else {
            return Err("This is not a HID credential".into());
        };
        let d = credential::decode_hid(format, raw)?;
        if !d.parity_valid {
            return Err("Parity does not match this format".into());
        }
        *credential = Some(d.credential);
        Ok(())
    }
}
pub fn validate_library(cards: &[SavedCard]) -> Result<(), String> {
    if cards.len() > 200 {
        return Err("Library is limited to 200 items".into());
    }
    for item in cards {
        if item.name.trim().is_empty() || item.name.len() > 100 {
            return Err("Enter a library name of 1–100 characters".into());
        }
        item.card.validate()?;
    }
    Ok(())
}
fn read(c: &mut impl Client, spec: CommandSpec) -> Result<Zeroizing<String>, Outcome> {
    let (out, raw) = c.run(spec);
    if out == Outcome::Completed {
        Ok(raw)
    } else {
        Err(out)
    }
}
fn simple(c: &mut impl Client, s: &str, kind: Kind) -> Result<Zeroizing<String>, Outcome> {
    read(c, CommandSpec::simple(s, kind))
}
fn clean(raw: &str) -> String {
    raw.lines()
        .map(crate::parse::clean)
        .collect::<Vec<_>>()
        .join("\n")
}
fn fail(s: impl Into<String>) -> Outcome {
    Outcome::Failed(s.into())
}
pub fn parse_t55(raw: &str) -> Result<Destination, String> {
    let text = clean(raw);
    if crate::parse::evidence(&text, &Kind::Detect).reported_error
        || text.to_lowercase().contains("ambiguous")
    {
        return Err("Destination is ambiguous or unreadable. No write is enabled.".into());
    }
    let chip = regex::Regex::new(r"(?m)^\[=\]\s+Chip type\.+\s+(T55x7|Q5/T5555)\s*$").unwrap();
    let block = regex::Regex::new(r"(?m)^\[=\]\s+Block0\.+\s+([0-9A-Fa-f]{8})(?:\s|$)").unwrap();
    let pwd = regex::Regex::new(r"(?m)^\[=\]\s+Password set\.+\s+No\s*$").unwrap();
    let chips = chip.captures_iter(&text).collect::<Vec<_>>();
    let blocks = block.captures_iter(&text).collect::<Vec<_>>();
    if chips.len() != 1 || blocks.len() != 1 || !pwd.is_match(&text) {
        return Err("A single password-free T5577/Q5 destination could not be established. Use Advanced for other targets.".into());
    }
    Ok(Destination::T55 {
        target: if &chips[0][1] == "T55x7" {
            Target::T5577
        } else {
            Target::Q5
        },
        block0: blocks[0][1].to_uppercase(),
    })
}
pub fn geometry(raw: &str) -> Option<Geometry> {
    let s = clean(raw).to_lowercase().replace(['-', '_'], " ");
    let mut found = vec![];
    for g in Geometry::ALL {
        let pattern = format!(r"\b{}\b", regex::escape(g.model()).replace(' ', r"\s*"));
        if regex::Regex::new(&pattern).unwrap().is_match(&s) {
            found.push(g)
        }
    }
    // Ultralight is also a generic family label for NTAGs; never infer its geometry from that alone.
    found.retain(|g| *g != Geometry::Ultralight);
    if found.len() == 1 {
        return Some(found[0]);
    }
    None
}
pub fn parse_lf(raw: &str) -> Result<Option<Content>, String> {
    let s = clean(raw);
    let lower = s.to_lowercase();
    let em = regex::Regex::new(r"(?i)EM 410x ID\s*[:=]?\s*([0-9a-f]{10})(?:\s|$)").unwrap();
    let ids = em
        .captures_iter(&s)
        .map(|c| c[1].to_uppercase())
        .collect::<Vec<_>>();
    if ids.len() > 1 {
        return Err("Multiple source observations; keep one card on the antenna".into());
    }
    if let Some(id) = ids.first() {
        return Ok(Some(Content::Em410x(id.clone())));
    }
    if lower.contains("hid prox")
        || lower.contains("[h103")
        || lower.contains("raw:") && lower.contains("hid")
    {
        let e = crate::parse::evidence(&s, &Kind::CredentialRead);
        if e.hid_raw.len() != 1 {
            return Err(
                "HID detected, but no single raw credential was decoded. Try Advanced.".into(),
            );
        }
        let raw = e.hid_raw[0].clone();
        let candidates = credential::candidates(&raw, false);
        let selected = if candidates.len() == 1 {
            Some(candidates[0].credential.clone())
        } else {
            None
        };
        return Ok(Some(Content::Hid {
            raw,
            credential: selected,
        }));
    }
    let awid = regex::Regex::new(r"AWID - len:\s*(\d+)\s+FC:\s*(\d+)\s+Card:\s*(\d+)\s+- Wiegand:")
        .unwrap();
    let io = regex::Regex::new(
        r"(?i)IO Prox - XSF\((\d+)\)([0-9a-f]{2}):(\d+), Raw: [0-9a-f]+\s+\(\s*ok\s*\)",
    )
    .unwrap();
    let matches = awid.captures_iter(&s).collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err("Multiple AWID observations".into());
    }
    if let Some(m) = matches.first() {
        use crate::lf::Family::*;
        let family = match &m[1] {
            "26" => Awid26,
            "34" => Awid34,
            "37" => Awid37,
            "50" => Awid50,
            _ => return Err("AWID format is not supported in Basic".into()),
        };
        let v = crate::lf::Credential {
            name: String::new(),
            family,
            facility: m[2].parse().map_err(|_| "Invalid facility")?,
            number: m[3].parse().map_err(|_| "Invalid card number")?,
            version: 0,
            target: Target::T5577,
        };
        v.write()?;
        return Ok(Some(Content::Lf(v)));
    }
    let matches = io.captures_iter(&s).collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err("Multiple ioProx observations".into());
    }
    if let Some(m) = matches.first() {
        let v = crate::lf::Credential {
            name: String::new(),
            family: crate::lf::Family::IoProx,
            version: m[1].parse().map_err(|_| "Invalid version")?,
            facility: u32::from_str_radix(&m[2], 16).map_err(|_| "Invalid facility")?,
            number: m[3].parse().map_err(|_| "Invalid number")?,
            target: Target::T5577,
        };
        v.write()?;
        return Ok(Some(Content::Lf(v)));
    }
    Ok(None)
}
fn no_lf_tag(raw: &str) -> bool {
    clean(raw).lines().any(|s| {
        s.trim() == "[-] No known 125/134 kHz tags found!"
            || s.trim() == "[!!] No known 125/134 kHz tags found!"
    })
}
fn capture(c: &mut impl Client, hint: Hint, simulated: bool) -> Result<Card, Outcome> {
    if hint != Hint::Hf {
        c.stage("Reading source · looking for a 125 kHz card");
        let (out, raw) = c.run(CommandSpec::simple("lf search", Kind::Detect));
        if out == Outcome::Completed {
            let lower = clean(&raw).to_lowercase();
            let details = if lower.contains("hid prox") {
                simple(c, "lf hid reader", Kind::CredentialRead)?
            } else if lower.contains("awid") {
                simple(c, "lf awid reader", Kind::Detect)?
            } else if lower.contains("io prox") {
                simple(c, "lf io reader", Kind::Detect)?
            } else {
                raw
            };
            if let Some(content) = parse_lf(&details).map_err(fail)? {
                return Ok(Card {
                    content,
                    captured_at: now(),
                    simulated,
                });
            }
            if lower.contains("valid") && lower.contains("found") {
                return Ok(Card {
                    content: Content::Identified {
                        family: "LF card".into(),
                        uid: None,
                        reason: "This encoding has no Basic copy adapter. Inspect it in Advanced."
                            .into(),
                    },
                    captured_at: now(),
                    simulated,
                });
            }
        } else if !no_lf_tag(&raw) {
            return Err(out);
        }
        if hint == Hint::Lf {
            return Err(fail(
                "No supported LF card read. Reposition one card and try again.",
            ));
        }
    }
    c.stage("Reading source · looking for a 13.56 MHz card");
    let raw = simple(c, "hf search", Kind::Detect)?;
    let info = simple(c, "hf 14a info", Kind::Detect)?;
    let uid = backup::uid(&info).ok_or_else(|| {
        fail("No single stable HF UID was read. Try Advanced for other HF protocols.")
    })?;
    let combined = format!("{}\n{}", raw.as_str(), info.as_str());
    let lower = combined.to_lowercase();
    if lower.contains("desfire") {
        return Ok(Card{content:Content::Identified{family:"MIFARE DESFire".into(),uid:Some(uid),reason:"Use Advanced → DESFire with known keys for supported D40/EV1 data and archives. Full-card cloning and EV2/LRP/EV3-specific operations are unavailable.".into()},captured_at:now(),simulated});
    }
    if lower.contains("ntag") || lower.contains("ultralight") {
        let model = simple(c, "hf mfu info", Kind::Detect)?;
        let g = geometry(&model).filter(|g| !g.classic()).ok_or_else(|| {
            fail("The exact memory model is not established. Open Advanced to inspect it.")
        })?;
        if backup::uid(&model).as_deref() != Some(uid.as_str()) {
            return Err(fail("Source changed during identification"));
        }
        c.stage("Reading source · capturing complete readable memory");
        let steps = (0..g.units())
            .map(|u| {
                Ok(Step {
                    read: g.read(u, "", false)?,
                    write: None,
                    expected: None,
                })
            })
            .collect::<Result<Vec<_>, String>>()
            .map_err(fail)?;
        let r = c.execute(Job::Batch(Batch {
            geometry: g,
            uid: uid.clone(),
            steps,
            writing: false,
        }));
        if r.outcome != Outcome::Completed {
            return Err(r.outcome);
        }
        let dump = Dump::new(g, &uid, r.evidence.bytes).map_err(fail)?;
        return Ok(Card {
            content: Content::Memory(dump),
            captured_at: now(),
            simulated,
        });
    }
    Ok(Card{content:Content::Identified{family:if lower.contains("classic"){"MIFARE Classic"}else{"HF card"}.into(),uid:Some(uid),reason:"Identification only. Open Advanced for known-key memory access or protocol-specific operations.".into()},captured_at:now(),simulated})
}
fn checked_destination(c: &mut impl Client, card: &Card) -> Result<Destination, Outcome> {
    card.writable().map_err(fail)?;
    c.stage("Checking destination · read-only inspection");
    match &card.content {
        Content::Memory(d) => {
            let raw = simple(c, "hf mfu info", Kind::Detect)?;
            let g = geometry(&raw).ok_or_else(|| fail("Destination memory model is unknown"))?;
            if g != d.geometry {
                return Err(fail("Destination model differs from the saved source"));
            }
            let uid = backup::uid(&raw).ok_or_else(|| fail("Destination UID is unreadable"))?;
            let raw = simple(c, "hf 14a info", Kind::Detect)?;
            if backup::uid(&raw).as_deref() != Some(uid.as_str()) {
                return Err(fail("Destination UID is changing"));
            }
            let page = read(c, g.read(2, "", false).map_err(fail)?)?;
            let page = crate::parse::evidence(&page, &Kind::Read).bytes;
            let dynamic = read(c, g.read(g.data_end() + 1, "", false).map_err(fail)?)?;
            let dynamic = crate::parse::evidence(&dynamic, &Kind::Read).bytes;
            backup::check_locks(g, &page, Some(&dynamic)).map_err(fail)?;
            Ok(Destination::Memory { geometry: g, uid })
        }
        _ => {
            let raw = simple(c, "lf t55xx detect", Kind::Detect)?;
            let dest = parse_t55(&raw).map_err(fail)?;
            if matches!(
                (&card.content, &dest),
                (
                    Content::Em410x(_) | Content::Em410xMapped { .. },
                    Destination::T55 {
                        target: Target::Q5,
                        ..
                    }
                )
            ) {
                return Err(fail("Basic EM410x writes require a T5577 destination"));
            }
            Ok(dest)
        }
    }
}
pub fn write_job(card: &Card, destination: &Destination) -> Result<Job, String> {
    card.writable()?;
    match (&card.content, destination) {
        (
            Content::Em410x(id) | Content::Em410xMapped { id, .. },
            Destination::T55 {
                target: Target::T5577,
                ..
            },
        ) => Ok(Job::Programme {
            write: command::em_write(id)?,
            read: CommandSpec::simple("lf em 410x reader", Kind::Read),
            expected: command::payload(id)?,
        }),
        (
            Content::Hid {
                credential: Some(c),
                ..
            },
            Destination::T55 { target, .. },
        ) => Ok(Job::ProgrammeCredential {
            write: credential::write_command(c, *target)?,
            expected: c.clone(),
        }),
        (Content::Lf(c), Destination::T55 { target, .. }) => {
            let mut c = c.clone();
            c.target = *target;
            c.write()?;
            Ok(Job::ProgrammeLf(c))
        }
        (Content::Memory(d), Destination::Memory { geometry, uid }) if d.geometry == *geometry => {
            backup::validate_uid(uid)?;
            let steps = (0..geometry.units())
                .filter(|u| geometry.writable(*u))
                .map(|u| {
                    let start = u as usize * geometry.width();
                    let bytes = d.bytes[start..start + geometry.width()].to_vec();
                    Ok(Step {
                        read: geometry.read(u, "", false)?,
                        write: Some(geometry.write(u, &bytes, "", false)?),
                        expected: Some(bytes),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(Job::Batch(Batch {
                geometry: *geometry,
                uid: uid.clone(),
                steps,
                writing: true,
            }))
        }
        _ => Err("Source and destination are not compatible with this Basic workflow".into()),
    }
}
pub fn execute(c: &mut impl Client, action: Action, simulated: bool) -> JobResult {
    let r = (|| -> Result<Evidence, Outcome> {
        match action {
            Action::Read(hint) => Ok(Evidence {
                card: Some(capture(c, hint, simulated)?),
                ..Default::default()
            }),
            Action::Check(card) => Ok(Evidence {
                destination: Some(checked_destination(c, &card)?),
                ..Default::default()
            }),
            Action::Write {
                card,
                destination,
                acknowledged,
            } => {
                if !acknowledged {
                    return Err(fail("Confirm the destination before writing"));
                }
                if card.simulated != simulated {
                    return Err(fail("Demo and physical captures cannot be mixed. Read the source in this session."));
                }
                // Require known compatibility in Basic, then check the target again immediately before writing.
                c.stage("Checking matching client and firmware");
                let version = simple(c, "hw version", Kind::Device)?;
                if !matches!(
                    crate::maintenance::compatibility(&version),
                    crate::maintenance::Compatibility::Matched(_)
                ) {
                    return Err(fail(
                        "Matching client and firmware could not be confirmed. Check Device setup.",
                    ));
                }
                let fresh = checked_destination(c, &card)?;
                if fresh != destination {
                    return Err(fail(
                        "Destination changed since review. Check it again before writing.",
                    ));
                }
                c.stage("Writing · a fresh read-back follows each write");
                let job = write_job(&card, &destination).map_err(fail)?;
                let r = c.execute(job);
                if r.outcome != Outcome::Verified {
                    return Err(r.outcome);
                }
                Ok(Evidence {
                    verification: Some(card.scope()),
                    ..r.evidence
                })
            }
        }
    })();
    match r {
        Ok(evidence) => JobResult {
            outcome: if evidence.verification.is_some() {
                Outcome::Verified
            } else {
                Outcome::Completed
            },
            evidence,
            capabilities: None,
        },
        Err(outcome) => JobResult {
            outcome,
            evidence: Evidence::default(),
            capabilities: None,
        },
    }
}
