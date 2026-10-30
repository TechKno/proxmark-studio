use super::*;
use basic::{Action, Card, Content, Destination, Hint, Pending, Route, SavedCard, StepView};

pub(super) struct BasicView {
    pub route: Route,
    pub flow: basic::Workflow,
    pub stage: String,
    pub connect_after_probe: bool,
    hint: Hint,
    save_name: String,
    filter: String,
    protocol: usize,
    format: Format,
    fc: String,
    number: String,
    em: String,
    em_known_facility: String,
    version: String,
    demo_card: String,
    active_record: Option<u64>,
}
impl Default for BasicView {
    fn default() -> Self {
        Self {
            route: Route::Home,
            flow: Default::default(),
            stage: String::new(),
            connect_after_probe: false,
            hint: Hint::Automatic,
            save_name: String::new(),
            filter: String::new(),
            protocol: 0,
            format: Format::H10301,
            fc: String::new(),
            number: String::new(),
            em: String::new(),
            em_known_facility: String::new(),
            version: "1".into(),
            demo_card: "em".into(),
            active_record: None,
        }
    }
}
impl Studio {
    pub(super) fn persist_ui(&mut self) -> Result<(), String> {
        let mut saved = self
            .simulation
            .as_ref()
            .map(|s| s.settings_to_save(&self.settings))
            .unwrap_or_else(|| self.settings.clone());
        if self.settings.sealed_library.is_some() {
            if self.library_passphrase.is_empty() {
                if !saved.cards.is_empty() || !saved.emulator_images.is_empty() {
                    return Err("Unlock the saved libraries before changing them".into());
                }
            } else {
                let library = proxmark_studio::sealed::Library {
                    cards: saved.cards.clone(),
                    images: saved.emulator_images.clone(),
                };
                let bytes =
                    Zeroizing::new(serde_json::to_vec(&library).map_err(|e| e.to_string())?);
                let sealed = proxmark_studio::sealed::seal(&bytes, &self.library_passphrase)?;
                saved.sealed_library = Some(sealed.clone());
                self.settings.sealed_library = Some(sealed);
            }
            saved.cards.clear();
            saved.emulator_images.clear();
        }
        storage::save(&self.path, &saved)
    }

    fn start_basic(&mut self, action: Action, pending: Pending, label: &str) {
        if self.worker.is_some() || self.terminal.is_some() || self.maintenance.busy {
            self.fail("Finish the current operation first");
            return;
        }
        if !self.inspected || self.settings.port.is_empty() {
            self.fail("Open Device setup and check the connection first");
            return;
        }
        if pending == Pending::Write {
            let at = basic::now();
            self.settings.operations.push(basic::Record {
                at,
                action: label.into(),
                outcome: "Running".into(),
                detail: self
                    .basic
                    .flow
                    .source
                    .as_ref()
                    .map(|c| format!("{} · {}", c.label(), c.scope()))
                    .unwrap_or_default(),
                simulated: self.simulation.is_some(),
            });
            if self.settings.operations.len() > 200 {
                self.settings.operations.remove(0);
            }
            if let Err(e) = self.persist_ui() {
                self.settings.operations.pop();
                self.fail(format!("Cannot record operation before writing: {e}"));
                return;
            }
            self.basic.active_record = Some(at);
        }
        self.basic.flow.pending = Some(pending);
        self.basic.stage = label.into();
        self.start(
            Job::Guided(Box::new(basic::Request {
                action,
                caps: self.caps.clone(),
                simulated: self.simulation.is_some(),
            })),
            label,
        );
    }
    pub(super) fn finish_basic(&mut self, r: &proxmark_studio::runner::JobResult) {
        let pending = self.basic.flow.pending;
        if pending.is_none() {
            return;
        }
        self.basic.flow.receive(&r.outcome, &r.evidence);
        let outcome = match &r.outcome {
            Outcome::Completed => "Completed",
            Outcome::Verified => "Verified",
            Outcome::Mismatch => "Verification failed",
            Outcome::Cancelled => "Cancelled / unknown",
            Outcome::TimedOut => "Timed out / unknown",
            Outcome::Failed(_) => "Failed",
        };
        if let Some(at) = self.basic.active_record.take() {
            if let Some(record) = self
                .settings
                .operations
                .iter_mut()
                .rev()
                .find(|r| r.at == at && r.outcome == "Running")
            {
                record.outcome = outcome.into();
                if let Outcome::Failed(e) = &r.outcome {
                    record.detail = e.clone();
                }
            }
        } else {
            self.settings.operations.push(basic::Record {
                at: basic::now(),
                action: match pending {
                    Some(Pending::Read) => "Read source",
                    _ => "Check destination",
                }
                .into(),
                outcome: outcome.into(),
                detail: if let Outcome::Failed(e) = &r.outcome {
                    e.clone()
                } else {
                    self.basic.stage.clone()
                },
                simulated: self.simulation.is_some(),
            });
            if self.settings.operations.len() > 200 {
                self.settings.operations.remove(0);
            }
        }
        if let Err(e) = self.persist_ui() {
            self.push(format!("History was not saved: {e}"));
        }
        self.basic.stage.clear();
    }
    pub(super) fn demo_controls(&mut self, ui: &mut egui::Ui) {
        if self.simulation.is_none() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "DEMO · no physical device access",
            );
            egui::ComboBox::from_id_salt("demo_card")
                .selected_text(match self.basic.demo_card.as_str() {
                    "em" => "EM410x source",
                    "na1" => "Organisation example fob",
                    "hid" => "HID source",
                    "ntag" => "NTAG source",
                    "ntag_blank" => "Blank NTAG",
                    "desfire" => "DESFire card",
                    "classic" => "Classic source",
                    "magic1" => "Blank Gen 1a",
                    "magic2" => "Blank Gen 2",
                    "t5577" => "Blank T5577",
                    "locked" => "Locked destination",
                    "none" => "No card",
                    _ => "Write mismatch",
                })
                .show_ui(ui, |ui| {
                    for (id, name) in [
                        ("em", "EM410x source"),
                        ("na1", "Organisation example fob"),
                        ("hid", "HID source"),
                        ("ntag", "NTAG source"),
                        ("ntag_blank", "Blank NTAG"),
                        ("desfire", "DESFire card"),
                        ("classic", "Classic source"),
                        ("magic1", "Blank Gen 1a"),
                        ("magic2", "Blank Gen 2"),
                        ("t5577", "Blank T5577"),
                        ("locked", "Locked destination"),
                        ("none", "No card"),
                        ("ignore", "Write mismatch"),
                    ] {
                        if ui
                            .selectable_value(&mut self.basic.demo_card, id.into(), name)
                            .clicked()
                        {
                            ui.close_menu();
                        }
                    }
                });
            if ui.button("Place demo card").clicked() {
                if let Err(e) = self.place_demo_card() {
                    self.fail(e);
                }
            }
        });
    }
    fn place_demo_card(&mut self) -> Result<(), String> {
        let Some(session) = &self.simulation else {
            return Err("Demo is not active".into());
        };
        let dir = session.directory.path();
        for name in [
            ".mock-pm3-locked",
            ".mock-pm3-ignore-writes",
            ".mock-pm3-raw",
            ".mock-pm3-em",
            ".mock-pm3-hid.json",
            ".mock-pm3-memory.bin",
            ".mock-pm3-classic.bin",
        ] {
            let _ = std::fs::remove_file(dir.join(name));
        }
        let card = self.basic.demo_card.as_str();
        std::fs::write(dir.join(".mock-pm3-card"), card).map_err(|e| e.to_string())?;
        if card == "locked" {
            std::fs::write(dir.join(".mock-pm3-locked"), b"").map_err(|e| e.to_string())?;
        }
        if card == "ignore" {
            std::fs::write(dir.join(".mock-pm3-ignore-writes"), b"").map_err(|e| e.to_string())?;
        }
        if card == "ntag_blank" {
            std::fs::write(dir.join(".mock-pm3-memory.bin"), vec![0; 4096])
                .map_err(|e| e.to_string())?;
        }
        self.basic.flow.invalidate();
        if self.basic.flow.step != StepView::Source {
            self.basic.flow.step = StepView::Target;
        }
        self.status = format!("Demo card placed: {card}. Source snapshot retained.");
        self.status_ok = true;
        Ok(())
    }
    pub(super) fn basic_page(&mut self, ui: &mut egui::Ui) {
        match self.basic.route {
            Route::Home => self.basic_home(ui),
            Route::Read | Route::Clone | Route::Write => self.basic_workflow(ui),
            Route::Library => self.basic_library(ui),
            Route::History => self.basic_history(ui),
            Route::Setup => self.basic_setup(ui),
        }
    }
    fn basic_home(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(RichText::new("Start with a card.").size(30.0));
        ui.add_space(8.0);
        ui.label(
            RichText::new("Read what’s there. Make a copy. Write something new.")
                .size(14.0)
                .color(appearance::secondary(ui)),
        );
        ui.add_space(32.0);
        ui.columns(3, |columns| {
            let actions = [
                (
                    Route::Read,
                    "Read",
                    "Identify a card and decode its contents.",
                    "Read a card",
                ),
                (
                    Route::Clone,
                    "Clone",
                    "Copy a saved or freshly read card to a compatible blank.",
                    "Clone a card",
                ),
                (
                    Route::Write,
                    "Write",
                    "Use a saved credential or enter a facility and card number.",
                    "Write a card",
                ),
            ];
            let tile_height = actions
                .iter()
                .map(|(_, name, body, _)| design::task_tile_height(&columns[0], name, body))
                .fold(226.0_f32, f32::max);
            for (index, (route, name, body, cta)) in actions.into_iter().enumerate() {
                if design::task_tile(&mut columns[index], name, body, cta, index, tile_height) {
                    self.basic.route = route;
                    self.basic.flow.step = StepView::Source;
                    self.basic.flow.invalidate();
                }
            }
        });
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), egui::Sense::hover());
            design::icon(
                ui.painter(),
                rect.shrink(5.0),
                "Device setup",
                appearance::secondary(ui),
            );
            let detail_width = (ui.available_width() - 180.0).max(120.0);
            ui.vertical(|ui| {
                ui.set_max_width(detail_width);
                ui.label(if self.simulation.is_some() {
                    "Demo reader connected"
                } else if self.device_seen {
                    "Reader ready"
                } else {
                    "Connect your Proxmark3"
                });
                ui.label(
                    RichText::new(if self.device_seen {
                        "Place one card on the appropriate antenna."
                    } else {
                        "Choose your reader to get started, or explore with the demo."
                    })
                    .small()
                    .color(appearance::secondary(ui)),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(if self.device_seen {
                        "Reader settings"
                    } else {
                        "Set up reader"
                    })
                    .clicked()
                {
                    self.basic.route = Route::Setup;
                }
            });
        });
        ui.add_space(28.0);
        ui.separator();
        ui.add_space(20.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Your library").size(16.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.link("View library →").clicked() {
                    self.basic.route = Route::Library;
                }
            });
        });
        ui.add_space(8.0);
        if self.settings.sealed_library.is_some() && self.library_passphrase.is_empty() {
            ui.label(
                RichText::new("Unlock your saved cards in Settings.")
                    .color(appearance::secondary(ui)),
            );
        } else if self.settings.cards.is_empty() {
            ui.label(
                RichText::new("Save a card after reading it. It will be ready here next time.")
                    .color(appearance::secondary(ui)),
            );
        } else {
            for saved in self
                .settings
                .cards
                .iter()
                .rev()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
            {
                if design::tool_row(ui, &saved.name, &saved.card.summary(), "Read") {
                    self.basic.flow.set_source(saved.card);
                    self.basic.route = Route::Read;
                }
            }
        }
        ui.add_space(24.0);
        ui.label(
            RichText::new(
                "Protected Classic and DESFire cards have specialist workflows in Advanced.",
            )
            .size(12.0)
            .color(appearance::secondary(ui)),
        );
    }
    pub(super) fn text_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
        let l = ui.label(label);
        ui.add(egui::TextEdit::singleline(value).desired_width(300.0))
            .labelled_by(l.id);
    }
    fn basic_draft(&self) -> Result<Card, String> {
        let content = match self.basic.protocol {
            0 => {
                let c = Credential::from_decimal(
                    self.basic.format,
                    if self.basic.format.facility_max() == 0 {
                        "0"
                    } else {
                        &self.basic.fc
                    },
                    &self.basic.number,
                )?;
                Content::Hid {
                    raw: c.encode()?.hid_raw,
                    credential: Some(c),
                }
            }
            1 => Content::Em410x(command::hex(&self.basic.em, 5, "EM410x ID")?),
            index => {
                let family = [
                    proxmark_studio::lf::Family::Awid26,
                    proxmark_studio::lf::Family::Awid34,
                    proxmark_studio::lf::Family::Awid37,
                    proxmark_studio::lf::Family::Awid50,
                    proxmark_studio::lf::Family::IoProx,
                ][index - 2];
                let c = proxmark_studio::lf::Credential {
                    name: String::new(),
                    family,
                    facility: self
                        .basic
                        .fc
                        .parse()
                        .map_err(|_| "Enter a decimal facility code")?,
                    number: self
                        .basic
                        .number
                        .parse()
                        .map_err(|_| "Enter a decimal card number")?,
                    version: if index == 6 {
                        self.basic
                            .version
                            .parse()
                            .map_err(|_| "Enter a version from 0 to 255")?
                    } else {
                        0
                    },
                    target: Target::T5577,
                };
                c.write()?;
                Content::Lf(c)
            }
        };
        Ok(Card {
            content,
            captured_at: basic::now(),
            simulated: self.simulation.is_some(),
        })
    }
    fn basic_write_form(&mut self, ui: &mut egui::Ui) {
        Self::card(ui, |ui| {
            ui.heading("Create a credential");
            let protocols = [
                "HID Prox",
                "EM410x",
                "AWID 26-bit",
                "AWID 34-bit",
                "AWID 37-bit",
                "AWID 50-bit",
                "ioProx",
            ];
            ui.label("Credential type");
            egui::ComboBox::from_id_salt("basic_protocol")
                .selected_text(protocols[self.basic.protocol])
                .show_ui(ui, |ui| {
                    for (i, name) in protocols.iter().enumerate() {
                        ui.selectable_value(&mut self.basic.protocol, i, *name);
                    }
                });
            if self.basic.protocol == 0 {
                ui.label("Wiegand format");
                egui::ComboBox::from_id_salt("basic_format")
                    .selected_text(format!(
                        "{} · {} bits",
                        self.basic.format.name(),
                        self.basic.format.bits()
                    ))
                    .show_ui(ui, |ui| {
                        for f in Format::ALL {
                            ui.selectable_value(
                                &mut self.basic.format,
                                f,
                                format!("{} · {} bits", f.name(), f.bits()),
                            );
                        }
                    });
            }
            if self.basic.protocol == 1 {
                Self::text_field(
                    ui,
                    "EM410x ID · 10 hexadecimal characters",
                    &mut self.basic.em,
                );
            } else {
                if self.basic.protocol != 0 || self.basic.format.facility_max() != 0 {
                    Self::text_field(ui, "Facility code · decimal", &mut self.basic.fc);
                }
                Self::text_field(ui, "Card number · decimal", &mut self.basic.number);
                if self.basic.protocol == 6 {
                    Self::text_field(ui, "ioProx version · 0–255", &mut self.basic.version);
                }
                if self.basic.protocol == 0 {
                    ui.label(
                        RichText::new(format!(
                            "Maximum facility {} · card {}",
                            self.basic.format.facility_max(),
                            self.basic.format.card_max()
                        ))
                        .small()
                        .color(appearance::secondary(ui)),
                    );
                }
            }
            let draft = self.basic_draft();
            if let Err(e) = &draft {
                ui.colored_label(ui.visuals().warn_fg_color, e);
            }
            if Self::action(ui, draft.is_ok(), "Use this credential") {
                self.basic.flow.set_source(draft.as_ref().unwrap().clone());
                self.basic.flow.step = StepView::Target;
            }
            ui.collapsing("Save credential profile", |ui| {
                Self::text_field(ui, "Profile name", &mut self.basic.save_name);
                if Self::action(
                    ui,
                    draft.is_ok() && !self.basic.save_name.trim().is_empty(),
                    "Save credential to Library",
                ) {
                    self.save_basic_card(draft.as_ref().unwrap().clone());
                }
            });
            if ui.link("Choose a saved item instead").clicked() {
                self.basic.route = Route::Library;
            }
        });
    }
    fn basic_workflow(&mut self, ui: &mut egui::Ui) {
        let route = self.basic.route;
        Self::title(ui,"BASIC",match route{Route::Read=>"Read a card",Route::Clone=>"Clone a card",_=>"Write a card"},match route{Route::Read=>"A read-only operation. Supported contents can be saved to your Library.",Route::Clone=>"Source and destination are separate. Remove the source before checking the blank.",_=>"Choose the data, check the destination, then review and write."});
        if route != Route::Read {
            ui.horizontal_wrapped(|ui| {
                for (step, name) in [
                    (StepView::Source, "1  Source"),
                    (StepView::Target, "2  Destination"),
                    (StepView::Review, "3  Review"),
                    (StepView::Result, "4  Result"),
                ] {
                    let current = self.basic.flow.step == step;
                    egui::Frame::new()
                        .fill(if current {
                            ui.visuals().selection.bg_fill
                        } else {
                            Color32::TRANSPARENT
                        })
                        .corner_radius(7)
                        .inner_margin(egui::Margin::symmetric(12, 7))
                        .show(ui, |ui| {
                            ui.label(RichText::new(name).color(if current {
                                ui.visuals().text_color()
                            } else {
                                appearance::secondary(ui)
                            }));
                        });
                }
            });
            ui.add_space(12.0);
        }
        if self.worker.is_some() {
            Self::card(ui, |ui| {
                ui.spinner();
                ui.heading(&self.basic.stage);
                ui.label(
                    "Keep the current card in position. Stopping cannot undo writes already sent.",
                );
            });
        }
        if route == Route::Write && self.basic.flow.step == StepView::Source {
            self.basic_write_form(ui);
        }
        if (route == Route::Read || route == Route::Clone)
            && self.basic.flow.step == StepView::Source
        {
            Self::card(ui, |ui| {
                ui.heading("Place the source card on the antenna");
                ui.label("Keep just one card in range.");
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("read_hint")
                        .selected_text(match self.basic.hint {
                            Hint::Automatic => "Detect automatically",
                            Hint::Lf => "125 kHz only",
                            Hint::Hf => "13.56 MHz only",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.basic.hint,
                                Hint::Automatic,
                                "Detect automatically",
                            );
                            ui.selectable_value(&mut self.basic.hint, Hint::Lf, "125 kHz only");
                            ui.selectable_value(&mut self.basic.hint, Hint::Hf, "13.56 MHz only");
                        });
                    if Self::action(ui, self.inspected, "Read source") {
                        self.basic.flow.invalidate();
                        self.start_basic(
                            Action::Read(self.basic.hint),
                            Pending::Read,
                            "Read source",
                        );
                    }
                    if !self.inspected && ui.link("Set up reader").clicked() {
                        self.basic.route = Route::Setup;
                    }
                });
            });
        }
        ui.add_space(12.0);
        if let Some(card) = self
            .basic
            .flow
            .source
            .clone()
            .filter(|_| route != Route::Write || self.basic.flow.step != StepView::Source)
        {
            Self::card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.heading(card.label());
                    if card.simulated {
                        ui.colored_label(ui.visuals().warn_fg_color, "SIMULATED");
                    }
                });
                if card.em_identity().is_some() {
                    self.em_decoding(ui, &card);
                } else {
                    ui.label(card.summary());
                }
                ui.label(
                    RichText::new(format!(
                        "Snapshot created {}",
                        format_time(card.captured_at)
                    ))
                    .small()
                    .color(appearance::secondary(ui)),
                );
                if let Content::Hid {
                    raw,
                    credential: None,
                } = &card.content
                {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "More than one format can describe these bits. Select the intended format.",
                    );
                    for d in credential::candidates(raw, false) {
                        let name = format!(
                            "{} · facility {} · card {}",
                            d.credential.format.name(),
                            d.credential.facility_code,
                            d.credential.card_number
                        );
                        if ui.button(name).clicked() {
                            let mut selected = card.clone();
                            if selected.select_format(d.credential.format).is_ok() {
                                self.basic.flow.set_source(selected);
                            }
                        }
                    }
                }
                if let Err(e) = card.writable() {
                    ui.colored_label(ui.visuals().warn_fg_color, e);
                    if ui.button("Open specialist tools").clicked() {
                        self.settings.ui_mode = Mode::Advanced;
                        self.page = if matches!(card.content,Content::Identified{ref family,..} if family.contains("DESFire"))
                        {
                            Page::Desfire
                        } else if matches!(card.content,Content::Identified{ref family,..} if family.contains("Classic"))
                        {
                            self.classic.tab = 0;
                            Page::Classic
                        } else {
                            Page::Workbench
                        };
                    }
                }
                if self.basic.flow.step == StepView::Source {
                    ui.horizontal_wrapped(|ui| {
                        if Self::action(ui, card.writable().is_ok(), "Continue to destination") {
                            self.basic.route = Route::Clone;
                            self.basic.flow.step = StepView::Target;
                            self.basic.flow.invalidate();
                        }
                        Self::text_field(ui, "Library name", &mut self.basic.save_name);
                        if Self::action(
                            ui,
                            !self.basic.save_name.trim().is_empty(),
                            "Save to Library",
                        ) {
                            self.save_basic_card(card.clone());
                        }
                    });
                }
            });
            ui.add_space(12.0);
            match self.basic.flow.step {
                StepView::Target => Self::card(ui, |ui| {
                    ui.heading("Remove the source. Place a compatible destination.");
                    ui.label(match card.content{Content::Memory(_)=>"Use the same NTAG model. Only unlocked user-data pages are copied; the destination keeps its own UID.",_=>"Use a password-free T5577 or supported Q5 blank. The check reads its configuration and does not write."});
                    if Self::action(
                        ui,
                        card.writable().is_ok() && self.inspected,
                        "Check destination",
                    ) {
                        self.basic.flow.destination = None;
                        self.start_basic(
                            Action::Check(card.clone()),
                            Pending::Check,
                            "Check destination",
                        );
                    }
                    if ui.link("Back to source").clicked() {
                        self.basic.flow.step = StepView::Source;
                        self.basic.flow.invalidate();
                    }
                }),
                StepView::Review => Self::card(ui, |ui| {
                    ui.heading("Review before writing");
                    if let Some(dest) = self.basic.flow.destination.clone() {
                        ui.label(match &dest {
                            Destination::T55 { target, block0 } => {
                                format!("Destination: {} · configuration {}", target.name(), block0)
                            }
                            Destination::Memory { geometry, uid } => {
                                format!("Destination: {} · UID {}", geometry.model(), uid)
                            }
                        });
                        ui.label(card.scope());
                        ui.colored_label(ui.visuals().warn_fg_color,"This overwrites the supported data on the destination. Keep it in place through verification.");
                        ui.checkbox(
                            &mut self.basic.flow.acknowledged,
                            "I have checked the destination and want to overwrite it.",
                        );
                        if Self::action(ui, self.basic.flow.acknowledged, "Write & verify") {
                            self.start_basic(
                                Action::Write {
                                    card: card.clone(),
                                    destination: dest,
                                    acknowledged: true,
                                },
                                Pending::Write,
                                if route == Route::Clone {
                                    "Clone & verify"
                                } else {
                                    "Write & verify"
                                },
                            );
                        }
                    }
                    if ui.link("Check another destination").clicked() {
                        self.basic.flow.invalidate();
                        self.basic.flow.step = StepView::Target;
                    }
                }),
                StepView::Result => Self::card(ui, |ui| {
                    let verified = self.basic.flow.result == Some(Outcome::Verified);
                    ui.colored_label(
                        if verified {
                            ui.visuals().selection.stroke.color
                        } else {
                            ui.visuals().warn_fg_color
                        },
                        RichText::new(if verified {
                            "Verified by a fresh read"
                        } else {
                            "Not verified"
                        })
                        .size(23.0)
                        .strong(),
                    );
                    ui.label(if verified {
                        card.scope()
                    } else {
                        self.status.clone()
                    });
                    if !verified {
                        ui.label("A partial write may have occurred. Read or check the destination before any retry.");
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Check next destination").clicked() {
                            self.basic.flow.invalidate();
                            self.basic.flow.step = StepView::Target;
                        }
                        if ui.button("View history").clicked() {
                            self.basic.route = Route::History;
                        }
                        if ui.button("View details").clicked() {
                            self.show_logs = true;
                        }
                    });
                }),
                StepView::Source => {}
            }
        }
    }
    fn save_basic_card(&mut self, card: Card) {
        let item = SavedCard {
            name: self.basic.save_name.trim().into(),
            card,
        };
        let mut cards = self.settings.cards.clone();
        cards.push(item);
        if let Err(e) = basic::validate_library(&cards) {
            self.fail(e);
            return;
        }
        let previous = std::mem::replace(&mut self.settings.cards, cards);
        match self.persist_ui() {
            Ok(()) => {
                self.status =
                    "Saved to Library. Card data is stored privately on this computer.".into();
                self.status_ok = true;
            }
            Err(e) => {
                self.settings.cards = previous;
                self.fail(e);
            }
        }
    }
    fn basic_library(&mut self, ui: &mut egui::Ui) {
        Self::title(ui,"LIBRARY","Saved cards and credentials","Load an item to review and write. Demo captures stay labelled and cannot be used for physical writes.");
        ui.horizontal_wrapped(|ui| {
            Self::text_field(ui,"Find saved items",&mut self.basic.filter);
            if ui.button("Import…").clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("Studio library or dump",&["json"]).pick_file(){let imported=(||->Result<Vec<SavedCard>,String>{let bytes=storage::read_bounded(&path,4*1024*1024)?;let cards=if let Ok(dump)=serde_json::from_slice::<proxmark_studio::backup::Dump>(&bytes){dump.validate()?;vec![SavedCard{name:path.file_stem().unwrap_or_default().to_string_lossy().into(),card:Card{content:Content::Memory(dump),captured_at:basic::now(),simulated:self.simulation.is_some()}}]}else{serde_json::from_slice(&bytes).map_err(|e|format!("Invalid library: {e}"))?};basic::validate_library(&cards)?;Ok(cards)})();match imported{Ok(mut items)=>{let mut all=self.settings.cards.clone();all.append(&mut items);match basic::validate_library(&all){Ok(())=>{self.settings.cards=all;if let Err(e)=self.persist_ui(){self.fail(e);}},Err(e)=>self.fail(e)}},Err(e)=>self.fail(e)}}}
            if ui.button("Export library…").clicked(){if let Some(path)=rfd::FileDialog::new().set_file_name("proxmark-library.json").save_file(){match serde_json::to_vec_pretty(&self.settings.cards).map_err(|e|e.to_string()).and_then(|v|storage::write_private(&path,&v)){Ok(())=>{self.status="Library exported. The file contains readable credential/card data; share it deliberately.".into();self.status_ok=true;},Err(e)=>self.fail(e)}}}
        });
        let mut items = self.settings.cards.clone();
        for p in &self.settings.credential_profiles {
            if let Ok(encoded) = p.credential.encode() {
                items.push(SavedCard {
                    name: format!("{} · credential profile", p.name),
                    card: Card {
                        content: Content::Hid {
                            raw: encoded.hid_raw,
                            credential: Some(p.credential.clone()),
                        },
                        captured_at: 0,
                        simulated: self.simulation.is_some(),
                    },
                });
            }
        }
        for p in &self.settings.lf_profiles {
            items.push(SavedCard {
                name: format!("{} · LF profile", p.name),
                card: Card {
                    content: Content::Lf(p.clone()),
                    captured_at: 0,
                    simulated: self.simulation.is_some(),
                },
            });
        }
        if items.is_empty() {
            ui.label("Your Library is empty. Read a card and save it, or import a Studio dump.");
        }
        let mut remove = None;
        for (index, item) in items
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.name
                    .to_lowercase()
                    .contains(&self.basic.filter.to_lowercase())
            })
            .collect::<Vec<_>>()
        {
            Self::card(ui, |ui| {
                ui.heading(&item.name);
                ui.label(item.card.summary());
                if item.card.simulated {
                    ui.colored_label(ui.visuals().warn_fg_color, "SIMULATED");
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("View / decode").clicked() {
                        self.basic.flow.set_source(item.card.clone());
                        self.basic.route=Route::Read;
                        self.status="Loaded a saved capture for local decoding. No new hardware read was performed.".into();
                        self.status_ok=true;
                    }
                    if Self::action(ui, item.card.writable().is_ok(), "Use for Write") {
                        self.basic.flow.set_source(item.card.clone());
                        self.basic.flow.step = StepView::Target;
                        self.basic.route = Route::Write;
                    }
                    if index < self.settings.cards.len()
                        && ui.button("Remove from Library").clicked()
                    {
                        remove = Some(index);
                    }
                    if let Err(e) = item.card.writable() {
                        ui.label(e);
                    }
                });
            });
            ui.add_space(8.0);
        }
        if let Some(index) = remove {
            self.settings.cards.remove(index);
            if let Err(e) = self.persist_ui() {
                self.fail(e);
            }
        }
        ui.label(RichText::new("DESFire authentication profiles, custom formats and CLI-catalogue profiles remain in Advanced → Saved profiles / Credentials.").color(appearance::secondary(ui)));
    }
    fn basic_history(&mut self, ui: &mut egui::Ui) {
        Self::title(ui,"HISTORY","Recent guided operations","Results describe software observations. A verified encoding does not prove acceptance by an access reader.");
        if self.settings.operations.is_empty() {
            ui.label("No guided operations recorded yet.");
        }
        if ui.button("Export history…").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("proxmark-history.json")
                .save_file()
            {
                let result = serde_json::to_vec_pretty(&self.settings.operations)
                    .map_err(|e| e.to_string())
                    .and_then(|v| storage::write_private(&path, &v));
                if let Err(e) = result {
                    self.fail(e);
                }
            }
        }
        for r in self.settings.operations.iter().rev() {
            Self::card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.heading(&r.action);
                    ui.colored_label(
                        if r.outcome == "Verified" || r.outcome == "Completed" {
                            ui.visuals().selection.stroke.color
                        } else {
                            ui.visuals().warn_fg_color
                        },
                        &r.outcome,
                    );
                    if r.simulated {
                        ui.colored_label(ui.visuals().warn_fg_color, "SIMULATED");
                    }
                });
                ui.label(&r.detail);
                ui.label(
                    RichText::new(format!("{} · retained locally", format_time(r.at)))
                        .small()
                        .color(appearance::secondary(ui)),
                );
            });
            ui.add_space(8.0);
        }
    }
    fn basic_setup(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "DEVICE SETUP",
            "Connect your reader",
            "Choose the RRG/Iceman executable and serial device, then check the connection.",
        );
        let before = (
            self.settings.cli.clone(),
            self.settings.working_dir.clone(),
            self.settings.port.clone(),
        );
        Self::card(ui, |ui| {
            if self.simulation.is_none() {
                Self::text_field(ui, "RRG/Iceman executable", &mut self.settings.cli);
                ui.horizontal_wrapped(|ui|{if ui.button("Browse executable…").clicked(){if let Some(p)=rfd::FileDialog::new().pick_file(){self.settings.cli=p.to_string_lossy().into();self.inspected=false;self.caps=Default::default();self.clear_target();}}
if ui.button("Find installed client").clicked(){let found=proxmark_studio::maintenance::discover();if let Some(path)=found.first(){self.settings.cli=path.to_string_lossy().into();self.inspected=false;self.caps=Default::default();self.clear_target();}else{self.fail("No client found automatically. Browse to your native RRG/Iceman executable.");}}});
                Self::text_field(
                    ui,
                    "Working folder · optional",
                    &mut self.settings.working_dir,
                );
                if ui.button("Choose folder…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.settings.working_dir = p.to_string_lossy().into();
                        self.inspected = false;
                        self.clear_target();
                    }
                }
                ui.label("Serial device");
                egui::ComboBox::from_id_salt("basic_port")
                    .width(400.0)
                    .selected_text(if self.settings.port.is_empty() {
                        "Select a device"
                    } else {
                        &self.settings.port
                    })
                    .show_ui(ui, |ui| {
                        for (name, description) in &self.ports {
                            ui.selectable_value(
                                &mut self.settings.port,
                                name.clone(),
                                format!("{name} · {description}"),
                            );
                        }
                    });
                Self::text_field(ui, "Manual serial port", &mut self.settings.port);
                ui.horizontal(|ui| {
                    if ui.button("Refresh").clicked() {
                        self.refresh_ports();
                    }
                    if ui.button("Check connection").clicked() {
                        self.clear_target();
                        self.caps = Default::default();
                        self.inspected = false;
                        self.basic.connect_after_probe = true;
                        self.start(Job::Probe, "Inspect CLI");
                    }
                    if ui.button("Save connection").clicked() {
                        self.save();
                    }
                });
            } else {
                ui.heading("Demo client connected");
                ui.label("Your physical connection settings are retained separately.");
            }
            ui.separator();
            if self.simulation.is_none() {
                if Self::action(
                    ui,
                    proxmark_studio::simulation::bundled_mock().is_some(),
                    "Try demo with mock client",
                ) {
                    self.lock_library();
                    match proxmark_studio::simulation::configure(
                        &self.settings,
                        &proxmark_studio::simulation::bundled_mock().unwrap(),
                    ) {
                        Ok((settings, session)) => {
                            self.clear_target();
                            self.settings = settings;
                            self.simulation = Some(session);
                            self.caps = Default::default();
                            self.inspected = false;
                            self.basic.connect_after_probe = true;
                            let _ = self.place_demo_card();
                            self.start(Job::Probe, "Inspect CLI");
                        }
                        Err(e) => self.fail(e),
                    }
                }
            } else if ui.button("End demo").clicked() {
                self.settings = self
                    .simulation
                    .as_ref()
                    .unwrap()
                    .settings_to_save(&self.settings);
                self.clear_target();
                self.inspected = false;
                self.caps = Default::default();
                self.status =
                    "Demo ended. Physical connection restored; check it before use.".into();
            }
        });
        if self.simulation.is_none()
            && before
                != (
                    self.settings.cli.clone(),
                    self.settings.working_dir.clone(),
                    self.settings.port.clone(),
                )
            && self.worker.is_none()
        {
            self.caps = Default::default();
            self.inspected = false;
            self.clear_target();
        }
        ui.add_space(12.0);
        ui.label(if self.device_seen{"Reader responded to a device check. Basic rechecks matching firmware before each write."}else{"Reader has not been checked in this session."});
        if self.version_blocked {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Client and firmware do not match. Writing is blocked.",
            );
        }
        if ui.button("Open reader diagnostics").clicked() {
            self.settings.ui_mode = Mode::Advanced;
            self.page = Page::Tools;
            self.tools.tab = 2;
        }
        ui.label(RichText::new("Firmware maintenance, timeout options and command capability details are in Advanced → Device setup / Client & firmware.").color(appearance::secondary(ui)));
    }
}
// Civil date conversion keeps local history readable without adding a time runtime.
fn format_time(seconds: u64) -> String {
    let days = (seconds / 86400) as i64 + 719468;
    let era = days / 146097;
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(m <= 2);
    format!(
        "{year:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}

impl Studio {
    fn em_decoding(&mut self, ui: &mut egui::Ui, card: &Card) {
        use proxmark_studio::em410x::{self, ReaderFormat};
        let Some((id, selected)) = card.em_identity() else {
            return;
        };
        let format = selected.unwrap_or_default();
        let Ok(decoded) = em410x::decode(id, format) else {
            ui.colored_label(ui.visuals().warn_fg_color, "Invalid EM410x ID");
            return;
        };
        ui.label(
            RichText::new("DECODED CREDENTIAL")
                .small()
                .color(ui.visuals().selection.stroke.color),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("Facility {}", decoded.facility))
                    .size(24.0)
                    .strong(),
            );
            ui.add_space(16.0);
            ui.label(
                RichText::new(format!("Card number {}", decoded.number))
                    .size(24.0)
                    .strong(),
            );
        });
        ui.label(if selected.is_some() {
            "User-selected reader interpretation. Controller acceptance is unverified."
        } else {
            "Preview using the low 24 bits. The fob does not identify its reader's output format."
        });
        if self.basic.flow.step == StepView::Source {
            let mut choice = selected;
            ui.label("Reader format");
            egui::ComboBox::from_id_salt("em_reader_interpretation")
                .width(365.0)
                .selected_text(
                    selected
                        .map(ReaderFormat::name)
                        .unwrap_or("Unconfirmed · preview Wiegand 26"),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice, None, "Unconfirmed · preview Wiegand 26");
                    for f in ReaderFormat::ALL {
                        ui.selectable_value(&mut choice, Some(f), f.name());
                    }
                });
            if choice != selected {
                let mut c = card.clone();
                if c.select_em_format(choice).is_ok() {
                    self.basic.flow.set_source(c);
                }
            }
            ui.horizontal_wrapped(|ui| {
                let label = ui.label("Known facility code · optional");
                ui.add(
                    egui::TextEdit::singleline(&mut self.basic.em_known_facility)
                        .desired_width(100.0)
                        .hint_text("Decimal"),
                )
                .labelled_by(label.id);
            });
            match em410x::known_facility(&self.basic.em_known_facility) {
                Ok(Some(known)) => {
                    let matches = em410x::matching_formats(id, known).unwrap_or_default();
                    if matches.is_empty() {
                        ui.colored_label(ui.visuals().warn_fg_color,"No supported reader interpretation matches that facility code. The captured ID is unchanged.");
                    } else {
                        if decoded.facility == known {
                            ui.colored_label(
                                ui.visuals().selection.stroke.color,
                                format!("Displayed facility matches your known code {known}."),
                            );
                        } else {
                            ui.colored_label(ui.visuals().warn_fg_color,"The displayed interpretation does not match the known facility; see the matching layouts below.");
                        }
                        ui.label(format!("{} supported layouts match. A facility match alone does not confirm the reader format.",matches.len()));
                        for d in matches {
                            ui.label(format!(
                                "{}: facility {}, card {}",
                                d.format.name(),
                                d.facility,
                                d.number
                            ));
                        }
                    }
                }
                Err(e) => {
                    ui.colored_label(ui.visuals().warn_fg_color, e);
                }
                Ok(None) => {}
            }
        } else {
            ui.label(format.name());
        }
        ui.collapsing("Raw ID and other number formats",|ui| {
            ui.monospace(format!("EM410x ID: {}",decoded.id));
            ui.label(format.explanation());
            ui.label(format!("Full 40-bit decimal: {} · low 32-bit decimal: {} · low 24-bit decimal: {}",decoded.decimal40,decoded.decimal32,decoded.decimal24));
            egui::Grid::new("em_format_candidates").num_columns(3).striped(true).show(ui,|ui| {
                ui.strong("Interpretation");ui.strong("Facility / customer");ui.strong("Card number");ui.end_row();
                for f in ReaderFormat::ALL {if let Ok(d)=em410x::decode(id,f){ui.label(f.name());ui.monospace(d.facility.to_string());ui.monospace(d.number.to_string());ui.end_row();}}
            });
            ui.label("Cloning preserves all 40 ID bits, regardless of this display choice. No new HID encoding is substituted.");
        });
    }
}
