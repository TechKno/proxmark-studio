use super::*;
use proxmark_studio::{backup::Geometry, emulator, magic, recovery};
pub struct ClassicView {
    pub tab: usize,
    pub uid: String,
    pub geometry: Geometry,
    pub method: recovery::Method,
    pub seed: Zeroizing<String>,
    pub sector: u8,
    pub key_b: bool,
    pub dictionary: Vec<u8>,
    pub dictionary_name: String,
    pub recovered: Option<recovery::Keys>,
    pub source: Option<emulator::Image>,
    pub target: Option<magic::Target>,
    pub generation: magic::Generation,
    pub destination_key: Zeroizing<String>,
    pub alias_prefix: String,
    pub message: String,
}
impl Default for ClassicView {
    fn default() -> Self {
        Self {
            tab: 0,
            uid: String::new(),
            geometry: Geometry::Classic1K,
            method: recovery::Method::Dictionary,
            seed: Zeroizing::new(String::new()),
            sector: 0,
            key_b: false,
            dictionary: vec![],
            dictionary_name: String::new(),
            recovered: None,
            source: None,
            target: None,
            generation: magic::Generation::Gen1a,
            destination_key: Zeroizing::new(String::new()),
            alias_prefix: "classic-lab".into(),
            message: String::new(),
        }
    }
}
impl ClassicView {
    pub fn forget(&mut self) {
        self.seed.zeroize();
        self.destination_key.zeroize();
        self.dictionary.zeroize();
        self.recovered = None;
        self.target = None;
    }
}
impl Studio {
    pub(super) fn classic_page(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "CLASSIC",
            "Classic cards",
            "Recover keys, capture a complete source and programme a compatible magic card.",
        );
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.classic.tab, 0, "Key recovery");
            ui.selectable_value(&mut self.classic.tab, 1, "Magic-card clone");
        });
        ui.separator();
        if self.classic.tab == 0 {
            self.recovery_page(ui)
        } else {
            self.magic_page(ui)
        }
        if !self.classic.message.is_empty() {
            ui.label(&self.classic.message);
        }
    }
    fn recovery_page(&mut self, ui: &mut egui::Ui) {
        ui.label("Place your Classic source on the HF antenna. Recovery can take several minutes and is not guaranteed. Automatic recovery replaces the reader's emulator buffer; it does not programme the card.");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.available("hf 14a info"),
                    egui::Button::new("Inspect source"),
                )
                .clicked()
            {
                self.classic.recovered = None;
                self.run_guided(
                    Ok(CommandSpec::simple("hf 14a info", Kind::Detect)),
                    "Inspect Classic source",
                );
            }
            ui.label("Source UID");
            ui.add(
                egui::TextEdit::singleline(&mut self.classic.uid)
                    .desired_width(170.0)
                    .hint_text("Inspect source first"),
            );
        });
        egui::ComboBox::from_label("Card size")
            .selected_text(format!("{:?}", self.classic.geometry))
            .show_ui(ui, |ui| {
                for g in Geometry::ALL.into_iter().filter(|g| g.classic()) {
                    if ui
                        .selectable_value(&mut self.classic.geometry, g, format!("{g:?}"))
                        .changed()
                    {
                        self.classic.recovered = None;
                    }
                }
            });
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.classic.method,
                recovery::Method::Dictionary,
                "Check dictionary",
            );
            ui.selectable_value(
                &mut self.classic.method,
                recovery::Method::Automatic,
                "Automatic recovery",
            );
        });
        ui.label("Dictionary checks supplied/default candidates. Automatic recovery lets the installed RRG client select supported recovery methods, including nested and hardnested.");
        ui.horizontal_wrapped(|ui| {
            ui.label("Known key · optional");
            ui.add(
                egui::TextEdit::singleline(&mut *self.classic.seed)
                    .password(true)
                    .desired_width(180.0),
            );
            ui.label("Sector");
            ui.add(
                egui::DragValue::new(&mut self.classic.sector)
                    .range(0..=self.classic.geometry.sectors() - 1),
            );
            ui.checkbox(&mut self.classic.key_b, "Key B");
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Choose dictionary…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Key dictionary", &["dic", "txt"])
                    .pick_file()
                {
                    match storage::read_bounded(&path, 1024 * 1024)
                        .and_then(|b| recovery::dictionary(&b))
                    {
                        Ok(b) => {
                            self.classic.dictionary.zeroize();
                            self.classic.dictionary = b;
                            self.classic.dictionary_name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into();
                        }
                        Err(e) => self.classic.message = e,
                    }
                }
            }
            ui.label(&self.classic.dictionary_name);
            if ui.button("Clear dictionary").clicked() {
                self.classic.dictionary.zeroize();
                self.classic.dictionary_name.clear();
            }
        });
        let cmd = if self.classic.method == recovery::Method::Automatic {
            "hf mf autopwn"
        } else {
            "hf mf fchk"
        };
        if ui
            .add_enabled(
                self.available(cmd)
                    && proxmark_studio::backup::validate_uid(&self.classic.uid).is_ok(),
                egui::Button::new("Review key recovery…"),
            )
            .clicked()
        {
            self.review_ack = false;
            self.review=Some(Review{summary:"Recover keys from the inspected Classic source. Raw key output is kept out of logs and no key files are saved by the client. Automatic recovery may replace emulator memory. Each reported key is independently checked; incomplete recovery remains labelled incomplete. The operation can be cancelled and has a 15-minute minimum timeout.".into(),job:Job::Recover{caps:self.caps.clone(),request:recovery::Request{uid:self.classic.uid.clone(),geometry:self.classic.geometry,method:self.classic.method,seed:self.classic.seed.clone(),sector:self.classic.sector,key_b:self.classic.key_b,dictionary:self.classic.dictionary.clone(),simulated:self.simulation.is_some(),acknowledged:true}}});
        }
        if let Some(keys) = self.classic.recovered.clone() {
            let (n, v) = keys.counts();
            ui.heading(format!("{v} of {n} found keys confirmed"));
            ui.label(format!(
                "Source {} · {:?}{}",
                keys.uid,
                keys.geometry,
                if keys.simulated { " · Simulated" } else { "" }
            ));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Use confirmed keys in Card memory").clicked() {
                    match keys.map() {
                        Ok(map) => {
                            self.bench.key_map = map;
                            self.bench.geometry = keys.geometry;
                            self.bench.uid = keys.uid.clone();
                            self.page = Page::Workbench;
                        }
                        Err(e) => self.classic.message = e,
                    }
                }
                if ui
                    .add_enabled(
                        keys.complete(),
                        egui::Button::new("Capture complete source"),
                    )
                    .clicked()
                {
                    self.start(
                        Job::CaptureClassic {
                            keys: keys.clone(),
                            simulated: self.simulation.is_some(),
                            caps: self.caps.clone(),
                        },
                        "Capture Classic source",
                    );
                }
            });
            egui::ScrollArea::vertical()
                .id_salt("key-sector-results")
                .max_height(220.0)
                .show(ui, |ui| {
                    egui::Grid::new("recovered_keys")
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Sector");
                            ui.strong("Key A");
                            ui.strong("Key B");
                            ui.end_row();
                            for (s, pair) in &keys.sectors {
                                ui.label(s.to_string());
                                for k in pair {
                                    ui.label(match k {
                                        Some(k) if k.verified => "Confirmed · hidden",
                                        Some(_) => "Found · unconfirmed",
                                        None => "Missing",
                                    });
                                }
                                ui.end_row();
                            }
                        });
                });
            ui.horizontal_wrapped(|ui|{ui.label("Key-store name prefix");ui.text_edit_singleline(&mut self.classic.alias_prefix);if ui.button("Save confirmed keys to key store").clicked(){self.classic.message=self.store_recovered_keys(&keys).map(|n|format!("{n} confirmed keys saved to the OS key store. No key values were written to settings.")).unwrap_or_else(|e|e);}});
        }
    }
    fn store_recovered_keys(&mut self, keys: &recovery::Keys) -> Result<usize, String> {
        let prefix = self.classic.alias_prefix.trim();
        if prefix.is_empty()
            || prefix.len() > 64
            || !prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(
                "Use a 1–64 character key-store prefix containing letters, digits, - or _.".into(),
            );
        }
        let mut n = 0;
        for (s, pair) in &keys.sectors {
            for (i, k) in pair.iter().enumerate() {
                if let Some(k) = k.as_ref().filter(|k| k.verified) {
                    let alias = format!(
                        "{prefix}-{}-{s}-{}",
                        keys.uid,
                        if i == 0 { "A" } else { "B" }
                    );
                    proxmark_studio::vault::store(&alias, &k.value, 6)?;
                    n += 1;
                }
            }
        }
        Ok(n)
    }
    fn magic_page(&mut self, ui: &mut egui::Ui) {
        ui.label("Full Classic 1K cloning for confirmed Gen 1a or Gen 2/CUID blanks with a four-byte UID. Gen 3, Gen 4, write-once cards and ambiguous detections are refused. Gen 2 requires transport permissions and a known destination key A.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Use Card memory draft").clicked() {
                match self.bench.dump.clone() {
                    Some(dump) => {
                        self.classic.source = Some(magic::image(
                            dump,
                            self.bench
                                .origin_simulated
                                .unwrap_or(self.simulation.is_some()),
                        ));
                        self.classic.target = None;
                    }
                    None => self.classic.message = "Load a complete source image first".into(),
                }
            }
            if ui.button("Import source image…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Classic image", &["json", "bin", "eml", "dump"])
                    .pick_file()
                {
                    match storage::read_bounded(&path, 65536).and_then(|bytes| {
                        emulator::import_image(
                            &bytes,
                            path.extension().and_then(|v| v.to_str()).unwrap_or(""),
                            Geometry::Classic1K,
                            &self.classic.uid,
                            "Imported source",
                            self.simulation.is_some(),
                        )
                    }) {
                        Ok(image) => {
                            self.classic.source = Some(image);
                            self.classic.target = None;
                        }
                        Err(e) => self.classic.message = e,
                    }
                }
            }
        });
        if let Some(source) = &self.classic.source {
            ui.label(format!(
                "Source {} · {} bytes{}",
                source.dump.uid,
                source.dump.bytes.len(),
                if source.simulated {
                    " · Simulated"
                } else {
                    ""
                }
            ));
            if let Err(e) = magic::validate_source(source) {
                ui.colored_label(ui.visuals().warn_fg_color, e);
            }
        } else {
            ui.label("Recover and capture the source on the Key recovery tab, or import a complete image with known trailer keys.");
        }
        ui.horizontal(|ui| {
            ui.label("Blank type");
            if ui
                .selectable_value(
                    &mut self.classic.generation,
                    magic::Generation::Gen1a,
                    "Gen 1a",
                )
                .changed()
                || ui
                    .selectable_value(
                        &mut self.classic.generation,
                        magic::Generation::Gen2,
                        "Gen 2 / CUID",
                    )
                    .changed()
            {
                self.classic.target = None;
            }
        });
        if self.classic.generation == magic::Generation::Gen2 {
            ui.horizontal(|ui| {
                ui.label("Destination key A");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut *self.classic.destination_key)
                            .password(true),
                    )
                    .changed()
                {
                    self.classic.target = None;
                }
            });
        }
        if ui
            .add_enabled(
                self.available("hf 14a info"),
                egui::Button::new("Inspect blank"),
            )
            .clicked()
        {
            self.classic.target = None;
            self.classic.message.clear();
            self.start(
                Job::Magic {
                    request: magic::Request {
                        action: magic::Action::Inspect {
                            generation: self.classic.generation,
                            key: self.classic.destination_key.clone(),
                        },
                        simulated: self.simulation.is_some(),
                    },
                    caps: self.caps.clone(),
                },
                "Inspect magic destination",
            );
        }
        if let Some(target) = self.classic.target.clone() {
            ui.label(format!(
                "Observed {:?} · destination UID {}",
                target.generation, target.uid
            ));
            if let Some(source) = self.classic.source.clone() {
                if ui
                    .add_enabled(
                        magic::validate_source(&source).is_ok()
                            && source.simulated == self.simulation.is_some(),
                        egui::Button::new("Review full-card clone…"),
                    )
                    .clicked()
                {
                    self.review_ack = false;
                    self.review=Some(Review{summary:format!("Overwrite all 64 blocks of the inspected {:?} blank {}, including trailer keys and manufacturer/UID bytes. The source UID becomes {}. Every write is checked; an interruption may leave a partly programmed card. There is no automatic retry or rollback. Access-reader acceptance is a separate test.",target.generation,target.uid,source.dump.uid),job:Job::Magic{request:magic::Request{action:magic::Action::Clone{source,target,key:self.classic.destination_key.clone(),acknowledged:true},simulated:self.simulation.is_some()},caps:self.caps.clone()}});
                }
            }
        }
        ui.label("The complete source image may contain keys. Inspect or export it explicitly from Card memory.");
    }
}
