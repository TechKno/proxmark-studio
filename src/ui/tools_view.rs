use super::*;
use proxmark_studio::{
    backup::{Dump, Geometry},
    emulator, explorer,
    signal::Signal,
};
use std::collections::{BTreeMap, BTreeSet};

pub struct ToolsView {
    pub live: bool,
    pub next_capture: Instant,
    capture_samples: u32,
    demod: proxmark_studio::acquisition::Demod,
    capture: Option<proxmark_studio::acquisition::Capture>,
    inspect_sample: usize,
    pub tab: usize,
    pub connection: String,
    pub entries: Vec<explorer::Entry>,
    query: String,
    group: String,
    favourites_only: bool,
    selected: Option<explorer::Entry>,
    help: String,
    form_ready: bool,
    fields: Vec<explorer::OptionField>,
    enabled: BTreeSet<String>,
    values: BTreeMap<String, Zeroizing<String>>,
    pub buffer: Option<(Geometry, Vec<u8>)>,
    pub pending_geometry: Option<Geometry>,
    geometry: Geometry,
    image_name: String,
    image_uid: String,
    remove_ack: bool,
    pub message: String,
    signal: Option<Signal>,
    signal_label: String,
    start: usize,
    end: usize,
}
impl Default for ToolsView {
    fn default() -> Self {
        Self {
            live: false,
            next_capture: Instant::now(),
            capture_samples: 12000,
            demod: Default::default(),
            capture: None,
            inspect_sample: 0,
            tab: 0,
            connection: String::new(),
            entries: vec![],
            query: String::new(),
            group: String::new(),
            favourites_only: false,
            selected: None,
            help: String::new(),
            form_ready: false,
            fields: vec![],
            enabled: BTreeSet::new(),
            values: BTreeMap::new(),
            buffer: None,
            pending_geometry: None,
            geometry: Geometry::Classic1K,
            image_name: String::new(),
            image_uid: String::new(),
            remove_ack: false,
            message: String::new(),
            signal: None,
            signal_label: String::new(),
            start: 0,
            end: 0,
        }
    }
}
impl ToolsView {
    pub fn receive_signal(&mut self, c: proxmark_studio::acquisition::Capture) {
        self.end = c.signal.samples.len();
        self.start = 0;
        self.inspect_sample = 0;
        self.signal = Some(c.signal.clone());
        self.signal_label = format!(
            "{} {} capture",
            if c.simulated { "Simulated" } else { "" },
            if c.live { "live LF" } else { "saved" }
        );
        self.capture = Some(c);
        self.next_capture = Instant::now() + Duration::from_secs(1);
    }

    pub fn forget(&mut self) {
        self.live = false;
        self.values.clear();
        self.enabled.clear();
        self.buffer = None;
        self.pending_geometry = None;
    }
    pub fn receive_help(&mut self, name: &str, text: &str) {
        if self.selected.as_ref().is_some_and(|e| e.name == name) {
            self.form_ready = false;
            self.help = text.into();
            self.values.clear();
            self.enabled.clear();
            match explorer::options(name, text) {
                Ok(v) => {
                    self.form_ready = true;
                    self.fields = v;
                    self.message="Review the exact help. Forms cover simple documented switches; complex syntax belongs in Console.".into();
                }
                Err(e) => {
                    self.fields.clear();
                    self.message = e;
                }
            }
        }
    }
}
impl Studio {
    pub(super) fn tools_page(&mut self, ui: &mut egui::Ui) {
        Self::title(ui,"ADVANCED TOOLS","Explore, inspect and test","Commands follow your selected client. Device support and RF behaviour need their own evidence.");
        ui.horizontal_wrapped(|ui| {
            for (i, label) in [
                "Command explorer",
                "Emulator images",
                "Diagnostics",
                "Signal viewer",
            ]
            .iter()
            .enumerate()
            {
                if ui
                    .selectable_value(&mut self.tools.tab, i, *label)
                    .changed()
                {
                    self.tools.message.clear();
                }
            }
        });
        ui.separator();
        match self.tools.tab {
            0 => self.explorer_page(ui),
            1 => self.emulator_page(ui),
            2 => self.diagnostics_page(ui),
            _ => self.signal_page(ui),
        }
        if !self.tools.message.is_empty() {
            ui.add_space(10.0);
            ui.label(&self.tools.message);
        }
    }
    fn explorer_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Discover client commands").clicked() {
                self.start(Job::CommandCatalogue, "Discover client commands");
            }
            ui.label(format!("{} commands discovered", self.tools.entries.len()));
            ui.checkbox(&mut self.tools.favourites_only, "Favourites only");
        });
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.tools.query)
                .hint_text("Search names and descriptions · Ctrl/Cmd+K")
                .desired_width(f32::INFINITY),
        );
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
            response.request_focus();
        }
        let groups = self
            .tools
            .entries
            .iter()
            .filter_map(|e| e.name.split_whitespace().next().map(str::to_owned))
            .collect::<BTreeSet<_>>();
        egui::ComboBox::from_label("Command group")
            .selected_text(if self.tools.group.is_empty() {
                "All groups"
            } else {
                &self.tools.group
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.tools.group, String::new(), "All groups");
                for g in groups {
                    ui.selectable_value(&mut self.tools.group, g.clone(), g);
                }
            });
        let matches = explorer::filtered(
            &self.tools.entries,
            &self.tools.query,
            &self.tools.group,
            if self.tools.favourites_only {
                Some(&self.settings.command_favourites)
            } else {
                None
            },
        )
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
        ui.label(format!("{} matches", matches.len()));
        let mut choose = None;
        egui::ScrollArea::vertical()
            .id_salt("command_list")
            .max_height(190.0)
            .show_rows(ui, 28.0, matches.len(), |ui, range| {
                for i in range {
                    let e = &matches[i];
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(self.tools.selected.as_ref() == Some(e), &e.name)
                            .clicked()
                        {
                            choose = Some(e.clone());
                        }
                        ui.label(
                            RichText::new(&e.description)
                                .small()
                                .color(appearance::secondary(ui)),
                        );
                    });
                }
            });
        if let Some(entry) = choose {
            self.tools.form_ready = false;
            self.tools.help.clear();
            self.tools.fields.clear();
            self.tools.values.clear();
            self.tools.enabled.clear();
            self.tools.selected = Some(entry.clone());
            self.start(Job::CommandHelp(entry.name), "Read command help");
        }
        if let Some(entry) = self.tools.selected.clone() {
            ui.separator();
            ui.heading(&entry.name);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(if self.settings.command_favourites.contains(&entry.name) {
                        "Remove favourite"
                    } else {
                        "Add favourite"
                    })
                    .clicked()
                {
                    let _ = explorer::toggle_favourite(
                        &mut self.settings.command_favourites,
                        &entry.name,
                    );
                    self.save();
                }
                if ui.button("Refresh exact help").clicked() {
                    self.start(Job::CommandHelp(entry.name.clone()), "Read command help");
                }
                if ui.button("Open in Console").clicked() {
                    *self.console = entry.name.clone();
                    self.page = Page::Console;
                }
                ui.label(if entry.offline {
                    "Client advertises offline availability"
                } else {
                    "Client requires a connected device"
                });
            });
            if command::validate_line(&entry.name).is_err() {
                ui.colored_label(ui.visuals().warn_fg_color,"Unsupported EV2/LRP/EV3 operation. Help is viewable; execution remains disabled.");
            }
            egui::CollapsingHeader::new("Exact client help")
                .default_open(true)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("command_help")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.tools.help).monospace().size(12.0),
                                )
                                .wrap(),
                            );
                        });
                });
            for f in self.tools.fields.clone() {
                ui.push_id(&f.flag, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let mut enabled = self.tools.enabled.contains(&f.flag);
                        if ui
                            .checkbox(
                                &mut enabled,
                                format!(
                                    "{}{}",
                                    f.flag,
                                    if f.required { " (required)" } else { "" }
                                ),
                            )
                            .changed()
                        {
                            if enabled {
                                self.tools.enabled.insert(f.flag.clone());
                            } else {
                                self.tools.enabled.remove(&f.flag);
                                self.tools.values.remove(&f.flag);
                            }
                        }
                        if let Some(hint) = &f.value_hint {
                            let value = self.tools.values.entry(f.flag.clone()).or_default();
                            ui.add_enabled(
                                enabled,
                                egui::TextEdit::singleline(&mut **value)
                                    .password(is_secret(&f.flag, &f.description))
                                    .hint_text(hint)
                                    .desired_width(160.0),
                            );
                        }
                        ui.label(RichText::new(&f.description).small());
                    });
                });
            }
            if self.tools.form_ready {
                match explorer::build(
                    &entry,
                    &self.tools.fields,
                    &self.tools.enabled,
                    &self.tools.values,
                ) {
                    Ok(spec) => {
                        let sensitive = spec.sensitive
                            || self.tools.fields.iter().any(|f| {
                                self.tools.enabled.contains(&f.flag)
                                    && is_secret(&f.flag, &f.description)
                            });
                        ui.label("Command preview");
                        if sensitive {
                            ui.label(format!("{} [private arguments withheld]", entry.name));
                        } else {
                            ui.monospace(spec.text.as_str());
                        }
                        if ui.button("Review command…").clicked() {
                            let mut spec = spec;
                            spec.sensitive |= sensitive;
                            self.review_ack = false;
                            self.review=Some(Review{summary:format!("Run {} using the selected client. This may alter device/card state or files. CLI completion does not verify a write. {}",entry.name,if sensitive{"Private arguments and output will be withheld."}else{spec.text.as_str()}),job:Job::ExplorerRun{spec,device:!entry.offline}});
                        }
                    }
                    Err(e) => {
                        ui.colored_label(ui.visuals().warn_fg_color, e);
                    }
                }
            }
        }
    }
    fn emulator_request(&mut self, action: emulator::Action, summary: &str) {
        self.review_ack = false;
        self.review = Some(Review {
            summary: summary.into(),
            job: Job::Emulator(emulator::Request {
                action,
                caps: self.caps.clone(),
                simulated: self.simulation.is_some(),
                acknowledged: true,
            }),
        });
    }
    fn emulator_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Classic emulator images");
        if !self.inspected {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Check the connection in Device setup to enable supported buffer operations.",
            );
        }
        ui.label("Up to 16 named PC images. The reader has one current emulator buffer in this workflow. Loading an image replaces that buffer; it does not write a physical card.");
        ui.colored_label(ui.visuals().warn_fg_color,"Images may contain card data and Classic keys. Complete bytes and valid access bits do not prove recovered keys or reader acceptance.");
        egui::ComboBox::from_label("Image / buffer geometry")
            .selected_text(format!("{:?}", self.tools.geometry))
            .show_ui(ui, |ui| {
                for g in Geometry::ALL.into_iter().filter(|g| g.classic()) {
                    ui.selectable_value(&mut self.tools.geometry, g, format!("{g:?}"));
                }
            });
        ui.horizontal(|ui| {
            ui.label("New PC image name");
            ui.text_edit_singleline(&mut self.tools.image_name);
        });
        ui.horizontal(|ui| {
            ui.label("UID metadata for raw imports");
            ui.text_edit_singleline(&mut self.tools.image_uid);
        });
        ui.label(RichText::new("UID metadata is provenance only. RF simulation uses the bytes in the image; this field never rewrites them.").small().color(appearance::secondary(ui)));
        ui.label(RichText::new("Studio image files retain their name and demo marker. Raw dumps have no demo marker and use the current session context.").small().color(appearance::secondary(ui)));
        ui.horizontal_wrapped(|ui| {
            if ui.button("Import PC image…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Classic dumps", &["json", "bin", "dump", "eml"])
                    .pick_file()
                {
                    let r = storage::read_bounded(&p, 65536).and_then(|b| {
                        emulator::import_image(
                            &b,
                            p.extension().and_then(|s| s.to_str()).unwrap_or(""),
                            self.tools.geometry,
                            &self.tools.image_uid,
                            &self.tools.image_name,
                            self.simulation.is_some(),
                        )
                    });
                    self.tools.message = r
                        .and_then(|image| {
                            emulator::save_image(&mut self.settings.emulator_images, image)
                        })
                        .map(|()| {
                            "PC image imported with its provenance; no device operation".into()
                        })
                        .unwrap_or_else(|e| e);
                    self.save();
                }
            }
            if ui.button("Use workbench draft").clicked() {
                let r = if self
                    .bench
                    .origin_simulated
                    .is_some_and(|s| s != self.simulation.is_some())
                {
                    Err("Workbench draft belongs to another demo/physical context".into())
                } else {
                    self.bench
                        .dump
                        .clone()
                        .ok_or("Load a complete Classic dump in Card memory first".to_owned())
                };
                self.add_emulator_image(r);
            }
            if ui
                .add_enabled(
                    self.simulation.is_some(),
                    egui::Button::new("Load demo image into draft"),
                )
                .clicked()
            {
                let mut bytes = vec![0; 320];
                bytes[0..4].copy_from_slice(&[1, 2, 3, 4]);
                for n in 0..5 {
                    let o = (n * 4 + 3) * 16;
                    bytes[o..o + 6].fill(255);
                    bytes[o + 6..o + 10].copy_from_slice(&[255, 7, 128, 105]);
                    bytes[o + 10..o + 16].fill(255);
                }
                let d = Dump::new(Geometry::ClassicMini, "01020304", bytes).unwrap();
                self.tools.geometry = Geometry::ClassicMini;
                self.tools.image_name = "Demo Classic Mini".into();
                self.bench.geometry = d.geometry;
                self.bench.uid = d.uid.clone();
                self.bench.editor = command::encode(&d.bytes);
                self.bench.dump = Some(d.clone());
                self.bench.origin_simulated = Some(true);
                match emulator::save_image(
                    &mut self.settings.emulator_images,
                    emulator::Image {
                        name: self.tools.image_name.clone(),
                        dump: d,
                        simulated: true,
                    },
                ) {
                    Ok(()) => {
                        self.save();
                        self.tools.message =
                            "Synthetic PC image saved as SIMULATED. No device operation.".into();
                    }
                    Err(e) => self.tools.message = e,
                }
            }
            if ui
                .add_enabled(
                    !self.settings.rf_may_be_active
                        && emulator::require_capabilities(
                            &emulator::Action::Read(self.tools.geometry),
                            &self.caps,
                        )
                        .is_ok(),
                    egui::Button::new("Read current buffer"),
                )
                .clicked()
            {
                self.tools.pending_geometry = Some(self.tools.geometry);
                self.start(
                    Job::Emulator(emulator::Request {
                        action: emulator::Action::Read(self.tools.geometry),
                        caps: self.caps.clone(),
                        simulated: self.simulation.is_some(),
                        acknowledged: false,
                    }),
                    "Read emulator buffer",
                );
            }
        });
        if let Some((g, bytes)) = &self.tools.buffer {
            ui.label(format!(
                "Last complete buffer: {g:?}, {} bytes. Snapshot only; simulation rechecks it.",
                bytes.len()
            ));
            if ui.button("Save buffer as PC image").clicked() {
                let r = Dump::new(*g, &self.tools.image_uid, bytes.clone());
                self.add_emulator_image(r);
            }
        }
        ui.checkbox(
            &mut self.tools.remove_ack,
            "Allow removing a PC image from this library",
        );
        for (i, image) in self
            .settings
            .emulator_images
            .clone()
            .into_iter()
            .enumerate()
        {
            ui.push_id(i,|ui|{Self::card(ui,|ui|{
                ui.label(RichText::new(format!("{} · {:?} · {} bytes{}",image.name,image.dump.geometry,image.dump.bytes.len(),if image.simulated{" · SIMULATED"}else{""})).strong());
                let valid=image.validate();if let Err(e)=&valid{ui.colored_label(ui.visuals().warn_fg_color,e);}
                ui.horizontal_wrapped(|ui|{
                    if ui.button("Inspect draft").clicked(){self.bench.geometry=image.dump.geometry;self.bench.uid=image.dump.uid.clone();self.bench.editor=command::encode(&image.dump.bytes);self.bench.dump=Some(image.dump.clone());self.bench.origin_simulated=Some(image.simulated);self.bench.draft_history.reset();self.page=Page::Workbench;}
                    if ui.button("Export image…").clicked(){if let Some(p)=rfd::FileDialog::new().set_file_name("classic-image.json").save_file(){self.tools.message=valid.clone().and_then(|()|serde_json::to_vec_pretty(&image).map_err(|e|e.to_string())).and_then(|b|storage::write_private(&p,&b)).map(|()|"Image exported with its provenance".into()).unwrap_or_else(|e|e);}}
                    let ready=valid.is_ok()&&!self.settings.rf_may_be_active&&image.simulated==self.simulation.is_some();
                    if ui.add_enabled(ready && emulator::require_capabilities(&emulator::Action::Load(image.clone()), &self.caps).is_ok(),egui::Button::new("Load buffer & verify…")).clicked(){self.tools.pending_geometry=Some(image.dump.geometry);self.emulator_request(emulator::Action::Load(image.clone()),"Replace the current device emulator buffer with this PC image. Every byte will be read back and compared. This does not programme a physical card.");}
                    if ui.add_enabled(ready && emulator::require_capabilities(&emulator::Action::Simulate(image.clone()), &self.caps).is_ok(),egui::Button::new("Review RF simulation…")).clicked(){self.tools.pending_geometry=Some(image.dump.geometry);self.emulator_request(emulator::Action::Simulate(image.clone()),"Re-read and compare the complete current buffer, then request Classic RF simulation for your test reader. No reader-attack flags are used. RF may continue after app exit; Stop or the physical device button is required. Reader acceptance is not verified.");}
                    if ui.add_enabled(self.tools.remove_ack,egui::Button::new("Remove PC image")).clicked(){self.settings.emulator_images.remove(i);self.tools.remove_ack=false;self.save();}
                });
            });});
        }
    }
    fn add_emulator_image(&mut self, result: Result<Dump, String>) {
        self.tools.message = result
            .and_then(|dump| {
                emulator::save_image(
                    &mut self.settings.emulator_images,
                    emulator::Image {
                        name: self.tools.image_name.trim().into(),
                        dump,
                        simulated: self.simulation.is_some(),
                    },
                )
            })
            .map(|()| "PC image saved; no device operation".into())
            .unwrap_or_else(|e| e);
        self.save();
    }
    fn diagnostics_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Reader diagnostics");
        ui.label("Check the device, USB response and antenna measurements. These operations do not write card memory. Antenna readings depend on the board and surroundings.");
        ui.horizontal_wrapped(|ui| {
            for (label, cmd) in [
                ("Version & compatibility", "hw version"),
                ("USB response", "hw ping"),
                ("Device status", "hw status"),
                ("Measure antennas", "hw tune"),
            ] {
                if ui
                    .add_enabled(self.caps.has(cmd), egui::Button::new(label))
                    .clicked()
                {
                    self.run_guided(
                        Ok(CommandSpec::simple(cmd, Kind::Device)),
                        "Reader diagnostics",
                    );
                }
            }
        });
        if !self.inspected {
            ui.label("Check the connection in Device setup first.");
        }
        self.read_result(ui);
        ui.label("Detailed output is available in Logs. A completed diagnostic is not an RF card-write test.");
        if ui.button("Open operation log").clicked() {
            self.show_logs = true;
        }
    }
    pub(super) fn start_signal(&mut self, saved: bool) {
        let request = proxmark_studio::acquisition::Request {
            samples: self.tools.capture_samples,
            demod: self.tools.demod,
            caps: self.caps.clone(),
            simulated: self.simulation.is_some(),
            saved: if saved {
                self.tools.signal.clone()
            } else {
                None
            },
        };
        self.start(
            Job::Signal(request),
            if saved {
                "Analyse saved signal"
            } else {
                "Capture live signal"
            },
        );
    }
    fn signal_page(&mut self, ui: &mut egui::Ui) {
        use proxmark_studio::acquisition::Demod;
        ui.heading("Live signal analysis");
        ui.label("Capture LF waveforms from the selected reader, or analyse saved RRG captures. Sampling uses the reader's current LF configuration. No sample rate is assumed. Repeated capture runs once per second and stops on failure or cancellation.");
        ui.horizontal_wrapped(|ui| {
            ui.label("Samples");
            ui.add(egui::DragValue::new(&mut self.tools.capture_samples).range(512..=60000));
            egui::ComboBox::from_label("Demodulation")
                .selected_text(format!("{:?}", self.tools.demod))
                .show_ui(ui, |ui| {
                    for d in Demod::ALL {
                        ui.selectable_value(&mut self.tools.demod, d, format!("{d:?}"));
                    }
                });
        });
        ui.horizontal_wrapped(|ui| {
            let ready = self.available("lf read")
                && self.caps.has("data save")
                && !self.settings.rf_may_be_active;
            if ui
                .add_enabled(ready, egui::Button::new("Capture waveform"))
                .clicked()
            {
                self.tools.live = false;
                self.start_signal(false);
            }
            if ui
                .add_enabled(
                    ready && !self.tools.live,
                    egui::Button::new("Start live capture"),
                )
                .clicked()
            {
                self.tools.live = true;
                self.start_signal(false);
            }
            if ui
                .add_enabled(self.tools.live, egui::Button::new("Stop live capture"))
                .clicked()
            {
                self.tools.live = false;
            }
            if ui
                .add_enabled(
                    self.tools.signal.is_some()
                        && self.tools.demod != Demod::None
                        && self.caps.has("data rawdemod"),
                    egui::Button::new("Analyse current waveform"),
                )
                .clicked()
            {
                self.tools.live = false;
                self.start_signal(true);
            }
        });
        if let Some(c) = &self.tools.capture {
            if let Some(e) = &c.analysis_error {
                ui.colored_label(ui.visuals().warn_fg_color, e);
            }
            for line in &c.decoding {
                ui.label(line);
            }
            if !c.bits.is_empty() {
                egui::CollapsingHeader::new(format!("Demodulated bits · {} bits", c.bits.len()))
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&c.bits).monospace()).wrap());
                    });
            }
        }

        ui.label("Open an RRG text capture saved with data save. One sample per line, -127 to 127. Plotting does not infer a credential or sample rate.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Open signal capture…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("RRG signal text", &["pm3", "txt"])
                    .pick_file()
                {
                    match storage::read_bounded(&p, 8 * 1024 * 1024)
                        .and_then(|b| {
                            String::from_utf8(b).map_err(|_| "Signal must be UTF-8 text".into())
                        })
                        .and_then(|s| Signal::parse(&s))
                    {
                        Ok(signal) => {
                            self.tools.live = false;
                            self.tools.capture = None;
                            self.tools.end = signal.samples.len();
                            self.tools.start = 0;
                            self.tools.signal = Some(signal);
                            self.tools.signal_label = p.display().to_string();
                        }
                        Err(e) => self.tools.message = e,
                    }
                }
            }
            if ui.button("Show synthetic signal").clicked() {
                self.tools.live = false;
                self.tools.capture = None;
                self.tools.signal = Some(Signal {
                    samples: (0..2048)
                        .map(|i| if i % 32 < 16 { 90 } else { -90 })
                        .collect(),
                });
                self.tools.start = 0;
                self.tools.end = 2048;
                self.tools.signal_label = "SIMULATED square wave".into();
            }
        });
        if let Some(signal) = &self.tools.signal {
            ui.label(&self.tools.signal_label);
            let min = signal.samples.iter().min().unwrap_or(&0);
            let max = signal.samples.iter().max().unwrap_or(&0);
            ui.label(format!(
                "Amplitude range: {min} to {max}. Sample positions have no inferred time scale."
            ));
            ui.horizontal_wrapped(|ui| {
                ui.label("Inspect sample");
                ui.add(
                    egui::DragValue::new(&mut self.tools.inspect_sample)
                        .range(0..=signal.samples.len() - 1),
                );
                self.tools.inspect_sample = self.tools.inspect_sample.min(signal.samples.len() - 1);
                ui.label(format!(
                    "Value: {}",
                    signal.samples[self.tools.inspect_sample]
                ));
                if ui.button("Export waveform…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("waveform.pm3")
                        .save_file()
                    {
                        self.tools.message = storage::write_private(
                            &path,
                            proxmark_studio::acquisition::text(signal).as_bytes(),
                        )
                        .map(|()| "Waveform exported".into())
                        .unwrap_or_else(|e| e);
                    }
                }
            });

            ui.label(format!("{} samples", signal.samples.len()));
            ui.horizontal(|ui| {
                ui.label("First sample");
                ui.add(
                    egui::DragValue::new(&mut self.tools.start).range(0..=signal.samples.len() - 2),
                );
                ui.label("End (exclusive)");
                ui.add(
                    egui::DragValue::new(&mut self.tools.end)
                        .range(self.tools.start + 1..=signal.samples.len()),
                );
                if ui.button("Show all").clicked() {
                    self.tools.start = 0;
                    self.tools.end = signal.samples.len();
                }
            });
            self.tools.end = self
                .tools
                .end
                .clamp(self.tools.start + 1, signal.samples.len());
            let (rect, response) = ui
                .allocate_exact_size(Vec2::new(ui.available_width(), 230.0), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
            let y = |v: i16| rect.center().y - v as f32 / 127.0 * (rect.height() / 2.0 - 10.0);
            painter.line_segment(
                [
                    egui::pos2(rect.left(), y(0)),
                    egui::pos2(rect.right(), y(0)),
                ],
                egui::Stroke::new(1.0_f32, appearance::secondary(ui)),
            );
            if let Ok(points) = signal.envelope(
                self.tools.start,
                self.tools.end,
                rect.width().max(1.0) as usize,
            ) {
                let n = points.len();
                for (i, (lo, hi)) in points.into_iter().enumerate() {
                    let x = rect.left() + i as f32 / n as f32 * rect.width();
                    painter.line_segment(
                        [egui::pos2(x, y(lo)), egui::pos2(x, y(hi) + 0.7)],
                        egui::Stroke::new(1.0_f32, ui.visuals().selection.stroke.color),
                    );
                }
            }
            if let Some(p) = response.hover_pos() {
                let n = (self.tools.start
                    + ((p.x - rect.left()) / rect.width()
                        * (self.tools.end - self.tools.start) as f32)
                        as usize)
                    .min(self.tools.end - 1);
                response.on_hover_text(format!("Sample {n}: {}", signal.samples[n]));
            }
        }
    }
}
fn is_secret(flag: &str, description: &str) -> bool {
    let text = format!("{flag} {description}").to_lowercase();
    flag == "-k"
        || ["key", "password", "secret", "pwd"]
            .iter()
            .any(|s| text.contains(s))
}
