use crate::{
    backup::{self, Batch, Dump, Geometry, Step},
    command::encode,
    runner::Job,
};
use eframe::egui;
use zeroize::{Zeroize, Zeroizing};
pub struct Workbench {
    pub origin_simulated: Option<bool>,
    pub selection: String,
    pub selection_pending: Option<Vec<u32>>,
    pub selected_result: Vec<(u32, Vec<u8>)>,
    pub draft_history: crate::memory_tools::DraftHistory,
    pub unit_edit: String,
    pub protected_edit: bool,
    pub search_text: String,
    pub search_ascii: bool,
    pub search_hits: Vec<usize>,
    pub changed_only: bool,
    pub geometry: Geometry,
    pub uid: String,
    pub key: Zeroizing<String>,
    pub key_b: bool,
    pub key_map: Zeroizing<String>,
    pub key_refs: String,
    pub use_map: bool,
    pub path: String,
    pub compare_path: String,
    pub comparison: Option<Dump>,
    pub editor: String,
    pub unit: u32,
    pub dump: Option<Dump>,
    pub pending: bool,
    pub message: String,
    pub trailer_editor: crate::classic::TrailerEditor,
}
impl Default for Workbench {
    fn default() -> Self {
        Self {
            origin_simulated: None,
            selection: "4-6".into(),
            selection_pending: None,
            selected_result: vec![],
            draft_history: Default::default(),
            unit_edit: String::new(),
            protected_edit: false,
            search_text: String::new(),
            search_ascii: false,
            search_hits: vec![],
            changed_only: false,
            geometry: Geometry::default(),
            uid: String::new(),
            key: Zeroizing::new(String::new()),
            key_b: false,
            key_map: Zeroizing::new(String::new()),
            key_refs: String::new(),
            use_map: false,
            path: String::new(),
            compare_path: String::new(),
            comparison: None,
            editor: String::new(),
            unit: 4,
            dump: None,
            pending: false,
            message: String::new(),
            trailer_editor: Default::default(),
        }
    }
}
impl Workbench {
    pub fn receive(&mut self, bytes: Vec<u8>) {
        self.pending = false;
        match Dump::new(self.geometry, &self.uid, bytes) {
            Ok(d) => {
                self.editor = encode(&d.bytes);
                self.dump = Some(d);
                self.draft_history.reset();
                self.message =
                    "Complete readable-memory backup captured. Save to preserve it.".into()
            }
            Err(e) => self.message = e,
        }
    }
    pub fn build(&self, write: bool, single: bool, verify: bool) -> Result<Job, String> {
        self.build_units(write, single, verify, None)
    }
    pub fn build_selection(&self, write: bool, verify: bool) -> Result<Job, String> {
        let units = crate::memory_tools::selection(&self.selection, self.geometry)?;
        self.build_units(write, false, verify, Some(units))
    }
    fn build_units(
        &self,
        write: bool,
        single: bool,
        verify: bool,
        selected: Option<Vec<u32>>,
    ) -> Result<Job, String> {
        backup::validate_uid(&self.uid)?;
        if single && self.unit >= self.geometry.units() {
            return Err("Selected block/page exceeds the new geometry".into());
        }
        let data = if write || verify {
            let d = self.dump.as_ref().ok_or("Load or capture a dump first")?;
            d.validate()?;
            if d.geometry != self.geometry {
                return Err("Loaded dump geometry differs from selection".into());
            }
            Some(d)
        } else {
            None
        };
        let units: Vec<_> = if let Some(units) = selected {
            units
        } else if single {
            vec![self.unit]
        } else {
            (0..self.geometry.units())
                .filter(|u| !write || self.geometry.writable(*u))
                .collect()
        };
        if write && units.iter().any(|u| !self.geometry.writable(*u)) {
            return Err("Selection includes protected units. Remove UID/manufacturer, lock/configuration or trailer units before writing.".into());
        }
        let map = if self.geometry.classic() && self.use_map {
            Some(if self.key_map.trim().is_empty() {
                let refs = serde_json::from_str::<Vec<crate::classic::Reference>>(&self.key_refs)
                    .map_err(|_| "Invalid key-reference JSON")?;
                crate::classic::KeyMap::resolve(&refs, self.geometry)?
            } else {
                crate::classic::KeyMap::parse(&self.key_map, self.geometry)?
            })
        } else {
            None
        };
        let mut steps = vec![];
        for u in units {
            let bytes = data.map(|d| {
                d.bytes
                    [u as usize * self.geometry.width()..(u as usize + 1) * self.geometry.width()]
                    .to_vec()
            });
            let keys = map.as_ref().map(|m| m.get(u)).transpose()?;
            let (rk, rb, wk, wb) = if let Some(k) = keys {
                (
                    k.read_key.as_str(),
                    k.read_type == crate::classic::KeyType::B,
                    k.write_key.as_str(),
                    k.write_type == crate::classic::KeyType::B,
                )
            } else {
                (self.key.as_str(), self.key_b, self.key.as_str(), self.key_b)
            };
            steps.push(Step {
                read: self.geometry.read(u, rk, rb)?,
                write: if write {
                    Some(self.geometry.write(u, bytes.as_ref().unwrap(), wk, wb)?)
                } else {
                    None
                },
                expected: bytes,
            });
        }
        Ok(Job::Batch(Batch {
            geometry: self.geometry,
            uid: self.uid.to_uppercase(),
            steps,
            writing: write,
        }))
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<(Job, bool)> {
        let mut requested_trailer = None;
        ui.heading("Card memory workbench");
        ui.label("Capture, edit, compare and restore readable memory. Restore skips UID/manufacturer blocks, lock/configuration pages and Classic sector trailers. A matching UID is required throughout each batch.");
        egui::ComboBox::from_label("Memory geometry")
            .selected_text(format!("{:?}", self.geometry))
            .show_ui(ui, |ui| {
                for g in Geometry::ALL {
                    ui.selectable_value(&mut self.geometry, g, format!("{g:?}"));
                }
            });
        ui.horizontal(|ui| {
            ui.label("Expected target UID");
            ui.text_edit_singleline(&mut self.uid);
        });
        if self.geometry.classic() {
            ui.horizontal(|ui| {
                ui.label("Known Classic key (one key across selected blocks)");
                ui.add(egui::TextEdit::singleline(&mut *self.key).password(true));
                ui.checkbox(&mut self.key_b, "Key B");
            });
        }
        if self.geometry.classic() {
            ui.collapsing("Per-sector key map",|ui|{
            ui.checkbox(&mut self.use_map,"Use sector-specific read/write keys");
            ui.label("Session-only CSV: sector,A-or-B,read key,A-or-B,write key. Every selected sector needs an entry. Session keys override vault references; clear them to use references.");
            ui.add(egui::TextEdit::multiline(&mut *self.key_map).password(true).desired_rows(4));
            ui.label("Persistent reference JSON: [{sector:0,read_type:A,read_alias:...,write_type:B,write_alias:...}] (use quoted JSON strings)");
            ui.add(egui::TextEdit::multiline(&mut self.key_refs).desired_rows(3));
            ui.horizontal(|ui|{if ui.button("Load key references from dump path").clicked(){self.message=match crate::storage::read_bounded(std::path::Path::new(&self.path),65536).and_then(|b|serde_json::from_slice::<Vec<crate::classic::Reference>>(&b).map_err(|e|e.to_string())){Ok(r)=>{self.key_refs=serde_json::to_string_pretty(&r).unwrap();"References loaded; keys will be resolved from the OS vault at execution".into()},Err(e)=>e};}
if ui.button("Save references to dump path").clicked(){self.message=match serde_json::from_str::<Vec<crate::classic::Reference>>(&self.key_refs).map_err(|e|e.to_string()).and_then(|r|{let b=serde_json::to_vec_pretty(&r).map_err(|e|e.to_string())?;crate::storage::write_private(std::path::Path::new(&self.path),&b)}){Ok(())=>"Only aliases saved".into(),Err(e)=>e};}});
            if ui.button("Forget session sector keys").clicked(){self.key_map.zeroize();}
        });
        }
        if self.geometry.classic() {
            ui.collapsing("Classic access bits and trailer keys", |ui| {
                if let Some(mut request) = self.trailer_editor.ui(
                    ui,
                    &self.uid,
                    self.geometry,
                    if self.use_map {
                        "000000000000"
                    } else {
                        self.key.as_str()
                    },
                    if self.key_b {
                        crate::classic::KeyType::B
                    } else {
                        crate::classic::KeyType::A
                    },
                ) {
                    if self.use_map {
                        let resolved = (|| {
                            let map = if self.key_map.trim().is_empty() {
                                let refs = serde_json::from_str::<Vec<crate::classic::Reference>>(
                                    &self.key_refs,
                                )
                                .map_err(|_| "Invalid key reference JSON")?;
                                crate::classic::KeyMap::resolve(&refs, self.geometry)?
                            } else {
                                crate::classic::KeyMap::parse(&self.key_map, self.geometry)?
                            };
                            let keys = map.get(crate::classic::trailer(request.sector)?)?;
                            Ok::<_, String>((keys.write_type, keys.write_key.clone()))
                        })();
                        match resolved {
                            Ok((t, key)) => {
                                request.current_type = t;
                                request.current_key = key;
                                requested_trailer = Some(request)
                            }
                            Err(e) => self.trailer_editor.message = e,
                        }
                    } else {
                        requested_trailer = Some(request);
                    }
                }
            });
        }
        ui.colored_label(egui::Color32::YELLOW,"Dump files contain readable card data. Store them securely. A locked or unreadable page stops a complete backup; no partial dump is labelled complete.");
        let mut requested = requested_trailer.map(|r| (Job::ClassicTrailer(r), true));
        ui.horizontal_wrapped(|ui| {
            for (label, w, s, v) in [
                ("Capture full memory", false, false, false),
                ("Verify entire dump", false, false, true),
                ("Restore data & verify…", true, false, false),
                ("Read selected unit", false, true, false),
                ("Programme selected unit & verify…", true, true, false),
            ] {
                if ui.button(label).clicked() {
                    match self.build(w, s, v) {
                        Ok(job) => {
                            self.pending = !w && !s && !v;
                            requested = Some((job, w))
                        }
                        Err(e) => self.message = e,
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Selected page/block");
            ui.add(egui::DragValue::new(&mut self.unit).range(0..=self.geometry.units() - 1));
        });
        ui.collapsing("Selected blocks / pages", |ui| {
            ui.label("Comma-separated units and ranges, for example 4-8,12. Reads remain selections; they do not replace the complete dump.");
            ui.text_edit_singleline(&mut self.selection);
            ui.horizontal_wrapped(|ui| {
                for (label,write,verify) in [("Read selection",false,false),("Verify selection",false,true),("Programme selection & verify…",true,false)] {
                    if ui.button(label).clicked() {
                        match self.build_selection(write,verify) {
                            Ok(job)=>{self.pending=false; self.selected_result.clear(); self.selection_pending=if !write&&!verify {crate::memory_tools::selection(&self.selection,self.geometry).ok()}else{None}; requested=Some((job,write));}
                            Err(e)=>self.message=e,
                        }
                    }
                }
            });
            for (unit,bytes) in &self.selected_result {ui.monospace(format!("Unit {unit}: {}",encode(bytes)));}
            if !self.selected_result.is_empty() {ui.label("Selection only. Not a complete memory backup.");}
        });
        self.local_editor(ui);
        ui.horizontal(|ui| {
            ui.label("Dump path (.json, raw/RRG-MFU .bin or .eml)");
            ui.text_edit_singleline(&mut self.path);
            if ui.button("Browse…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Memory dumps", &["json", "bin", "dump", "eml"])
                    .pick_file()
                {
                    self.path = p.display().to_string();
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Load").clicked() {
                let res = (|| {
                    let p = std::path::Path::new(&self.path);
                    let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
                    if meta.len() > 65536 {
                        return Err("Dump file too large".into());
                    }
                    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
                    let dump = backup::import_memory(
                        &bytes,
                        p.extension().and_then(|v| v.to_str()).unwrap_or(""),
                        self.geometry,
                        &self.uid,
                    )?;
                    dump.validate()?;
                    Ok(dump)
                })();
                match res {
                    Ok(d) => {
                        self.geometry = d.geometry;
                        self.origin_simulated = None;
                        self.editor = encode(&d.bytes);
                        self.draft_history.reset();
                        self.dump = Some(d);
                        self.message = "Dump loaded and integrity checked".into()
                    }
                    Err(e) => self.message = e,
                }
            }
            if ui.button("Save JSON").clicked() {
                self.message = match &self.dump {
                    Some(d) => serde_json::to_vec_pretty(d)
                        .map_err(|e| e.to_string())
                        .and_then(|b| {
                            crate::storage::write_private(std::path::Path::new(&self.path), &b)
                        })
                        .map(|()| "Dump saved".into())
                        .unwrap_or_else(|e| e),
                    None => "Capture or load a dump first".into(),
                }
            }
            if ui.button("Export raw binary").clicked() {
                self.message = match &self.dump {
                    Some(d) => {
                        crate::storage::write_private(std::path::Path::new(&self.path), &d.bytes)
                            .map(|()| "Raw memory exported (metadata not included)".into())
                            .unwrap_or_else(|e| e)
                    }
                    None => "No dump loaded".into(),
                }
            }
        });
        if ui.button("Export EML to dump path").clicked() {
            self.message = self
                .dump
                .as_ref()
                .ok_or("Load a dump first".to_owned())
                .and_then(|d| d.eml())
                .and_then(|text| {
                    crate::storage::write_private(std::path::Path::new(&self.path), text.as_bytes())
                })
                .map(|()| "EML memory exported".into())
                .unwrap_or_else(|e| e);
        }
        if let Some(d) = &self.dump {
            ui.collapsing(
                "Address / hex / ASCII view and side-by-side comparison",
                |ui| {
                    ui.checkbox(&mut self.changed_only, "Show only differing rows");
                    eframe::egui::ScrollArea::vertical()
                        .max_height(250.0)
                        .show_rows(ui, 22.0, d.bytes.len().div_ceil(16), |ui, range| {
                            for row in range {
                                let start = row * 16;
                                let end = (start + 16).min(d.bytes.len());
                                let bytes = &d.bytes[start..end];
                                let ascii = bytes
                                    .iter()
                                    .map(|b| {
                                        if b.is_ascii_graphic() {
                                            char::from(*b)
                                        } else {
                                            '.'
                                        }
                                    })
                                    .collect::<String>();
                                let right = self
                                    .comparison
                                    .as_ref()
                                    .filter(|c| c.geometry == d.geometry)
                                    .map(|c| &c.bytes[start..end]);
                                let different = right.is_some_and(|other| other != bytes);
                                if self.changed_only && !different {
                                    continue;
                                }
                                let line = format!(
                                    "{start:04X}  {:<32}  {ascii:<16}  {}",
                                    encode(bytes),
                                    right.map(encode).unwrap_or_default()
                                );
                                ui.colored_label(
                                    if different {
                                        eframe::egui::Color32::YELLOW
                                    } else {
                                        eframe::egui::Color32::GRAY
                                    },
                                    eframe::egui::RichText::new(line).monospace(),
                                );
                            }
                        });
                },
            );
        }
        ui.label("Hex editor (complete memory, whitespace allowed)");
        ui.add(
            egui::TextEdit::multiline(&mut self.editor)
                .font(egui::TextStyle::Monospace)
                .desired_rows(6)
                .desired_width(f32::INFINITY),
        );
        if ui.button("Apply hex edits").clicked() {
            let compact = self.editor.split_whitespace().collect::<String>();
            let res = if compact.len() == self.geometry.units() as usize * self.geometry.width() * 2
                && compact.bytes().all(|b| b.is_ascii_hexdigit())
            {
                let bytes = (0..compact.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&compact[i..i + 2], 16).unwrap())
                    .collect();
                Dump::new(self.geometry, &self.uid, bytes)
            } else {
                Err("Hex data size must exactly match the selected geometry".into())
            };
            match res {
                Ok(d) => {
                    if let Some(old) = &self.dump {
                        self.draft_history.record(old);
                    }
                    self.dump = Some(d);
                    self.message = "Edits applied in memory; review before programming".into()
                }
                Err(e) => self.message = e,
            }
        }
        ui.horizontal(|ui| {
            ui.label("Compare memory dump");
            ui.text_edit_singleline(&mut self.compare_path);
            if ui.button("Compare").clicked() {
                let res = (|| {
                    let bytes = crate::storage::read_bounded(
                        std::path::Path::new(&self.compare_path),
                        65536,
                    )?;
                    if bytes.len() > 65536 {
                        return Err("Dump too large".into());
                    }
                    let other = backup::import_memory(
                        &bytes,
                        std::path::Path::new(&self.compare_path)
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or(""),
                        self.geometry,
                        &self.uid,
                    )?;
                    self.comparison = Some(other.clone());
                    self.dump
                        .as_ref()
                        .ok_or("Load a dump first")?
                        .differences(&other)
                })();
                self.message = match res {
                    Ok(d) => format!(
                        "{} differing bytes; first offsets: {:?}",
                        d.len(),
                        d.iter().take(32).collect::<Vec<_>>()
                    ),
                    Err(e) => e,
                }
            }
        });
        ui.label(&self.message);
        requested
    }
}

impl Workbench {
    pub fn receive_selection(&mut self, bytes: &[u8], completed: bool) {
        if let Some(units) = self.selection_pending.take() {
            self.selected_result.clear();
            if completed && bytes.len() == units.len() * self.geometry.width() {
                self.selected_result = units
                    .into_iter()
                    .zip(
                        bytes
                            .chunks_exact(self.geometry.width())
                            .map(|b| b.to_vec()),
                    )
                    .collect();
                self.message = "Selected units read. Complete dump remains unchanged.".into();
            } else {
                self.message = "Selection did not complete; no complete read is claimed.".into();
            }
        }
    }
    fn local_editor(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Local block editor, undo and search", |ui| {
            let Some(d) = self.dump.as_mut() else {
                ui.label("Load or capture a complete dump to edit a local draft.");
                return;
            };
            ui.label(
                "Edits affect the local draft. Card programming is a separate reviewed operation.",
            );
            ui.horizontal(|ui| {
                if ui.button("Undo draft edit").clicked() {
                    self.draft_history.undo(d);
                    self.editor = encode(&d.bytes);
                    self.protected_edit = false;
                }
                if ui.button("Redo draft edit").clicked() {
                    self.draft_history.redo(d);
                    self.editor = encode(&d.bytes);
                    self.protected_edit = false;
                }
                if ui.button("Copy selected unit into editor").clicked()
                    && self.unit < d.geometry.units()
                {
                    let start = self.unit as usize * d.geometry.width();
                    self.unit_edit = encode(&d.bytes[start..start + d.geometry.width()]);
                    self.protected_edit = false;
                }
            });
            ui.text_edit_singleline(&mut self.unit_edit);
            if !d.geometry.writable(self.unit) {
                ui.checkbox(
                    &mut self.protected_edit,
                    "I intend to edit protected metadata in this local draft",
                );
            }
            if ui.button("Apply unit to local draft").clicked() {
                match crate::memory_tools::edit_unit(
                    d,
                    self.unit,
                    &self.unit_edit,
                    self.protected_edit,
                ) {
                    Ok(next) => {
                        self.draft_history.record(d);
                        *d = next;
                        self.editor = encode(&d.bytes);
                        self.protected_edit = false;
                        self.message = "Local unit edited. Source file was not changed.".into();
                    }
                    Err(e) => self.message = e,
                }
            }
            if d.geometry.classic() {
                match crate::memory_tools::permissions(d, self.unit) {
                    Ok(lines) => {
                        for line in lines {
                            ui.label(line);
                        }
                    }
                    Err(e) => {
                        ui.colored_label(egui::Color32::YELLOW, e);
                    }
                }
            }
            ui.horizontal(|ui| {
                ui.label("Find bytes/text");
                ui.text_edit_singleline(&mut self.search_text);
                ui.checkbox(&mut self.search_ascii, "Text");
                if ui.button("Find in dump").clicked() {
                    match crate::memory_tools::search(d, &self.search_text, self.search_ascii) {
                        Ok(v) => self.search_hits = v,
                        Err(e) => self.message = e,
                    }
                }
            });
            ui.label(format!(
                "{} matches. Byte offsets: {:?}",
                self.search_hits.len(),
                self.search_hits.iter().take(32).collect::<Vec<_>>()
            ));
        });
    }
}
