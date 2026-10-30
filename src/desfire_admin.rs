use crate::{
    command::{self, CommandSpec, DesfireParams, Kind},
    model::Algorithm,
};
use eframe::egui;
use zeroize::Zeroizing;
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Action {
    #[default]
    CreateApp,
    DeleteApp,
    CreateData,
    CreateBackup,
    CreateValue,
    CreateLinear,
    CreateCyclic,
    DeleteFile,
    ChangeRights,
    ChangeKey,
    KeySettings,
    ChangeKeySettings,
    ReadData,
    WriteData,
    ReadRecord,
    WriteRecord,
    ReadValue,
    Credit,
    Debit,
    ClearRecords,
    LimitedCredit,
    ClearValue,
}
impl Action {
    pub const ALL: [Self; 22] = [
        Self::CreateApp,
        Self::DeleteApp,
        Self::CreateData,
        Self::CreateBackup,
        Self::CreateValue,
        Self::CreateLinear,
        Self::CreateCyclic,
        Self::DeleteFile,
        Self::ChangeRights,
        Self::ChangeKey,
        Self::KeySettings,
        Self::ChangeKeySettings,
        Self::ReadData,
        Self::WriteData,
        Self::ReadRecord,
        Self::WriteRecord,
        Self::ReadValue,
        Self::Credit,
        Self::Debit,
        Self::ClearRecords,
        Self::LimitedCredit,
        Self::ClearValue,
    ];
    pub fn write(self) -> bool {
        !matches!(
            self,
            Self::KeySettings | Self::ReadRecord | Self::ReadValue | Self::ReadData
        )
    }
}
#[derive(Clone)]
pub struct Admin {
    pub target_uid: String,
    pub action: Action,
    pub size: u32,
    pub offset: u32,
    pub record_start: u32,
    pub record_count: u32,
    pub update_record: Option<u32>,
    pub limited_credit: u8,
    pub file_mode: crate::model::Communication,
    pub records: u32,
    pub rights: String,
    pub ks1: String,
    pub numkeys: u8,
    pub newkey: Zeroizing<String>,
    pub oldkey: Zeroizing<String>,
    pub newkeyno: u8,
    pub version: String,
    pub algorithm: Algorithm,
    pub lower: i32,
    pub upper: i32,
    pub value: i32,
    pub kdf: String,
    pub kdf_input: String,
    pub selection_iso: String,
    pub iso_id: String,
    pub df_name: String,
    pub data: String,
    pub alias: String,
    pub message: String,
}
impl Default for Admin {
    fn default() -> Self {
        Self {
            target_uid: String::new(),
            action: Action::default(),
            size: 32,
            offset: 0,
            record_start: 0,
            record_count: 0,
            update_record: None,
            limited_credit: 0,
            file_mode: crate::model::Communication::Plain,
            records: 8,
            rights: "EEEE".into(),
            ks1: "0F".into(),
            numkeys: 1,
            newkey: Zeroizing::new(String::new()),
            oldkey: Zeroizing::new(String::new()),
            newkeyno: 0,
            version: "00".into(),
            algorithm: Algorithm::Aes,
            lower: 0,
            upper: 1000000,
            value: 0,
            kdf: "none".into(),
            kdf_input: String::new(),
            selection_iso: String::new(),
            iso_id: String::new(),
            df_name: String::new(),
            data: String::new(),
            alias: String::new(),
            message: String::new(),
        }
    }
}
impl Admin {
    pub fn build(&self, p: &DesfireParams<'_>) -> Result<CommandSpec, String> {
        let auth = p.build(
            if p.no_auth {
                Kind::Files
            } else {
                Kind::Authenticate
            },
            0,
            0,
            None,
        )?;
        let prefix = if p.no_auth {
            "hf mfdes getfileids"
        } else {
            "hf mfdes auth"
        };
        let args = auth.text.strip_prefix(prefix).unwrap();
        let verb = match self.action {
            Action::CreateApp => "createapp",
            Action::DeleteApp => "deleteapp",
            Action::CreateData | Action::CreateBackup => "createfile",
            Action::CreateValue => "createvaluefile",
            Action::CreateLinear | Action::CreateCyclic => "createrecordfile",
            Action::DeleteFile => "deletefile",
            Action::ChangeRights => "chfilesettings",
            Action::ChangeKey => "changekey",
            Action::KeySettings => "getkeysettings",
            Action::ChangeKeySettings => "chkeysettings",
            Action::ReadRecord | Action::ReadData => "read",
            Action::WriteRecord | Action::WriteData => "write",
            Action::ReadValue
            | Action::Credit
            | Action::Debit
            | Action::LimitedCredit
            | Action::ClearValue => "value",
            Action::ClearRecords => "clearrecfile",
        };
        let mut cmd = Zeroizing::new(format!("hf mfdes {verb}{args}"));
        // auth currently omits AID only for getAIDs; every admin action selects an explicit application.
        if !cmd.contains("--aid") {
            cmd.push_str(&format!(" --aid {}", command::hex(p.aid, 3, "AID")?));
        }
        if ![
            Action::CreateApp,
            Action::DeleteApp,
            Action::ChangeKey,
            Action::KeySettings,
            Action::ChangeKeySettings,
        ]
        .contains(&self.action)
        {
            cmd.push_str(&format!(" --fid {}", command::hex(p.fid, 1, "File ID")?));
        }
        if !["none", "AN10922", "gallagher"].contains(&self.kdf.as_str()) {
            return Err("Unsupported KDF".into());
        }
        cmd.push_str(&format!(" --kdf {}", self.kdf));
        if self.kdf != "none" {
            let raw = self.kdf_input.split_whitespace().collect::<String>();
            if raw.is_empty()
                || raw.len() > 62
                || !raw.len().is_multiple_of(2)
                || !raw.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("KDF input must be 1–31 hex bytes".into());
            }
            cmd.push_str(&format!(" -i {raw}"));
        }
        if !self.selection_iso.is_empty() {
            if matches!(self.action, Action::CreateApp | Action::DeleteApp) {
                return Err("This action requires native AID selection".into());
            }
            cmd.push_str(&format!(
                " --isoid {}",
                command::hex(&self.selection_iso, 2, "Application ISO ID")?
            ));
        }
        if self.offset > 0xffffff
            || self.record_start > 0xffffff
            || self.record_count > 0xffffff
            || self.update_record.is_some_and(|n| n > 0xffffff)
            || self.limited_credit > 1
        {
            return Err("Offset, record index/count or limited-credit flags out of range".into());
        }
        if self.size == 0 || self.size > 0xffffff || self.records == 0 || self.records > 0xffffff {
            return Err("Size/count must fit in 1–FFFFFF".into());
        }
        match self.action {
            Action::CreateApp => {
                if p.aid == "000000" || !(1..=14).contains(&self.numkeys) {
                    return Err("Application needs a non-zero AID and 1–14 keys".into());
                }
                cmd.push_str(&format!(
                    " --ks1 {} --dstalgo {} --numkeys {}",
                    command::hex(&self.ks1, 1, "Key settings")?,
                    self.algorithm.cli(),
                    self.numkeys
                ));
                if !self.iso_id.is_empty() {
                    let iso = command::hex(&self.iso_id, 2, "ISO ID")?;
                    if ["0000", "3F00", "3FFF", "FFFF"].contains(&iso.as_str()) {
                        return Err("Reserved ISO application ID".into());
                    }
                    cmd.push_str(&format!(" --fid {iso}"));
                }
                if !self.df_name.is_empty() {
                    let n = self.df_name.split_whitespace().collect::<String>();
                    if n.len() > 32
                        || !n.len().is_multiple_of(2)
                        || !n.bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        return Err("DF name must be 1–16 hex bytes".into());
                    }
                    cmd.push_str(&format!(" --dfhex {n}"));
                }
            }
            Action::CreateData
            | Action::CreateBackup
            | Action::CreateLinear
            | Action::CreateCyclic => {
                cmd.push_str(&format!(
                    " --size {:06X} --rawrights {} --amode {}",
                    self.size,
                    command::hex(&self.rights, 2, "Access rights")?,
                    self.file_mode.cli()
                ));
                if self.action == Action::CreateBackup {
                    cmd.push_str(" --backup");
                }
                if matches!(self.action, Action::CreateLinear | Action::CreateCyclic) {
                    cmd.push_str(&format!(" --maxrecord {:06X}", self.records));
                }
                if self.action == Action::CreateCyclic {
                    cmd.push_str(" --cyclic");
                }
            }
            Action::CreateValue => {
                if self.lower > self.value || self.value > self.upper {
                    return Err("Value must lie within lower and upper limits".into());
                }
                cmd.push_str(&format!(
                    " --rawrights {} --lower {:08X} --upper {:08X} --value {:08X} --lcredit {} --amode {}",
                    command::hex(&self.rights, 2, "Rights")?,
                    self.lower as u32,
                    self.upper as u32,
                    self.value as u32, self.limited_credit,self.file_mode.cli()
                ));
            }
            Action::ChangeRights => cmd.push_str(&format!(
                " --rawrights {} --amode {}",
                command::hex(&self.rights, 2, "Rights")?,
                self.file_mode.cli()
            )),
            Action::ChangeKey => {
                if p.no_auth || self.newkeyno > 13 {
                    return Err("Key changes require authentication and a key number 0–13".into());
                }
                let new = Zeroizing::new(command::hex(
                    &self.newkey,
                    self.algorithm.key_bytes(),
                    "New key",
                )?);
                let old = Zeroizing::new(command::hex(
                    &self.oldkey,
                    p.algorithm.key_bytes(),
                    "Old key",
                )?);
                cmd.push_str(&format!(
                    " --newkeyno {} --oldalgo {} --oldkey {} --newalgo {} --newkey {} --newver {}",
                    self.newkeyno,
                    p.algorithm.cli(),
                    old.as_str(),
                    self.algorithm.cli(),
                    new.as_str(),
                    command::hex(&self.version, 1, "Key version")?
                ));
            }
            Action::ChangeKeySettings => cmd.push_str(&format!(
                " -d {}",
                command::hex(&self.ks1, 1, "Key settings")?
            )),
            Action::ReadData => cmd.push_str(&format!(
                " --type data -o {:06X} -l {:06X}",
                self.offset, self.size
            )),
            Action::WriteData => {
                let bytes = command::payload(&self.data)?;
                cmd.push_str(&format!(
                    " --type data -o {:06X} -d {}",
                    self.offset,
                    command::encode(&bytes)
                ));
                if p.backup_file {
                    cmd.push_str(" --commit");
                }
            }
            Action::ReadRecord => cmd.push_str(&format!(
                " --type record -o {:06X} -l {:06X}",
                self.record_start, self.record_count
            )),
            Action::WriteRecord => {
                let bytes = command::payload(&self.data)?;
                cmd.push_str(&format!(
                    " --type record -o 000000 -d {} --commit",
                    command::encode(&bytes)
                ));
                if let Some(n) = self.update_record {
                    cmd.push_str(&format!(" --updaterec {n}"));
                }
            }
            Action::ReadValue
            | Action::Credit
            | Action::Debit
            | Action::LimitedCredit
            | Action::ClearValue => {
                cmd.push_str(&format!(
                    " -o {}",
                    match self.action {
                        Action::Credit => "credit",
                        Action::Debit => "debit",
                        Action::LimitedCredit => "limcredit",
                        Action::ClearValue => "clear",
                        _ => "get",
                    }
                ));
                if !matches!(self.action, Action::ReadValue | Action::ClearValue) {
                    if self.value < 0 {
                        return Err("Credit/debit amount must be non-negative".into());
                    }
                    cmd.push_str(&format!(" -d {:08X}", self.value as u32));
                }
            }
            _ => {}
        }
        CommandSpec::new(
            cmd.to_string(),
            format!("DESFire {:?}", self.action),
            true,
            if self.action.write() {
                Kind::Write
            } else {
                Kind::Read
            },
        )
    }
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        file: Option<&crate::desfire_archive::File>,
        key_number: u8,
        no_auth: bool,
    ) -> bool {
        ui.heading("Application and file management");
        if let Some(f) = file {
            ui.label(format!("Observed file type {}, rights {:04X}, mode {}. Incompatible or unauthorised actions are disabled; writes still check fresh state.",f.kind,f.rights,f.mode.cli()));
        } else {
            ui.label("Inspect the selected file settings to filter actions by observed type and permissions.");
        }
        egui::ComboBox::from_label("Action")
            .selected_text(format!("{:?}", self.action))
            .show_ui(ui, |ui| {
                for a in Action::ALL {
                    ui.add_enabled_ui(a.available(file, key_number, no_auth), |ui| {
                        ui.selectable_value(&mut self.action, a, format!("{a:?}"));
                    });
                }
            });
        ui.horizontal_wrapped(|ui| {
            ui.label("Data offset");
            ui.add(egui::DragValue::new(&mut self.offset).range(0..=0xffffff));
            ui.label("Record start/count (0=all)");
            ui.add(egui::DragValue::new(&mut self.record_start).range(0..=0xffffff));
            ui.add(egui::DragValue::new(&mut self.record_count).range(0..=0xffffff));
        });
        ui.horizontal(|ui| {
            let mut update = self.update_record.is_some();
            ui.checkbox(&mut update, "Update existing record");
            if update {
                let n = self.update_record.get_or_insert(0);
                ui.add(egui::DragValue::new(n).range(0..=0xffffff));
            } else {
                self.update_record = None;
            }
            ui.label("Limited credit enabled (0/1; FreeValue unsupported)");
            ui.add(egui::DragValue::new(&mut self.limited_credit).range(0..=1));
            egui::ComboBox::from_label("File access mode")
                .selected_text(self.file_mode.cli())
                .show_ui(ui, |ui| {
                    for mode in [
                        crate::model::Communication::Plain,
                        crate::model::Communication::Mac,
                        crate::model::Communication::Encrypt,
                    ] {
                        ui.selectable_value(&mut self.file_mode, mode, mode.cli());
                    }
                });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("File / record size");
            ui.add(egui::DragValue::new(&mut self.size).range(1..=0xffffff));
            ui.label("Max records");
            ui.add(egui::DragValue::new(&mut self.records).range(1..=0xffffff));
            ui.label("Access rights R/W/RW/change");
            ui.text_edit_singleline(&mut self.rights);
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Lower / upper / value or amount");
            for v in [&mut self.lower, &mut self.upper, &mut self.value] {
                ui.add(egui::DragValue::new(v));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("New application key settings");
            ui.text_edit_singleline(&mut self.ks1);
            ui.label("Key count");
            ui.add(egui::DragValue::new(&mut self.numkeys).range(1..=14));
            egui::ComboBox::from_label("New key algorithm")
                .selected_text(self.algorithm.cli())
                .show_ui(ui, |ui| {
                    for a in Algorithm::ALL {
                        ui.selectable_value(&mut self.algorithm, a, a.cli());
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Select existing app by ISO ID (optional)");
            ui.text_edit_singleline(&mut self.selection_iso);
        });
        ui.horizontal(|ui| {
            ui.label("Optional app ISO ID");
            ui.text_edit_singleline(&mut self.iso_id);
            ui.label("DF name (hex)");
            ui.text_edit_singleline(&mut self.df_name);
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("KDF")
                .selected_text(&self.kdf)
                .show_ui(ui, |ui| {
                    for k in ["none", "AN10922", "gallagher"] {
                        ui.selectable_value(&mut self.kdf, k.into(), k);
                    }
                });
            ui.label("KDF input (hex)");
            ui.text_edit_singleline(&mut self.kdf_input);
        });
        if self.action == Action::ChangeKey {
            ui.label(
                "Old and new key are never persisted. Changing keys may permanently remove access.",
            );
            ui.horizontal(|ui| {
                ui.label("Old key");
                ui.add(egui::TextEdit::singleline(&mut *self.oldkey).password(true));
                ui.label("New key");
                ui.add(egui::TextEdit::singleline(&mut *self.newkey).password(true));
            });
            ui.horizontal(|ui| {
                ui.label("Target key number");
                ui.add(egui::DragValue::new(&mut self.newkeyno).range(0..=13));
                ui.label("New version (hex)");
                ui.text_edit_singleline(&mut self.version);
            });
        }
        if matches!(self.action, Action::WriteRecord | Action::WriteData) {
            ui.label("Record bytes (hex)");
            ui.text_edit_singleline(&mut self.data);
        }
        ui.label("Operations use the authentication, AID and file selected above. Credit/debit commits follow RRG value semantics. Mutations run a fresh state check; unsupported or ambiguous results do not verify.");
        ui.add_enabled(
            false,
            egui::Button::new(
                "EV2 / LRP / EV3 secure operations and transaction-MAC creation — unsupported",
            ),
        );
        ui.add_enabled(
            self.action.available(file, key_number, no_auth),
            egui::Button::new(if self.action.write() {
                "Review management operation…"
            } else {
                "Run management read"
            }),
        )
        .clicked()
    }
}
impl Action {
    pub fn available(
        self,
        file: Option<&crate::desfire_archive::File>,
        key_number: u8,
        no_auth: bool,
    ) -> bool {
        let Some(f) = file else { return true };
        let allowed = |write: bool| {
            let shifts = if write { [8, 4] } else { [12, 4] };
            let permit = shifts.iter().any(|shift| {
                let n = ((f.rights >> shift) & 15) as u8;
                n == 14 || !no_auth && n == key_number
            });
            permit && (f.mode == crate::model::Communication::Plain || !no_auth)
        };
        match self {
            Self::ReadData => f.kind <= 1 && allowed(false),
            Self::WriteData => f.kind <= 1 && allowed(true) && allowed(false),
            Self::ReadRecord => matches!(f.kind, 3 | 4) && allowed(false),
            Self::WriteRecord | Self::ClearRecords => {
                matches!(f.kind, 3 | 4) && allowed(true) && allowed(false)
            }
            Self::ReadValue => f.kind == 2 && allowed(false),
            Self::Credit | Self::Debit | Self::ClearValue => {
                f.kind == 2 && allowed(true) && allowed(false)
            }
            Self::LimitedCredit => {
                f.kind == 2 && f.limited & 1 != 0 && allowed(true) && allowed(false)
            }
            Self::ChangeRights => {
                let key = (f.rights & 15) as u8;
                key == 14 || !no_auth && key == key_number
            }
            _ => true,
        }
    }
}
