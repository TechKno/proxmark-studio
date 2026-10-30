//! Use the installed RRG codec for its advertised format library, including issue/OEM fields.
use crate::{
    command::{CommandSpec, Kind},
    credential::Target,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Descriptor {
    pub name: String,
    pub description: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Credential {
    pub name: String,
    pub format: String,
    pub facility: u32,
    pub card: u64,
    pub issue: u32,
    pub oem: u32,
    pub target: Target,
}
impl Default for Credential {
    fn default() -> Self {
        Self {
            name: String::new(),
            format: "H10301".into(),
            facility: 118,
            card: 1603,
            issue: 0,
            oem: 0,
            target: Target::T5577,
        }
    }
}
impl Credential {
    pub fn encode(&self) -> Result<CommandSpec, String> {
        if self.format.is_empty()
            || self.format.len() > 15
            || !self.format.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err("Invalid RRG format name".into());
        }
        CommandSpec::new(
            format!(
                "wiegand encode -w {} --fc {} --cn {} --issue {} --oem {} --pre",
                self.format, self.facility, self.card, self.issue, self.oem
            ),
            "Encode advertised RRG Wiegand format",
            false,
            Kind::Help,
        )
    }
    pub fn write(&self, raw: &str) -> Result<CommandSpec, String> {
        let raw = crate::credential::normalise_raw(raw)?;
        let flag = match self.target {
            Target::T5577 => "",
            Target::Q5 => " --q5",
            Target::Em4305 => " --em",
        };
        CommandSpec::new(
            format!("lf hid clone -r {raw}{flag}"),
            format!("Programme RRG {} encoding", self.format),
            false,
            Kind::Write,
        )
    }
}
pub fn list(raw: &str) -> Result<Vec<Descriptor>, String> {
    let mut out = vec![];
    let bits = regex::Regex::new(r"(?i)\b\d+\s*-?\s*bit").unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for l in raw.lines() {
        let l = crate::parse::clean(l);
        let body = l.trim().strip_prefix("[=]").unwrap_or(&l).trim();
        if let Some((name, description)) = body.split_once(char::is_whitespace) {
            let description = description.trim();
            if name.len() <= 15
                && name.bytes().all(|b| b.is_ascii_alphanumeric())
                && bits.is_match(description)
            {
                if !seen.insert(name.to_owned()) {
                    return Err("Duplicate advertised format".into());
                }
                out.push(Descriptor {
                    name: name.into(),
                    description: description.into(),
                });
            }
        }
    }
    if out.is_empty() {
        return Err("No recognised RRG format catalogue".into());
    }
    Ok(out)
}
pub fn encoded(raw: &str) -> Result<String, String> {
    let re = regex::Regex::new(r"(?i)^Wiegand:\s*([0-9a-f]{1,24})$").unwrap();
    let all = raw
        .lines()
        .map(crate::parse::clean)
        .filter_map(|l| {
            let l = l.trim().strip_prefix("[+]").unwrap_or(&l).trim();
            re.captures(l).map(|c| c[1].to_owned())
        })
        .collect::<Vec<_>>();
    if all.len() != 1 {
        return Err("RRG did not produce one encoding; check format-specific field limits".into());
    }
    crate::credential::normalise_raw(&all[0])
}
#[derive(Default)]
pub struct Workspace {
    pub selected: Credential,
    pub formats: Vec<Descriptor>,
    pub encoded: String,
    pub input: String,
    pub decoded: String,
    pub search: String,
    pub message: String,
}
impl Workspace {
    pub fn ui(
        &mut self,
        ui: &mut eframe::egui::Ui,
        profiles: &mut Vec<Credential>,
    ) -> Option<crate::runner::Job> {
        use crate::runner::Job;
        use eframe::egui;
        ui.heading("Installed RRG format library");
        ui.label("Load the selected CLI's catalogue, encode facility/card/issue/OEM fields offline, then programme and verify exact HID raw data with a separate read. The six built-in formats retain independent local parity checks; this wider library uses the RRG codec.");
        let mut request = None;
        if ui.button("Load advertised formats (offline)").clicked() {
            request = Some(Job::FormatCatalogue);
        }
        ui.horizontal(|ui| {
            ui.label("Search formats");
            ui.text_edit_singleline(&mut self.search);
            egui::ComboBox::from_label("RRG format")
                .selected_text(&self.selected.format)
                .show_ui(ui, |ui| {
                    for f in self.formats.iter().filter(|f| {
                        format!("{} {}", f.name, f.description)
                            .to_lowercase()
                            .contains(&self.search.to_lowercase())
                    }) {
                        if ui
                            .selectable_value(
                                &mut self.selected.format,
                                f.name.clone(),
                                format!("{} • {}", f.name, f.description),
                            )
                            .changed()
                        {
                            self.encoded.clear();
                        }
                    }
                });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Facility");
            ui.add(egui::DragValue::new(&mut self.selected.facility));
            ui.label("Card");
            ui.add(egui::DragValue::new(&mut self.selected.card));
            ui.label("Issue");
            ui.add(egui::DragValue::new(&mut self.selected.issue));
            ui.label("OEM");
            ui.add(egui::DragValue::new(&mut self.selected.oem));
        });
        egui::ComboBox::from_label("Library writable target")
            .selected_text(self.selected.target.name())
            .show_ui(ui, |ui| {
                for t in Target::ALL {
                    ui.selectable_value(&mut self.selected.target, t, t.name());
                }
            });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Encode using selected CLI").clicked() {
                request = Some(Job::FormatEncode(self.selected.clone()));
            }
            if ui.button("Review programme from fresh encoding…").clicked() {
                request = Some(Job::FormatProgramme(self.selected.clone()));
            }
        });
        if !self.encoded.is_empty() {
            ui.monospace(format!("Last generated HID raw: {}", self.encoded));
        }
        ui.horizontal(|ui| {
            ui.label("Raw hex to decode");
            ui.text_edit_singleline(&mut self.input);
            if ui.button("Find CLI decode candidates (offline)").clicked() {
                request = Some(Job::FormatDecode(self.input.clone()));
            }
        });
        if !self.decoded.is_empty() {
            ui.monospace(&self.decoded);
        }
        ui.horizontal(|ui| {
            ui.label("Library profile name");
            ui.text_edit_singleline(&mut self.selected.name);
            if ui.button("Save library credential profile").clicked() {
                if self.selected.name.trim().is_empty() || self.selected.name.len() > 80 {
                    self.message = "Enter a profile name of 1–80 bytes".into()
                } else {
                    profiles.retain(|p| p.name != self.selected.name);
                    profiles.push(self.selected.clone());
                    self.message = "Profile saved in local settings".into();
                }
            }
        });
        for p in profiles.clone() {
            ui.horizontal(|ui| {
                if ui.button(format!("Load {}", p.name)).clicked() {
                    self.selected = p.clone();
                    self.encoded.clear();
                }
                if ui.small_button(format!("Delete {}", p.name)).clicked() {
                    profiles.retain(|v| v.name != p.name);
                }
            });
        }
        ui.label(&self.message);
        request
    }
}
