use crate::{
    credential::{Credential, Format, Target},
    custom_format, lf,
    runner::Job,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "family", content = "credential")]
pub enum Value {
    Hid(Credential, Target),
    Lf(lf::Credential),
    Custom(custom_format::Profile),
}
impl Value {
    pub fn validate(&self) -> Result<String, String> {
        match self {
            Self::Hid(c, _) => Ok(format!("hid:{}", c.encode()?.hid_raw)),
            Self::Custom(p) => Ok(format!(
                "hid:{}",
                p.layout.encode(p.facility_code, p.card_number)?.hid_raw
            )),
            Self::Lf(c) => {
                c.write()?;
                Ok(format!(
                    "{:?}:{}:{}:{}",
                    c.family,
                    c.facility,
                    c.number,
                    if c.family == lf::Family::IoProx {
                        c.version
                    } else {
                        0
                    }
                ))
            }
        }
    }
    pub fn job(&self) -> Result<Job, String> {
        match self {
            Self::Hid(c, t) => Ok(Job::ProgrammeCredential {
                write: crate::credential::write_command(c, *t)?,
                expected: c.clone(),
            }),
            Self::Lf(c) => Ok(Job::ProgrammeLf(c.clone())),
            Self::Custom(p) => {
                let (write, expected) = p.layout.write(p.facility_code, p.card_number, p.target)?;
                Ok(Job::ProgrammeRaw { write, expected })
            }
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    Pending,
    InProgress,
    Verified,
    Failed,
    Unknown,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Row {
    pub id: String,
    pub value: Value,
    pub status: Status,
    pub result: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Batch {
    pub schema: u32,
    pub rows: Vec<Row>,
}
impl Batch {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 || self.rows.is_empty() || self.rows.len() > 10000 {
            return Err("Batch must have 1–10000 rows and schema 1".into());
        }
        let mut ids = BTreeSet::new();
        let mut values = BTreeSet::new();
        for row in &self.rows {
            if row.id.trim().is_empty()
                || row.id.len() > 80
                || row.id.chars().any(char::is_control)
                || !ids.insert(row.id.clone())
            {
                return Err("Invalid or duplicate row ID".into());
            }
            if !values.insert(row.value.validate()?) {
                return Err("Duplicate credential payload; batch rejected".into());
            }
        }
        Ok(())
    }
    pub fn load(bytes: &[u8]) -> Result<Self, String> {
        let mut b: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        b.validate()?;
        for r in &mut b.rows {
            if r.status == Status::InProgress {
                r.status = Status::Unknown;
                r.result =
                    "Previous run interrupted; inspect the card before explicitly retrying".into();
            }
        }
        Ok(b)
    }
    pub fn next(&self) -> Option<usize> {
        self.rows.iter().position(|r| r.status == Status::Pending)
    }
    pub fn range(
        format: Format,
        target: Target,
        fc: u32,
        start: u64,
        count: u32,
    ) -> Result<Self, String> {
        if count == 0 || count > 10000 {
            return Err("Range count must be 1–10000".into());
        }
        let mut rows = vec![];
        for i in 0..count {
            let c = Credential {
                format,
                facility_code: fc,
                card_number: start
                    .checked_add(u64::from(i))
                    .ok_or("Card range overflow")?,
            };
            c.validate()?;
            rows.push(Row {
                id: format!("card-{}", c.card_number),
                value: Value::Hid(c, target),
                status: Status::Pending,
                result: String::new(),
            });
        }
        let b = Self { schema: 1, rows };
        b.validate()?;
        Ok(b)
    }
    pub fn csv(input: &str) -> Result<Self, String> {
        let records = parse_csv(input)?;
        let headers = ["id", "format", "facility", "card", "version", "target"];
        if records
            .first()
            .map(|r| r.iter().map(String::as_str).collect::<Vec<_>>())
            != Some(headers.to_vec())
        {
            return Err("CSV header must be id,format,facility,card,version,target".into());
        }
        let mut rows = vec![];
        for r in records.into_iter().skip(1) {
            if r.len() != 6 {
                return Err("CSV rows need six fields".into());
            }
            let target = match r[5].as_str() {
                "T5577" => Target::T5577,
                "Q5" => Target::Q5,
                "EM4305" => Target::Em4305,
                _ => return Err("Unknown writable target".into()),
            };
            let value = if let Some(f) = Format::ALL.into_iter().find(|f| f.name() == r[1]) {
                Value::Hid(Credential::from_decimal(f, &r[2], &r[3])?, target)
            } else {
                let family = match r[1].as_str() {
                    "AWID26" => lf::Family::Awid26,
                    "AWID34" => lf::Family::Awid34,
                    "AWID37" => lf::Family::Awid37,
                    "AWID50" => lf::Family::Awid50,
                    "ioProx" => lf::Family::IoProx,
                    _ => return Err("Unknown credential format".into()),
                };
                Value::Lf(lf::Credential {
                    name: r[0].clone(),
                    family,
                    facility: r[2].parse().map_err(|_| "Invalid facility")?,
                    number: r[3].parse().map_err(|_| "Invalid card")?,
                    version: if r[4].is_empty() {
                        1
                    } else {
                        r[4].parse().map_err(|_| "Invalid version")?
                    },
                    target,
                })
            };
            rows.push(Row {
                id: r[0].clone(),
                value,
                status: Status::Pending,
                result: String::new(),
            });
        }
        let b = Self { schema: 1, rows };
        b.validate()?;
        Ok(b)
    }
    pub fn report(&self) -> String {
        let mut out = "id,status,result\r\n".to_owned();
        for r in &self.rows {
            out.push_str(&format!(
                "{},{:?},{}\r\n",
                csv_cell(&r.id),
                r.status,
                csv_cell(&r.result)
            ));
        }
        out
    }
}
pub fn csv_cell(input: &str) -> String {
    let safe = if input.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{input}")
    } else {
        input.to_owned()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}
pub fn parse_csv(input: &str) -> Result<Vec<Vec<String>>, String> {
    if input.len() > 1024 * 1024 {
        return Err("CSV exceeds 1 MiB".into());
    }
    let mut rows = vec![];
    let mut row = vec![];
    let mut cell = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut chars = input.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                cell.push(c)
            }
            continue;
        }
        match c {
            '"' => {
                if !cell.is_empty() || closed {
                    return Err("Malformed CSV quoting".into());
                }
                quoted = true;
            }
            ',' => {
                row.push(std::mem::take(&mut cell));
                closed = false;
            }
            '\n' | '\r' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut cell));
                if row.iter().any(|v| !v.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                closed = false;
            }
            _ => {
                if closed {
                    return Err("Unexpected data after quoted CSV field".into());
                }
                cell.push(c);
            }
        }
    }
    if quoted {
        return Err("Unclosed CSV quotation".into());
    }
    if !cell.is_empty() || !row.is_empty() || closed {
        row.push(cell);
        rows.push(row);
    }
    Ok(rows)
}
#[derive(Default)]
pub struct Workspace {
    pub batch: Option<Batch>,
    pub path: String,
    pub csv_input: String,
    pub range_start: u64,
    pub range_count: u32,
    pub active: Option<usize>,
    pub ack: bool,
    pub message: String,
}
impl Workspace {
    pub fn persist(&self) -> Result<(), String> {
        let b = self.batch.as_ref().ok_or("No batch")?;
        b.validate()?;
        crate::storage::write_private(
            std::path::Path::new(&self.path),
            &serde_json::to_vec_pretty(b).map_err(|e| e.to_string())?,
        )
    }
    pub fn finish(&mut self, outcome: &crate::runner::Outcome) {
        if let Some(i) = self.active.take() {
            if let Some(b) = &mut self.batch {
                let r = &mut b.rows[i];
                r.status = match outcome {
                    crate::runner::Outcome::Verified => Status::Verified,
                    crate::runner::Outcome::Failed(_) | crate::runner::Outcome::Mismatch => {
                        Status::Failed
                    }
                    _ => Status::Unknown,
                };
                r.result=match r.status{Status::Verified=>"Fresh read-back verified; physical LF identity/reader acceptance not established",Status::Failed=>"Operation failed or read-back mismatch; inspect before retry",_=>"Interrupted; card state unknown"}.into();
            }
            if let Err(e) = self.persist() {
                self.message = format!("Result is in memory but journal could not be saved: {e}");
            }
        }
    }
    pub fn ui(
        &mut self,
        ui: &mut eframe::egui::Ui,
        format: Format,
        target: Target,
        fc: u32,
    ) -> Option<Job> {
        use eframe::egui;
        ui.heading("Credential batch provisioning");
        ui.label("One card per explicit acknowledgement. Verified rows are never automatically replayed. LF read-back cannot prove physical card identity.");
        ui.horizontal(|ui| {
            ui.label("Batch journal JSON path");
            ui.text_edit_singleline(&mut self.path);
            if ui.button("Load journal").clicked() {
                match crate::storage::read_bounded(
                    std::path::Path::new(&self.path),
                    4 * 1024 * 1024,
                )
                .and_then(|b| Batch::load(&b))
                {
                    Ok(b) => {
                        self.batch = Some(b);
                        self.ack = false;
                        self.message =
                            "Journal loaded; interrupted rows require explicit retry".into()
                    }
                    Err(e) => self.message = e,
                }
            }
        });
        ui.label("CSV: id,format,facility,card,version,target");
        ui.add(egui::TextEdit::multiline(&mut self.csv_input).desired_rows(4));
        if ui.button("Import CSV").clicked() {
            match Batch::csv(&self.csv_input) {
                Ok(b) => {
                    self.batch = Some(b);
                    self.message = self
                        .persist()
                        .map(|()| "CSV imported; journal saved".into())
                        .unwrap_or_else(|e| e)
                }
                Err(e) => self.message = e,
            }
        }
        ui.horizontal(|ui| {
            ui.label("HID range start");
            ui.add(egui::DragValue::new(&mut self.range_start));
            ui.label("Count");
            ui.add(egui::DragValue::new(&mut self.range_count).range(1..=10000));
            if ui
                .button("Generate from selected format/facility/target")
                .clicked()
            {
                match Batch::range(format, target, fc, self.range_start, self.range_count) {
                    Ok(b) => {
                        self.batch = Some(b);
                        self.message = self
                            .persist()
                            .map(|()| "Range created and journal saved".into())
                            .unwrap_or_else(|e| e)
                    }
                    Err(e) => self.message = e,
                }
            }
        });
        let mut requested = None;
        if let Some(b) = &mut self.batch {
            ui.label(format!(
                "{} rows; {} verified",
                b.rows.len(),
                b.rows
                    .iter()
                    .filter(|r| r.status == Status::Verified)
                    .count()
            ));
            for (i, r) in b.rows.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{} • {:?}", r.id, r.status));
                    if matches!(r.status, Status::Failed | Status::Unknown)
                        && ui.small_button("Queue explicit retry").clicked()
                    {
                        r.status = Status::Pending;
                        r.result.clear();
                        self.ack = false;
                    }
                    if self.active == Some(i) {
                        ui.spinner();
                    }
                });
            }
            if let Some(i) = b.next() {
                ui.checkbox(
                    &mut self.ack,
                    format!(
                        "A fresh writable card is placed for {} and I authorise this row",
                        b.rows[i].id
                    ),
                );
                if ui
                    .add_enabled(self.ack, egui::Button::new("Programme next card & verify"))
                    .clicked()
                {
                    requested = Some(i);
                }
            }
        }
        if let Some(i) = requested {
            let job = self.batch.as_ref().unwrap().rows[i].value.job();
            match job {
                Ok(job) => {
                    self.batch.as_mut().unwrap().rows[i].status = Status::InProgress;
                    self.ack = false;
                    match self.persist() {
                        Ok(()) => {
                            self.active = Some(i);
                            return Some(job);
                        }
                        Err(e) => {
                            self.batch.as_mut().unwrap().rows[i].status = Status::Pending;
                            self.message = e
                        }
                    }
                }
                Err(e) => self.message = e,
            }
        }
        if ui.button("Export credential CSV beside journal").clicked() {
            if let Some(b) = &self.batch {
                let p = std::path::Path::new(&self.path).with_extension("credentials.csv");
                self.message = b
                    .export_csv()
                    .and_then(|csv| crate::storage::write_private(&p, csv.as_bytes()))
                    .map(|()| format!("Credentials exported to {}", p.display()))
                    .unwrap_or_else(|e| e);
            }
        }
        if ui.button("Export results CSV beside journal").clicked() {
            if let Some(b) = &self.batch {
                let p = std::path::Path::new(&self.path).with_extension("results.csv");
                self.message = crate::storage::write_private(&p, b.report().as_bytes())
                    .map(|()| format!("Results saved to {}", p.display()))
                    .unwrap_or_else(|e| e);
            }
        }
        ui.label(&self.message);
        None
    }
}

pub fn check_job(job: &Job, caps: &crate::capabilities::Capabilities) -> Result<(), String> {
    match job {
        Job::ProgrammeCredential { write, .. } | Job::ProgrammeRaw { write, .. } => {
            caps.validate(&write.text)?;
            caps.require("lf hid reader", &[])
        }
        Job::ProgrammeLf(value) => {
            caps.validate(&value.write()?.text)?;
            caps.validate(&value.read().text)
        }
        _ => Err("Unsupported provisioning row".into()),
    }
}
impl Batch {
    pub fn export_csv(&self) -> Result<String, String> {
        self.validate()?;
        let mut out = "id,format,facility,card,version,target\r\n".to_string();
        for r in &self.rows {
            let (format,fc,cn,version,target)=match &r.value{Value::Hid(c,t)=>(c.format.name(),c.facility_code.to_string(),c.card_number.to_string(),String::new(),*t),Value::Lf(c)=>(match c.family{lf::Family::Awid26=>"AWID26",lf::Family::Awid34=>"AWID34",lf::Family::Awid37=>"AWID37",lf::Family::Awid50=>"AWID50",lf::Family::IoProx=>"ioProx"},c.facility.to_string(),c.number.to_string(),c.version.to_string(),c.target),Value::Custom(_)=>return Err("Custom layouts are preserved in batch JSON; flat CSV cannot represent their parity/layout rules".into())};
            let target = match target {
                Target::T5577 => "T5577",
                Target::Q5 => "Q5",
                Target::Em4305 => "EM4305",
            };
            out.push_str(&format!(
                "{},{format},{fc},{cn},{version},{target}\r\n",
                csv_cell(&r.id)
            ));
        }
        Ok(out)
    }
}
