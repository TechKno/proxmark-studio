use super::*;
use proxmark_studio::{
    basic::{Action, Card, Hint, StepView},
    netaccess::{self, CardRecord, Profile, Repository, State, Store, User},
};

#[derive(Clone, Copy)]
enum Pending {
    Scan,
    CloneRead(u16),
    Check,
    Write(u16),
    CloneWrite(u16),
}
#[derive(Default)]
struct UserEditor {
    key: Option<u32>,
    name: String,
    controller_number: String,
}
struct Transfer {
    card: u16,
    from: u32,
    to: Option<u32>,
}
struct FobEditor {
    card: u16,
    owner: u32,
    notes: String,
    number: String,
    confirm_delete: bool,
}
struct ProfileEditor {
    profile: Profile,
    creating: bool,
}
fn outcome_label(outcome: &Outcome) -> &str {
    match outcome {
        Outcome::Completed => "completed without verification",
        Outcome::Verified => "verified",
        Outcome::Failed(message) => message,
        Outcome::Cancelled => "cancelled",
        Outcome::TimedOut => "timed out",
        Outcome::Mismatch => "read-back did not match",
    }
}
fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_setup_and_profile_editor_render_without_creating_company_data() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = Studio::with_config(&ctx, Some(directory.path().join("settings.json")));
        app.settings.ui_mode = Mode::NetAccess;
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.render(ctx));
        assert!(app.netaccess.repo.is_none());
        app.netaccess.profile_editor = Some(ProfileEditor {
            profile: Profile::default(),
            creating: true,
        });
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.render(ctx));
        assert!(app.netaccess.repo.is_none());
        assert!(!directory.path().join("organisation-register.json").exists());
        assert!(app.worker.is_none());
    }
    #[test]
    fn na1_register_and_write_states_render_without_executing_operations() {
        let dir = tempfile::tempdir().unwrap();
        let data = Store {
            profile: Profile {
                organisation: "Example Organisation".into(),
                site: "Main".into(),
                facility: 42,
                confirmed: true,
                ..Default::default()
            },
            users: vec![User {
                number: 1,
                name: "Example User".into(),
                controller_number: Some(1),
            }],
            cards: vec![CardRecord::imported(1001, 1)],
            ..Store::default()
        };
        netaccess::save_roster(&dir.path().join("netaccess-na1.json"), &data).unwrap();
        let ctx = egui::Context::default();
        let mut app = Studio::with_config(&ctx, Some(dir.path().join("settings.json")));
        app.settings.ui_mode = Mode::NetAccess;
        app.na1_load();
        app.netaccess.selected = Some(1);
        let n = app
            .netaccess
            .repo
            .as_mut()
            .unwrap()
            .update(|s| s.reserve(1))
            .unwrap();
        app.na1_prepare(n).unwrap();
        for theme in [
            proxmark_studio::appearance::Appearance::Light,
            proxmark_studio::appearance::Appearance::Dark,
        ] {
            app.settings.ui_preferences.appearance = theme;
            app.settings.ui_preferences.text_size = 20.0;
            appearance::apply(&ctx, &app.settings.ui_preferences);
            for step in [StepView::Target, StepView::Review, StepView::Result] {
                app.netaccess.flow.step = step;
                app.netaccess.flow.destination = Some(basic::Destination::T55 {
                    target: Target::T5577,
                    block0: "000880E0".into(),
                });
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(960.0, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.render(ctx),
                );
                assert!(output.shapes.len() > 10);
                assert!(app.worker.is_none());
            }
        }
        assert_eq!(app.netaccess.repo.as_ref().unwrap().data.cards.len(), 2);
        let before = std::fs::read(dir.path().join("netaccess-na1.json")).unwrap();
        app.netaccess.editor = Some(UserEditor {
            key: Some(1),
            name: "Example User".into(),
            controller_number: "1".into(),
        });
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.render(ctx));
        app.netaccess.editor = None;
        app.netaccess.transfer = Some(Transfer {
            card: 1001,
            from: 1,
            to: None,
        });
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.render(ctx));
        app.netaccess.transfer = None;
        for confirm_delete in [false, true] {
            app.netaccess.fob_editor = Some(FobEditor {
                card: 1001,
                owner: 1,
                notes: "Example".into(),
                number: "334".into(),
                confirm_delete,
            });
            let _ = ctx.run(egui::RawInput::default(), |ctx| app.render(ctx));
        }
        assert!(app.worker.is_none());
        assert_eq!(
            std::fs::read(dir.path().join("netaccess-na1.json")).unwrap(),
            before
        );
    }
}
#[derive(Default)]
pub(super) struct NetAccessView {
    repo: Option<Repository>,
    attempted: Option<PathBuf>,
    pub setup: bool,
    selected: Option<u32>,
    query: String,
    scan: Option<Card>,
    number: Option<u16>,
    pub flow: basic::Workflow,
    pending: Option<Pending>,
    message: String,
    editor: Option<UserEditor>,
    transfer: Option<Transfer>,
    fob_editor: Option<FobEditor>,
    cloning: bool,
    profile_editor: Option<ProfileEditor>,
}
impl Studio {
    fn na1_path(&self) -> PathBuf {
        if let Some(simulation) = &self.simulation {
            return simulation
                .directory
                .path()
                .join("organisation-register.json");
        }
        if !self.settings.organisation_register.is_empty() {
            return PathBuf::from(&self.settings.organisation_register);
        }
        let legacy = self.path.with_file_name("netaccess-na1.json");
        if legacy.exists() {
            legacy
        } else {
            self.path.with_file_name("organisation-register.json")
        }
    }
    fn na1_load(&mut self) {
        let path = self.na1_path();
        if self.netaccess.pending.is_some() || self.netaccess.attempted.as_ref() == Some(&path) {
            return;
        }
        self.netaccess.repo = None;
        self.netaccess.attempted = Some(path.clone());
        self.netaccess.scan = None;
        self.netaccess.flow = Default::default();
        self.netaccess.number = None;
        self.netaccess.selected = None;
        self.netaccess.editor = None;
        self.netaccess.transfer = None;
        self.netaccess.fob_editor = None;
        self.netaccess.cloning = false;
        self.netaccess.profile_editor = None;
        if self.simulation.is_some() && !path.exists() {
            let demo = Store {
                profile: Profile {
                    organisation: "Demo Organisation".into(),
                    site: "Example site".into(),
                    facility: 42,
                    confirmed: true,
                    ..Default::default()
                },
                simulated: true,
                source: "Synthetic demo; no access system personal data".into(),
                users: vec![
                    User {
                        number: 1,
                        name: "Demo User One".into(),
                        controller_number: Some(1),
                    },
                    User {
                        number: 2,
                        name: "Demo User Two".into(),
                        controller_number: Some(2),
                    },
                ],
                cards: vec![CardRecord::imported(1001, 1)],
                ..Store::default()
            };
            if let Err(e) = netaccess::save_roster(&path, &demo) {
                self.netaccess.message = e;
                return;
            }
        }
        match Repository::open(&path, self.simulation.is_some()) {
            Ok(repo) => {
                self.netaccess.repo = Some(repo);
                self.netaccess.message.clear();
            }
            Err(e) => self.netaccess.message = e,
        }
    }
    fn na1_prepare(&mut self, number: u16) -> Result<(), String> {
        let repo = self
            .netaccess
            .repo
            .as_ref()
            .ok_or("Open the access system register first")?;
        let c = repo
            .data
            .card(number)
            .ok_or("Reservation no longer exists")?;
        if Some(c.user_no) != self.netaccess.selected
            || c.deleted
            || !matches!(c.state, State::Reserved | State::Uncertain)
        {
            return Err("Choose an unused reservation for the selected user".into());
        }
        let card = Card {
            content: basic::Content::Em410xMapped {
                id: c.planned_id.clone().ok_or("Planned fob ID is missing")?,
                format: repo.data.profile.format,
            },
            captured_at: basic::now(),
            simulated: repo.data.simulated,
        };
        self.netaccess.cloning = c.clone_write;
        self.netaccess.flow.set_source(card);
        self.netaccess.flow.step = StepView::Target;
        self.netaccess.number = Some(number);
        Ok(())
    }
    fn na1_start(&mut self, action: Action, pending: Pending) -> Result<(), String> {
        if self.worker.is_some()
            || self.terminal.is_some()
            || self.maintenance.busy
            || self.settings.rf_may_be_active
        {
            return Err("Finish the current operation before using the reader".into());
        }
        if !self.inspected || !self.device_seen || self.settings.port.is_empty() {
            return Err("Check the reader connection in Device setup first".into());
        }
        let repo = self
            .netaccess
            .repo
            .as_mut()
            .ok_or("Open the access system register first")?;
        if repo.data.simulated != self.simulation.is_some() {
            return Err("The reader context changed; reopen Organisations mode".into());
        }
        if let Pending::Write(number) | Pending::CloneWrite(number) = pending {
            let Action::Write {
                card,
                acknowledged: true,
                ..
            } = &action
            else {
                return Err("Review and acknowledge the destination first".into());
            };
            let (found, id) = netaccess::identity(&repo.data.profile, card, repo.data.simulated)?;
            if number != found {
                return Err("Reserved number does not match the fob data".into());
            }
            let user = self.netaccess.selected.ok_or("Choose a user")?;
            if matches!(pending, Pending::CloneWrite(_)) {
                repo.update(|s| s.begin_clone(number, user, &id))?;
            } else {
                repo.update(|s| s.begin_write(number, user, &id))?;
            }
        }
        let label = match pending {
            Pending::Scan => "Organisation scan",
            Pending::CloneRead(_) => "Read original fob for cloning",
            Pending::Check => "Organisation destination check",
            Pending::Write(_) => "Organisation write & verify",
            Pending::CloneWrite(_) => "Organisation clone & verify",
        };
        self.netaccess.pending = Some(pending);
        self.netaccess.message = format!("{label}...");
        self.start(
            Job::Guided(Box::new(basic::Request {
                action,
                caps: self.caps.clone(),
                simulated: self.simulation.is_some(),
            })),
            label,
        );
        Ok(())
    }
    pub(super) fn finish_netaccess(&mut self, result: &proxmark_studio::runner::JobResult) {
        let Some(pending) = self.netaccess.pending.take() else {
            return;
        };
        let result_message = (|| -> Result<String, String> {
            let repo = self
                .netaccess
                .repo
                .as_mut()
                .ok_or("The access system register is unavailable")?;
            match pending {
                Pending::CloneRead(expected) => {
                    if result.outcome != Outcome::Completed {
                        return Err(format!(
                            "Original read did not complete: {}",
                            outcome_label(&result.outcome)
                        ));
                    }
                    let card = result
                        .evidence
                        .card
                        .as_ref()
                        .ok_or("No supported original fob was read")?;
                    let (number, id) =
                        netaccess::identity(&repo.data.profile, card, repo.data.simulated)?;
                    if number != expected {
                        return Err(format!("Read card {number}, but selected card {expected}. Place the selected original fob."));
                    }
                    let owner = repo
                        .update(|s| s.observe_scan(card))?
                        .ok_or("Original fob is not assigned")?;
                    self.netaccess.selected = Some(owner);
                    self.netaccess.number = Some(number);
                    self.netaccess.cloning = true;
                    self.netaccess.flow.set_source(Card {
                        content: basic::Content::Em410xMapped {
                            id,
                            format: repo.data.profile.format,
                        },
                        captured_at: card.captured_at,
                        simulated: card.simulated,
                    });
                    self.netaccess.flow.step = StepView::Target;
                    Ok(format!("Original card {number} read. Replace it with a writable blank, then check the blank. The clone keeps the same holder and number."))
                }
                Pending::Scan => {
                    if result.outcome != Outcome::Completed {
                        return Err(format!(
                            "Scan did not complete: {}",
                            outcome_label(&result.outcome)
                        ));
                    }
                    let card = result
                        .evidence
                        .card
                        .as_ref()
                        .ok_or("No supported fob was read")?;
                    let (number, _) =
                        netaccess::identity(&repo.data.profile, card, repo.data.simulated)?;
                    self.netaccess.scan = Some(card.clone());
                    if let Some(user) = repo.update(|s| s.observe_scan(card))? {
                        self.netaccess.selected = Some(user);
                        Ok(format!(
                            "Matched card {number} to {} ({}).",
                            repo.data.user(user).unwrap().name,
                            repo.data.user(user).unwrap().reference()
                        ))
                    } else {
                        Ok(format!("Card {number} is not in the register. Select its user and choose Link scanned fob."))
                    }
                }
                Pending::Check => {
                    self.netaccess.flow.destination = None;
                    self.netaccess.flow.acknowledged = false;
                    if result.outcome != Outcome::Completed {
                        return Err(format!(
                            "Destination check did not complete: {}",
                            outcome_label(&result.outcome)
                        ));
                    }
                    self.netaccess.flow.destination = result.evidence.destination.clone();
                    if self.netaccess.flow.destination.is_none() {
                        return Err("No supported writable blank was found".into());
                    }
                    self.netaccess.flow.step = StepView::Review;
                    Ok(
                        "Destination checked. Review the user and card number before writing."
                            .into(),
                    )
                }
                Pending::Write(number) | Pending::CloneWrite(number) => {
                    self.netaccess.flow.result = Some(result.outcome.clone());
                    self.netaccess.flow.destination = None;
                    self.netaccess.flow.acknowledged = false;
                    self.netaccess.flow.step = StepView::Result;
                    repo.update(|s| s.finish_write(number, &result.outcome))?;
                    if result.outcome == Outcome::Verified
                        && matches!(pending, Pending::CloneWrite(_))
                    {
                        Ok(format!("Clone of card {number} verified by a fresh full-ID read. Both fobs use the same credential."))
                    } else if result.outcome == Outcome::Verified {
                        Ok(format!("Card {number} written and verified by a fresh read. Enrol this number for the selected user in access system."))
                    } else {
                        Err(format!("Card {number} was not verified ({}). Its number remains reserved; inspect the fob before retrying.",outcome_label(&result.outcome)))
                    }
                }
            }
        })();
        self.netaccess.message = result_message.unwrap_or_else(|e| e);
    }
    pub(super) fn netaccess_page(&mut self, ui: &mut egui::Ui) {
        self.na1_load();
        let heading = self
            .netaccess
            .repo
            .as_ref()
            .map(|r| format!("{} · {}", r.data.profile.organisation, r.data.profile.site))
            .unwrap_or_else(|| "Set up your organisation".into());
        let subtitle = self
            .netaccess
            .repo
            .as_ref()
            .map(|r| r.data.profile.summary())
            .unwrap_or_else(|| {
                "Create a private register and configure your reader format.".into()
            });
        Self::title(ui, "ORGANISATIONS", &heading, &subtitle);
        self.organisation_controls(ui);
        if self.simulation.is_some() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "SIMULATED — separate demo register",
            );
        }
        if self.netaccess.repo.is_none() {
            ui.label("Create an organisation to start empty, or open an existing private register. Add users manually or import the CSV template after setup.");
            ui.label(&self.netaccess.message);
            if ui.button("Retry opening register").clicked() {
                self.netaccess.attempted = None;
            }
            return;
        }
        let data = self.netaccess.repo.as_ref().unwrap().data.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{} users · {} active fobs · {} deleted numbers retained",
                data.users.len(),
                data.cards.iter().filter(|c| !c.deleted).count(),
                data.cards.iter().filter(|c| c.deleted).count()
            ));
            if ui.button("Add user").clicked() {
                self.netaccess.editor = Some(UserEditor::default());
                self.netaccess.transfer = None;
                self.netaccess.fob_editor = None;
            }
            if ui.button("Import users / fobs CSV...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .pick_file()
                {
                    let result = (|| -> Result<usize, String> {
                        let bytes = storage::read_bounded(&path, 1024 * 1024)?;
                        let input =
                            String::from_utf8(bytes).map_err(|_| "CSV must use UTF-8 encoding")?;
                        self.netaccess
                            .repo
                            .as_mut()
                            .unwrap()
                            .update(|s| s.import_csv(&input))
                    })();
                    self.netaccess.message = result
                        .map(|n| {
                            format!("CSV imported: {n} fobs. Existing assignments were preserved.")
                        })
                        .unwrap_or_else(|e| e);
                }
            }
            if ui.button("Export card register CSV...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("organisation-card-register.csv")
                    .add_filter("CSV", &["csv"])
                    .save_file()
                {
                    self.netaccess.message =
                        storage::write_private(&path, data.export_csv().as_bytes())
                            .map(|_| {
                                "Register exported. This does not change access system permissions."
                                    .into()
                            })
                            .unwrap_or_else(|e| e);
                }
            }
        });
        ui.label(RichText::new("Numbers are unique against this imported roster and this PC's reservations. New fobs must also be enrolled in access system.").small().color(appearance::secondary(ui)));
        ui.add_space(10.0);
        Self::card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Scan an existing fob");
                if Self::action(ui, self.inspected && self.device_seen, "Scan & find user") {
                    self.netaccess.scan = None;
                    self.netaccess.flow = Default::default();
                    self.netaccess.number = None;
                    if let Err(e) = self.na1_start(Action::Read(Hint::Lf), Pending::Scan) {
                        self.netaccess.message = e;
                    }
                }
                if !self.device_seen && ui.button("Set up reader").clicked() {
                    self.netaccess.setup = true;
                }
            });
            ui.label("Known card numbers select their assigned user automatically. Other facilities and card types are rejected.");
        });
        ui.add_space(10.0);
        ui.columns(2,|columns| {
            columns[0].heading("Select a user");
            Self::text_field(&mut columns[0],"Search name, user or card number",&mut self.netaccess.query);
            let query=self.netaccess.query.trim().to_lowercase();
            egui::ScrollArea::vertical().id_salt("na1_users").max_height(220.0).show(&mut columns[0],|ui| {
                for user in &data.users {
                    let numbers=data.cards.iter().filter(|c|c.user_no==user.number && !c.deleted).map(|c|c.number.to_string()).collect::<Vec<_>>().join(", ");
                    if !query.is_empty() && !format!("{} {} {}",user.name.to_lowercase(),user.reference().to_lowercase(),numbers).contains(&query) {continue;}
                    if ui.selectable_label(self.netaccess.selected==Some(user.number),format!("{}  ·  {}\nCards: {}",user.name,user.reference(),if numbers.is_empty(){"none"}else{&numbers})).clicked() {
                        if self.netaccess.selected!=Some(user.number) {self.netaccess.flow=Default::default();self.netaccess.number=None;self.netaccess.editor=None;self.netaccess.transfer=None;self.netaccess.fob_editor=None;}
                        self.netaccess.selected=Some(user.number);
                    }
                }
            });
            let ui=&mut columns[1];
            if let Some(user)=self.netaccess.selected.and_then(|n|data.user(n)) {
                ui.heading(&user.name);
                ui.label(user.reference());
                if ui.button("Edit user").clicked() {
                    self.netaccess.editor=Some(UserEditor { key:Some(user.number),name:user.name.clone(),controller_number:user.controller_number.map(|n|n.to_string()).unwrap_or_default() });
                    self.netaccess.transfer=None;
                    self.netaccess.fob_editor=None;
                }
                for c in data.cards.iter().filter(|c|c.user_no==user.number && !c.deleted) {
                    ui.label(format!("Card {} · {}",c.number,c.state.label()));
                    if c.enabled==Some(false) || c.authorized==Some(false) {ui.colored_label(ui.visuals().warn_fg_color,format!("access system report: enabled {}, authorized {}",yes_no(c.enabled),yes_no(c.authorized)));}
                    if !c.expires_on.is_empty() {ui.small(format!("Report expiry: {}",c.expires_on));}
                    if c.needs_enrolment {ui.small("access system enrolment or assignment update required.");}
                    if ui.add_enabled(c.state != State::Writing, egui::Button::new(format!("Reassign fob {}...",c.number))).clicked() {
                        self.netaccess.transfer=Some(Transfer {card:c.number,from:user.number,to:None});
                        self.netaccess.editor=None;
                        self.netaccess.fob_editor=None;
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(format!("Edit fob {}...",c.number)).clicked() {
                            self.netaccess.fob_editor=Some(FobEditor {card:c.number,owner:user.number,notes:c.notes.clone(),number:c.number.to_string(),confirm_delete:false});
                            self.netaccess.editor=None;self.netaccess.transfer=None;
                        }
                        if ui.button(format!("Clone fob {}...",c.number)).clicked() {
                            self.netaccess.flow=Default::default();self.netaccess.number=None;
                            if let Err(e)=self.na1_start(Action::Read(Hint::Lf),Pending::CloneRead(c.number)) {self.netaccess.message=e;}
                        }
                        if ui.button(format!("Delete fob {}...",c.number)).clicked() {
                            self.netaccess.fob_editor=Some(FobEditor {card:c.number,owner:user.number,notes:c.notes.clone(),number:c.number.to_string(),confirm_delete:true});
                            self.netaccess.editor=None;self.netaccess.transfer=None;
                        }
                    });
                    if !c.notes.is_empty() {ui.label(&c.notes);}
                    if matches!(c.state,State::Reserved|State::Uncertain) && ui.button(format!("Prepare reserved card {}",c.number)).clicked() {
                        if let Err(e)=self.na1_prepare(c.number) {self.netaccess.message=e;}
                    }
                }
                ui.add_space(8.0);
                if ui.button("Reserve unused number for new fob").clicked() {
                    match self.netaccess.repo.as_mut().unwrap().update(|s|s.reserve(user.number)) {
                        Ok(n)=>{self.netaccess.message=format!("Card {n} is reserved for {}. Place a writable blank.",user.name);if let Err(e)=self.na1_prepare(n){self.netaccess.message=e;}},
                        Err(e)=>self.netaccess.message=e,
                    }
                }
                if let Some(card)=self.netaccess.scan.clone() {
                    if let Ok((n,_))=netaccess::identity(&data.profile,&card,data.simulated) {
                    if data.card(n).is_none() && ui.button(format!("Link scanned fob {n} to {}",user.name)).clicked() {
                    self.netaccess.message=self.netaccess.repo.as_mut().unwrap().update(|s|s.link_scan(&card,Some(user.number)))
                        .map(|_|format!("Card {n} linked to {} in this register. Confirm its enrolment in access system.",user.name)).unwrap_or_else(|e|e);
                    }
                    }
                }
            } else {ui.label("Choose the person receiving the new fob, or scan an existing one to find its user.");}
        });
        self.na1_management(ui, &data);
        ui.add_space(12.0);
        if !self.netaccess.message.is_empty() {
            Self::card(ui, |ui| {
                ui.label(&self.netaccess.message);
            });
        }
        if let (Some(number), Some(card)) =
            (self.netaccess.number, self.netaccess.flow.source.clone())
        {
            ui.add_space(12.0);
            Self::card(ui, |ui| {
                let name = self
                    .netaccess
                    .selected
                    .and_then(|n| data.user(n))
                    .map(|u| u.name.as_str())
                    .unwrap_or("No user selected");
                ui.heading(format!(
                    "{} for {name}",
                    if self.netaccess.cloning {
                        "Clone fob"
                    } else {
                        "New fob"
                    }
                ));
                ui.label(format!(
                    "Facility {} · card {number} · EM410x ID {}",
                    data.profile.facility,
                    card.em_identity().unwrap().0
                ));
                match self.netaccess.flow.step {
                    StepView::Target | StepView::Source => {
                        ui.label(
                            "Place a password-free T5577 or supported Q5 blank on the antenna.",
                        );
                        if Self::action(ui, self.inspected && self.device_seen, "Check blank fob") {
                            self.netaccess.flow.invalidate();
                            if let Err(e) =
                                self.na1_start(Action::Check(card.clone()), Pending::Check)
                            {
                                self.netaccess.message = e;
                            }
                        }
                    }
                    StepView::Review => {
                        if let Some(destination) = self.netaccess.flow.destination.clone() {
                            ui.label(match &destination {
                                basic::Destination::T55 { target, block0 } => format!(
                                    "Checked blank: {} · configuration {}",
                                    target.name(),
                                    block0
                                ),
                                basic::Destination::Memory { geometry, uid } => {
                                    format!("Checked blank: {} · UID {}", geometry.model(), uid)
                                }
                            });
                            ui.checkbox(
                                &mut self.netaccess.flow.acknowledged,
                                "I have checked the user and blank fob and want to overwrite it.",
                            );
                            if Self::action(
                                ui,
                                self.netaccess.flow.acknowledged,
                                if self.netaccess.cloning {
                                    "Write clone & verify"
                                } else {
                                    "Program fob & verify"
                                },
                            ) {
                                if let Err(e) = self.na1_start(
                                    Action::Write {
                                        card: card.clone(),
                                        destination,
                                        acknowledged: true,
                                    },
                                    if self.netaccess.cloning {
                                        Pending::CloneWrite(number)
                                    } else {
                                        Pending::Write(number)
                                    },
                                ) {
                                    self.netaccess.message = e;
                                }
                            }
                        }
                        if ui.button("Check a different blank").clicked() {
                            self.netaccess.flow.invalidate();
                            self.netaccess.flow.step = StepView::Target;
                        }
                    }
                    StepView::Result => {
                        ui.heading(if self.netaccess.flow.result == Some(Outcome::Verified) {
                            "Verified by a fresh read"
                        } else {
                            "Not verified"
                        });
                        ui.label("access system enrolment is separate. A programmed fob does not grant access by itself.");
                        if self.netaccess.flow.result != Some(Outcome::Verified)
                            && ui.button("Check the blank before retrying").clicked()
                        {
                            self.netaccess.flow.invalidate();
                            self.netaccess.flow.step = StepView::Target;
                        }
                    }
                }
            });
        }
        ui.add_space(12.0);
        ui.collapsing("User and fob change history", |ui| {
            if data.changes.is_empty() { ui.label("No user edits or reassignments recorded yet."); }
            egui::ScrollArea::vertical().id_salt("na1_history").max_height(180.0).show(ui, |ui| {
                for change in data.changes.iter().rev().take(100) {
                    ui.label(&change.detail);
                }
            });
            ui.small("Latest 100 changes shown. The full history and timestamps remain in the private register.");
        });
        ui.collapsing("Deleted fobs — numbers remain reserved", |ui| {
            for card in data.cards.iter().filter(|c| c.deleted) {
                ui.label(format!(
                    "Card {} · previous holder {} · revoke in access system separately",
                    card.number,
                    data.user(card.user_no)
                        .map(|u| u.name.as_str())
                        .unwrap_or("Unknown")
                ));
            }
        });
    }
    fn adopt_organisation(&mut self, repo: Repository) {
        let path = repo.path.clone();
        self.settings.organisation_register = path.display().to_string();
        self.netaccess = NetAccessView {
            repo: Some(repo),
            attempted: Some(path),
            ..Default::default()
        };
        self.save();
    }
    fn organisation_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.simulation.is_none(),egui::Button::new("New organisation...")).clicked() {
                self.netaccess.profile_editor=Some(ProfileEditor {profile:Profile::default(),creating:true});
            }
            if ui.add_enabled(self.simulation.is_none(),egui::Button::new("Open organisation...")).clicked() {
                if let Some(path)=rfd::FileDialog::new().add_filter("Private register",&["json"]).pick_file() {
                    if self.netaccess.repo.as_ref().is_none_or(|r|r.path!=path) {
                        match Repository::open(&path,false) {Ok(repo)=>self.adopt_organisation(repo),Err(e)=>self.netaccess.message=e}
                    }
                }
            }
            if let Some(repo)=&self.netaccess.repo {
                if ui.button("Organisation settings").clicked() {
                    self.netaccess.profile_editor=Some(ProfileEditor {profile:repo.data.profile.clone(),creating:false});
                }
                if ui.button("Export profile...").clicked() {
                    if let Some(path)=rfd::FileDialog::new().set_file_name("organisation-profile.json").save_file() {
                        let result=serde_json::to_vec_pretty(&repo.data.profile).map_err(|e|e.to_string()).and_then(|bytes|storage::write_private(&path,&bytes));
                        self.netaccess.message=result.map(|_|"Organisation profile exported without users or fob data. Keep company profiles private.".into()).unwrap_or_else(|e|e);
                    }
                }
            }
        });
        if let Some(mut editor) = self.netaccess.profile_editor.take() {
            let mut keep = true;
            Self::card(ui, |ui| {
                ui.heading(if editor.creating {
                    "Create organisation"
                } else {
                    "Organisation settings"
                });
                Self::text_field(ui, "Organisation name", &mut editor.profile.organisation);
                Self::text_field(ui, "Site name", &mut editor.profile.site);
                ui.label("Fob type: EM410x (125 kHz). Other fob types use Basic / Advanced tools.");
                let format_locked = !editor.creating
                    && self.netaccess.repo.as_ref().is_some_and(|r| {
                        !r.data.cards.is_empty() || !r.data.seen_numbers.is_empty()
                    });
                ui.add_enabled_ui(!format_locked, |ui| {
                    egui::ComboBox::from_id_salt("organisation_format")
                        .selected_text(editor.profile.format.name())
                        .show_ui(ui, |ui| {
                            for format in [
                                proxmark_studio::em410x::ReaderFormat::Wiegand26,
                                proxmark_studio::em410x::ReaderFormat::Wiegand34,
                            ] {
                                if ui
                                    .selectable_value(
                                        &mut editor.profile.format,
                                        format,
                                        format.name(),
                                    )
                                    .changed()
                                {
                                    editor.profile.confirmed = false;
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label("Facility code");
                        if ui
                            .add(
                                egui::DragValue::new(&mut editor.profile.facility).range(0..=65535),
                            )
                            .changed()
                        {
                            editor.profile.confirmed = false;
                        }
                    });
                });
                if format_locked {
                    ui.small("Format and facility are fixed once fobs are recorded. Create a separate register for a different encoding.");
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Allocate new numbers from");
                    ui.add(egui::DragValue::new(&mut editor.profile.first_number).range(1..=65535));
                    ui.label("to");
                    ui.add(egui::DragValue::new(&mut editor.profile.last_number).range(1..=65535));
                });
                ui.checkbox(
                    &mut editor.profile.confirmed,
                    "I have confirmed this reader format and facility code for this site.",
                );
                ui.small("The register is private local data. Existing credentials outside a changed allocation range remain reserved.");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Load profile JSON...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Organisation profile", &["json"])
                            .pick_file()
                        {
                            let result = storage::read_bounded(&path, 65536).and_then(|bytes| {
                                serde_json::from_slice::<Profile>(&bytes).map_err(|e| e.to_string())
                            });
                            match result {
                                Ok(mut profile) => {
                                    profile.confirmed = false;
                                    editor.profile = profile;
                                }
                                Err(e) => self.netaccess.message = e,
                            }
                        }
                    }
                    if ui
                        .button(if editor.creating {
                            "Create private register..."
                        } else {
                            "Save organisation settings"
                        })
                        .clicked()
                    {
                        let result = editor.profile.validate().and_then(|_| {
                            if editor.creating {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_file_name("organisation-register.json")
                                    .set_directory(
                                        self.path.parent().unwrap_or(std::path::Path::new(".")),
                                    )
                                    .save_file()
                                {
                                    let repo = Repository::create(
                                        &path,
                                        Store {
                                            profile: editor.profile.clone(),
                                            ..Default::default()
                                        },
                                    )?;
                                    self.adopt_organisation(repo);
                                    keep = false;
                                }
                            } else {
                                self.netaccess
                                    .repo
                                    .as_mut()
                                    .ok_or("Open an organisation first")?
                                    .update(|s| s.update_profile(editor.profile.clone()))?;
                                self.netaccess.flow = Default::default();
                                self.netaccess.number = None;
                                self.netaccess.scan = None;
                                keep = false;
                            }
                            Ok(!keep)
                        });
                        self.netaccess.message = result
                            .map(|saved| {
                                if saved {
                                    "Organisation settings saved locally.".into()
                                } else {
                                    "Creation cancelled; no register was created.".into()
                                }
                            })
                            .unwrap_or_else(|e| e);
                    }
                    if ui.button("Cancel organisation settings").clicked() {
                        keep = false;
                    }
                });
            });
            if keep {
                self.netaccess.profile_editor = Some(editor);
            }
        }
    }
    fn na1_management(&mut self, ui: &mut egui::Ui, data: &Store) {
        if let Some(mut editor) = self.netaccess.fob_editor.take() {
            let mut keep = true;
            ui.add_space(12.0);
            Self::card(ui, |ui| {
                ui.heading(format!(
                    "{} fob {}",
                    if editor.confirm_delete {
                        "Delete"
                    } else {
                        "Edit"
                    },
                    editor.card
                ));
                if editor.confirm_delete {
                    ui.label("Remove this fob from the active local register? Its number stays reserved and its history is kept. This does not erase the fob or revoke access in access system.");
                    if ui.button("Confirm deletion from local register").clicked() {
                        match self
                            .netaccess
                            .repo
                            .as_mut()
                            .unwrap()
                            .update(|s| s.delete_fob(editor.card, editor.owner))
                        {
                            Ok(()) => {
                                self.netaccess.message = format!(
                                    "Fob {} deleted locally. Revoke it in access system separately.",
                                    editor.card
                                );
                                self.netaccess.flow = Default::default();
                                self.netaccess.number = None;
                                self.netaccess.scan = None;
                                keep = false;
                            }
                            Err(e) => self.netaccess.message = e,
                        }
                    }
                } else {
                    Self::text_field(ui, "Fob notes", &mut editor.notes);
                    ui.small("Use Reassign fob to change its holder.");
                    if ui.button("Save fob notes").clicked() {
                        match self
                            .netaccess
                            .repo
                            .as_mut()
                            .unwrap()
                            .update(|s| s.edit_fob(editor.card, &editor.notes))
                        {
                            Ok(()) => {
                                self.netaccess.message = "Fob notes saved.".into();
                                keep = false;
                            }
                            Err(e) => self.netaccess.message = e,
                        }
                    }
                    ui.separator();
                    Self::text_field(ui, "New card number (1–65535)", &mut editor.number);
                    ui.label("Changing the encoded number requires rewriting the fob. The old assignment stays active until a verified write; its number is then retained as deleted. Update access system separately.");
                    if ui.button("Prepare card number change").clicked() {
                        let result = (|| -> Result<u16, String> {
                            let number = editor
                                .number
                                .trim()
                                .parse::<u16>()
                                .map_err(|_| "Enter an unused card number from 1 to 65535")?;
                            self.netaccess.repo.as_mut().unwrap().update(|s| {
                                s.edit_fob(editor.card, &editor.notes)?;
                                s.change_number(editor.card, editor.owner, number)
                            })?;
                            Ok(number)
                        })();
                        match result {
                            Ok(number) => {
                                self.netaccess.selected = Some(editor.owner);
                                self.netaccess.message=format!("Number {number} reserved to replace {}. Place the fob to rewrite, check it, then review and program it.",editor.card);
                                if let Err(e) = self.na1_prepare(number) {
                                    self.netaccess.message = e;
                                }
                                keep = false;
                            }
                            Err(e) => self.netaccess.message = e,
                        }
                    }
                }
                if ui.button("Cancel fob edit").clicked() {
                    keep = false;
                }
            });
            if keep {
                self.netaccess.fob_editor = Some(editor);
            }
        }
        if let Some(mut editor) = self.netaccess.editor.take() {
            let mut keep = true;
            ui.add_space(12.0);
            Self::card(ui, |ui| {
                ui.heading(if editor.key.is_some() {
                    "Edit user"
                } else {
                    "Add user"
                });
                Self::text_field(ui, "User name", &mut editor.name);
                Self::text_field(
                    ui,
                    "Controller user number (optional)",
                    &mut editor.controller_number,
                );
                ui.small("Leave the controller user number blank if unknown. This saves the local register; update access system separately.");
                ui.horizontal(|ui| {
                    if ui.button("Save user").clicked() {
                        let result = (|| -> Result<u32, String> {
                            let number = if editor.controller_number.trim().is_empty() {
                                None
                            } else {
                                Some(editor.controller_number.trim().parse::<u32>().map_err(
                                    |_| "Enter a whole Controller user number or leave it blank",
                                )?)
                            };
                            self.netaccess.repo.as_mut().unwrap().update(|s| {
                                if let Some(key) = editor.key {
                                    s.edit_user(key, &editor.name, number)?;
                                    Ok(key)
                                } else {
                                    s.add_user(&editor.name, number)
                                }
                            })
                        })();
                        match result {
                            Ok(key) => {
                                self.netaccess.selected = Some(key);
                                self.netaccess.query.clear();
                                self.netaccess.flow = Default::default();
                                self.netaccess.number = None;
                                self.netaccess.message =
                                    "User saved in the local register. access system is unchanged."
                                        .into();
                                keep = false;
                            }
                            Err(e) => self.netaccess.message = e,
                        }
                    }
                    if ui.button("Cancel user edit").clicked() {
                        keep = false;
                    }
                });
            });
            if keep {
                self.netaccess.editor = Some(editor);
            }
        }
        if let Some(mut transfer) = self.netaccess.transfer.take() {
            let mut keep = true;
            ui.add_space(12.0);
            Self::card(ui, |ui| {
                ui.heading(format!("Reassign fob {}", transfer.card));
                ui.label(format!(
                    "Current holder: {}",
                    data.user(transfer.from)
                        .map(|u| u.name.as_str())
                        .unwrap_or("Unknown")
                ));
                egui::ComboBox::from_id_salt("na1_transfer_to")
                    .selected_text(
                        transfer
                            .to
                            .and_then(|n| data.user(n))
                            .map(|u| u.name.as_str())
                            .unwrap_or("Choose new holder"),
                    )
                    .show_ui(ui, |ui| {
                        for user in data.users.iter().filter(|u| u.number != transfer.from) {
                            ui.selectable_value(
                                &mut transfer.to,
                                Some(user.number),
                                format!("{} · {}", user.name, user.reference()),
                            );
                        }
                    });
                ui.label("The card number and full fob ID stay the same. This updates the local holder and history. Update the assignment in access system separately.");
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            transfer.to.is_some(),
                            egui::Button::new("Save reassignment"),
                        )
                        .clicked()
                    {
                        let to = transfer.to.unwrap();
                        match self
                            .netaccess
                            .repo
                            .as_mut()
                            .unwrap()
                            .update(|s| s.reassign(transfer.card, transfer.from, to))
                        {
                            Ok(()) => {
                                self.netaccess.selected = Some(to);
                                self.netaccess.query.clear();
                                self.netaccess.flow = Default::default();
                                self.netaccess.number = None;
                                self.netaccess.scan = None;
                                self.netaccess.message = format!(
                                    "Fob {} reassigned to {} locally. Update access system separately.",
                                    transfer.card,
                                    data.user(to).unwrap().name
                                );
                                keep = false;
                            }
                            Err(e) => self.netaccess.message = e,
                        }
                    }
                    if ui.button("Cancel reassignment").clicked() {
                        keep = false;
                    }
                });
            });
            if keep {
                self.netaccess.transfer = Some(transfer);
            }
        }
    }
}
