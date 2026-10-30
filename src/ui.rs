mod appearance;
mod basic_view;
mod classic_view;
mod design;
mod netaccess_view;
mod platform;
mod security_view;
mod tools_view;
use basic_view::BasicView;
use eframe::egui::{self, Color32, RichText, Vec2};
use proxmark_studio::basic::{self, Mode};
use proxmark_studio::{
    capabilities::{Capabilities, COMMANDS},
    command::{self, CommandSpec, DesfireParams, Kind},
    credential::{self, Credential, CredentialProfile, Decoded, Format, Target},
    model::{Algorithm, Channel, Communication, Evidence, Profile, Settings},
    runner::{Event, Job, Outcome, Worker},
    storage,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Page {
    Overview,
    Classic,
    Tags,
    Desfire,
    Credentials,
    Console,
    Profiles,
    Connection,
    Workbench,
    Maintenance,
    Tools,
}
#[derive(Clone, Copy, PartialEq)]
enum Family {
    Ntag,
    Em410x,
}
struct Review {
    job: Job,
    summary: String,
}

type PortList = Result<Vec<(String, String)>, String>;

pub struct Studio {
    classic: classic_view::ClassicView,
    show_settings: bool,
    native_menu: Option<platform::NativeMenu>,
    library_passphrase: Zeroizing<String>,
    file_passphrase: Zeroizing<String>,
    library_unlock_input: Zeroizing<String>,
    passphrase_confirmation: Zeroizing<String>,
    security_message: String,
    tools: tools_view::ToolsView,
    basic: BasicView,
    netaccess: netaccess_view::NetAccessView,
    show_logs: bool,
    desfire_tab: usize,
    credential_tab: usize,
    settings: Settings,
    path: PathBuf,
    page: Page,
    ports: Vec<(String, String)>,
    port_rx: Option<mpsc::Receiver<PortList>>,
    worker: Option<Worker>,
    caps: Capabilities,
    logs: VecDeque<String>,
    log_filter: String,
    status: String,
    status_ok: bool,
    started: Option<Instant>,
    last_label: String,
    evidence: Evidence,
    device_seen: bool,
    inspected: bool,
    aid: String,
    fid: String,
    algorithm: Algorithm,
    channel: Channel,
    communication: Communication,
    key_number: u8,
    key: Zeroizing<String>,
    no_auth: bool,
    backup_file: bool,
    offset: u32,
    length: u32,
    payload: String,
    apps: Vec<String>,
    files: Vec<String>,
    file_settings: Vec<String>,
    file_metadata: Option<(String, String, proxmark_studio::desfire_archive::File)>,
    family: Family,
    ntag_page: u32,
    em_id: String,
    credential_format: Format,
    facility_code: String,
    card_number: String,
    credential_target: Target,
    credential_input: String,
    credential_binary_input: bool,
    decoded_credential: Option<Decoded>,
    credential_decode_error: Option<String>,
    credential_profile_name: String,
    console: Zeroizing<String>,
    console_mask: bool,
    terminal: Option<proxmark_studio::terminal::Session>,
    terminal_text: String,
    terminal_screen: vt100::Parser,
    terminal_private: bool,
    terminal_offline: bool,
    history: proxmark_studio::terminal::History,
    bench: proxmark_studio::workbench::Workbench,
    admin: proxmark_studio::desfire_admin::Admin,
    archive: proxmark_studio::desfire_archive::Workspace,
    provisioning: proxmark_studio::provision::Workspace,
    maintenance: proxmark_studio::maintenance::Maintenance,
    custom: proxmark_studio::custom_format::Layout,
    library: proxmark_studio::format_catalogue::Workspace,
    lf: proxmark_studio::lf::Credential,
    version_blocked: bool,
    ports_refreshed: Instant,
    port_was_present: bool,
    usb_identity: Option<String>,
    profile_name: String,
    review: Option<Review>,
    review_ack: bool,
    show_help: bool,
    closing: bool,
    last_activity: Instant,
    key_version: u8,
    simulation: Option<proxmark_studio::simulation::Session>,
}
impl Studio {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Option<PathBuf>) -> Self {
        {
            let mut app = Self::with_config(&cc.egui_ctx, config);
            app.native_menu = platform::NativeMenu::install().ok();
            app
        }
    }
    #[cfg(test)]
    fn with_context(ctx: &egui::Context) -> Self {
        Self::with_config(ctx, None)
    }
    fn with_config(ctx: &egui::Context, config: Option<PathBuf>) -> Self {
        appearance::fonts(ctx);
        let path = config
            .or_else(|| std::env::var_os("PROXMARK_STUDIO_CONFIG").map(PathBuf::from))
            .map(Ok)
            .unwrap_or_else(storage::config_path)
            .unwrap_or_else(|_| PathBuf::from("proxmark-studio-settings.json"));
        let (mut settings, status) = match storage::load(&path) {
            Ok(s) => (
                s,
                "Ready. Configure your RRG/Iceman client to begin.".into(),
            ),
            Err(e) => (Settings::default(), e),
        };
        appearance::apply(ctx, &settings.ui_preferences);
        settings
            .command_favourites
            .retain(|s| proxmark_studio::explorer::valid_name(s));
        for record in &mut settings.operations {
            if record.outcome == "Running" {
                record.outcome = "Interrupted / unknown".into();
                record.detail = "The app closed before a result was recorded. Read the destination before retrying.".into();
            }
        }
        let mut app = Self {
            classic: Default::default(),
            show_settings: false,
            native_menu: None,
            library_passphrase: Zeroizing::new(String::new()),
            file_passphrase: Zeroizing::new(String::new()),
            library_unlock_input: Zeroizing::new(String::new()),
            passphrase_confirmation: Zeroizing::new(String::new()),
            security_message: String::new(),
            tools: Default::default(),
            basic: BasicView::default(),
            netaccess: Default::default(),
            show_logs: false,
            desfire_tab: 0,
            credential_tab: 0,
            settings,
            path,
            page: Page::Overview,
            ports: vec![],
            port_rx: None,
            worker: None,
            caps: Capabilities::default(),
            logs: VecDeque::new(),
            log_filter: String::new(),
            status,
            status_ok: true,
            started: None,
            last_label: String::new(),
            evidence: Evidence::default(),
            device_seen: false,
            inspected: false,
            aid: "000000".into(),
            fid: "01".into(),
            algorithm: Algorithm::Aes,
            channel: Channel::Ev1,
            communication: Communication::Plain,
            key_number: 0,
            key: Zeroizing::new(String::new()),
            no_auth: true,
            backup_file: false,
            offset: 0,
            length: 4,
            payload: "DE AD BE EF".into(),
            apps: vec![],
            files: vec![],
            file_settings: vec![],
            file_metadata: None,
            family: Family::Ntag,
            ntag_page: 4,
            em_id: String::new(),
            credential_format: Format::H10301,
            facility_code: "0".into(),
            card_number: "0".into(),
            credential_target: Target::T5577,
            credential_input: String::new(),
            credential_binary_input: false,
            decoded_credential: None,
            credential_decode_error: None,
            credential_profile_name: String::new(),
            console: Zeroizing::new(String::new()),
            console_mask: false,
            terminal: None,
            terminal_text: String::new(),
            terminal_screen: vt100::Parser::new(36, 120, 200),
            terminal_private: false,
            terminal_offline: false,
            history: Default::default(),
            bench: Default::default(),
            admin: Default::default(),
            archive: Default::default(),
            provisioning: Default::default(),
            maintenance: Default::default(),
            custom: Default::default(),
            library: Default::default(),
            lf: Default::default(),
            version_blocked: false,
            ports_refreshed: Instant::now(),
            port_was_present: false,
            usb_identity: None,
            profile_name: String::new(),
            review: None,
            review_ack: false,
            show_help: false,
            closing: false,
            last_activity: Instant::now(),
            key_version: 0,
            simulation: None,
        };
        app.admin.alias = app.settings.vault_alias.clone();
        for line in &app.settings.console_history {
            if proxmark_studio::terminal::persistable(line) {
                app.history.add(line);
            }
        }
        app.refresh_ports();
        app
    }
    fn push(&mut self, s: impl Into<String>) {
        if self.logs.len() >= 1500 {
            self.logs.pop_front();
        }
        self.logs.push_back(s.into());
    }
    fn fail(&mut self, e: impl Into<String>) {
        self.status = e.into();
        self.status_ok = false;
    }
    fn refresh_ports(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.port_rx = Some(rx);
        std::thread::spawn(move || {
            let value = serialport::available_ports()
                .map(|ports| {
                    ports
                        .into_iter()
                        .map(|p| {
                            let desc = match p.port_type {
                                serialport::SerialPortType::UsbPort(u) => format!(
                                    "USB {:04X}:{:04X} {} serial:{}",
                                    u.vid,
                                    u.pid,
                                    u.product.unwrap_or_default(),
                                    u.serial_number.unwrap_or_default()
                                ),
                                _ => "Serial port".into(),
                            };
                            (p.port_name, desc)
                        })
                        .collect()
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(value);
        });
    }
    fn clear_target(&mut self) {
        self.version_blocked = false;
        self.device_seen = false;
        self.apps.clear();
        self.files.clear();
        self.file_settings.clear();
        self.file_metadata = None;
        self.evidence = Evidence::default();
        self.forget_session_keys();
        self.simulation = None;
        self.basic.flow.invalidate();
        self.netaccess.flow.invalidate();
        self.netaccess.flow.step = basic::StepView::Target;
    }
    fn forget_session_keys(&mut self) {
        self.tools.forget();
        self.classic.forget();
        self.lock_library();
        self.file_passphrase.zeroize();
        self.library_unlock_input.zeroize();
        self.passphrase_confirmation.zeroize();
        self.review = None;
        self.key.zeroize();
        self.admin.newkey.zeroize();
        self.admin.oldkey.zeroize();
        self.bench.key.zeroize();
        self.bench.key_map.zeroize();
        self.bench.trailer_editor.new_a.zeroize();
        self.bench.trailer_editor.new_b.zeroize();
        self.console.zeroize();
    }
    fn start(&mut self, job: Job, label: &str) {
        if self.settings.rf_may_be_active
            && !matches!(
                &job,
                Job::Probe
                    | Job::CommandCatalogue
                    | Job::CommandHelp(_)
                    | Job::Emulator(proxmark_studio::emulator::Request {
                        action: proxmark_studio::emulator::Action::Stop,
                        ..
                    })
            )
        {
            self.fail("RF simulation may still be active. Send Stop and confirm the device is stopped before another operation.");
            return;
        }
        if self.maintenance.busy {
            self.fail("Wait for the client/firmware tool to finish");
            return;
        }
        if self.terminal.is_some() {
            self.fail("End the interactive console session before starting another operation");
            return;
        }
        if self.worker.is_some() {
            return;
        }
        if matches!(
            &job,
            Job::Emulator(proxmark_studio::emulator::Request {
                action: proxmark_studio::emulator::Action::Simulate(_),
                ..
            })
        ) {
            self.settings.rf_may_be_active = true;
            self.save();
        }
        self.evidence = Evidence::default();
        self.last_label = label.into();
        self.status = format!("{label}…");
        self.status_ok = true;
        self.started = Some(Instant::now());
        self.worker = Some(Worker::start(self.settings.clone(), job));
    }
    fn run_guided(&mut self, spec: Result<CommandSpec, String>, label: &str) {
        match spec {
            Ok(s) => match self.caps.validate(&s.text) {
                Ok(()) => self.start(Job::Run(s), label),
                Err(e) => self.fail(e),
            },
            Err(e) => self.fail(e),
        }
    }
    fn poll(&mut self) {
        let connection = format!(
            "{}|{}|{}",
            self.settings.cli, self.settings.working_dir, self.settings.port
        );
        if self.tools.connection != connection {
            self.tools = tools_view::ToolsView::default();
            self.tools.connection = connection;
        }

        if self.ports_refreshed.elapsed() > Duration::from_secs(3) && self.port_rx.is_none() {
            self.ports_refreshed = Instant::now();
            self.refresh_ports();
        }
        if let Some(session) = &self.terminal {
            for mut text in session.output.try_iter().take(128) {
                if !self.terminal_private {
                    self.terminal_screen.process(text.as_bytes());
                    self.terminal_text = self.terminal_screen.screen().contents();
                }
                text.zeroize();
            }
            if self.terminal_text.len() > 256 * 1024 {
                let mut cut = self.terminal_text.len() - 128 * 1024;
                while !self.terminal_text.is_char_boundary(cut) {
                    cut += 1;
                }
                self.terminal_text.drain(..cut);
            }
        }
        if let Some(rx) = &self.port_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(p) => {
                        let present = p.iter().any(|(port, _)| port == &self.settings.port);
                        if self.port_was_present && !present && self.simulation.is_none() {
                            if let Some(w) = &self.worker {
                                self.tools.live = false;
                                w.cancel();
                            }
                            self.terminal = None;
                            self.clear_target();
                            self.push("Selected serial device disconnected. Operation cancelled; reconnect and check the target before retrying.");
                        }
                        if self.simulation.is_none() {
                            if let Some((_, desc)) =
                                p.iter().find(|(port, _)| port == &self.settings.port)
                            {
                                if desc
                                    .split_once("serial:")
                                    .is_some_and(|(_, s)| !s.is_empty())
                                {
                                    self.usb_identity = Some(desc.clone());
                                }
                            } else if let Some(id) = &self.usb_identity {
                                let matching =
                                    p.iter().filter(|(_, desc)| desc == id).collect::<Vec<_>>();
                                if matching.len() == 1 && self.worker.is_none() {
                                    self.settings.port = matching[0].0.clone();
                                    self.push("The same USB serial device reappeared. Connection restored; check the target before programming.");
                                }
                            }
                        }
                        self.port_was_present = present;
                        self.ports = p;
                    }
                    Err(e) => self.push(format!("Serial enumeration: {e}")),
                };
                self.port_rx = None;
            }
        }
        let events: Vec<_> = self
            .worker
            .as_ref()
            .map(|w| w.rx.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Log(s) => {
                    if let Some(stage) = s.strip_prefix("[stage] ") {
                        self.basic.stage = stage.into();
                    }
                    self.push(s);
                }
                Event::Finished(r) => {
                    let r = *r;
                    if self.last_label == "Inspect Classic source"
                        && r.outcome == Outcome::Completed
                    {
                        if let Some(uid) = &r.evidence.uid {
                            self.classic.uid = uid.clone();
                        }
                    }
                    if let Some(keys) = &r.evidence.recovered {
                        self.classic.recovered = Some(keys.clone());
                    }
                    if let Some(target) = &r.evidence.magic_target {
                        self.classic.target = Some(target.clone());
                    }
                    if let Some(dump) = &r.evidence.memory_dump {
                        self.classic.source = Some(proxmark_studio::magic::image(
                            dump.clone(),
                            self.simulation.is_some(),
                        ));
                        self.bench.geometry = dump.geometry;
                        self.bench.uid = dump.uid.clone();
                        self.bench.editor = command::encode(&dump.bytes);
                        self.bench.dump = Some(dump.clone());
                        self.bench.origin_simulated = Some(self.simulation.is_some());
                        self.classic.message =
                            "Complete source captured. Place a blank, then open Magic-card clone."
                                .into();
                    }
                    if let Some(capture) = &r.evidence.signal {
                        self.tools.receive_signal(capture.clone());
                    }
                    if self.last_label == "Capture live signal" && r.outcome != Outcome::Completed {
                        self.tools.live = false;
                    }

                    if !r.evidence.command_catalogue.is_empty() {
                        self.tools.entries = r.evidence.command_catalogue.clone();
                    }
                    if let Some((name, help)) = &r.evidence.command_help {
                        self.tools.receive_help(name, help);
                    }
                    if let Some(g) = self.tools.pending_geometry.take() {
                        self.tools.buffer =
                            if matches!(r.outcome, Outcome::Completed | Outcome::Verified)
                                && r.evidence.bytes.len() == g.units() as usize * 16
                            {
                                Some((g, r.evidence.bytes.clone()))
                            } else {
                                None
                            };
                    }
                    self.bench
                        .receive_selection(&r.evidence.bytes, r.outcome == Outcome::Completed);
                    if self.last_label == "Request RF stop"
                        && self.simulation.is_some()
                        && r.outcome == Outcome::Completed
                    {
                        self.settings.rf_may_be_active = false;
                        self.save();
                    }
                    self.finish_basic(&r);
                    self.finish_netaccess(&r);
                    self.provisioning.finish(&r.outcome);
                    if !r.evidence.format_catalogue.is_empty() {
                        self.library.formats = r.evidence.format_catalogue.clone();
                    }
                    if let Some(raw) = &r.evidence.encoded_wiegand {
                        self.library.encoded = raw.clone();
                    }
                    if let Some(text) = &r.evidence.decoded_wiegand {
                        self.library.decoded = text.clone();
                    }
                    if self.bench.pending {
                        if r.outcome == Outcome::Completed {
                            self.bench.receive(r.evidence.bytes.clone());
                            self.bench.origin_simulated = Some(self.simulation.is_some());
                        } else {
                            self.bench.pending = false;
                            self.bench.message =
                                "Backup did not complete; no complete dump created".into();
                        }
                    }
                    self.worker = None;
                    if let Some(c) = r.capabilities {
                        self.caps = c;
                        self.inspected = true;
                    }
                    self.device_seen |= r.evidence.device_seen && r.outcome == Outcome::Completed;
                    if self.last_label == "List applications" {
                        self.apps = r.evidence.aids.clone();
                        self.files.clear();
                        self.file_settings.clear();
                    }
                    if self.last_label == "List files" {
                        self.files = r.evidence.files.clone();
                        self.file_settings.clear();
                    }
                    if self.last_label == "File settings" {
                        self.file_settings = r.evidence.settings.clone();
                        if let Some(mut f) = r.evidence.desfire_file.clone() {
                            f.id = u8::from_str_radix(&self.fid, 16).unwrap_or(0);
                            self.admin.file_mode = f.mode;
                            self.admin.rights = format!("{:04X}", f.rights);
                            self.admin.size = if f.kind >= 3 {
                                f.record_size
                            } else {
                                f.size.max(1)
                            };
                            self.admin.records = f.max_records.max(1);
                            self.admin.lower = f.lower;
                            self.admin.upper = f.upper;
                            self.admin.limited_credit = f.limited;
                            self.file_metadata = Some((self.aid.clone(), self.fid.clone(), f));
                        }
                    }
                    if self.last_label == "Bind DESFire target" && r.outcome == Outcome::Completed {
                        self.admin.target_uid = r.evidence.uid.clone().unwrap_or_default();
                    }
                    if self.last_label == "Check device" {
                        self.version_blocked = r.evidence.version_mismatch;
                    }
                    self.status_ok = matches!(r.outcome, Outcome::Completed | Outcome::Verified);
                    self.status=match r.outcome{
                    Outcome::Completed=>format!("{}: CLI completed. Review the observed result below.",self.last_label),
                    Outcome::Verified=>if let Some(detail)=&r.evidence.verification {format!("{}: verified • {detail}",self.last_label)} else if r.evidence.credential.is_some() {format!("{}: verified • format, facility code, card number, parity and raw data match",self.last_label)} else {format!("{}: verified • exact read-back bytes match",self.last_label)},
                    Outcome::Mismatch=>format!("{}: verification FAILED • read-back differs",self.last_label),
                    Outcome::Cancelled=>"Cancelled. Any write already sent may have altered the tag; read it again.".into(),
                    Outcome::TimedOut=>if self.last_label=="Inspect CLI" {"CLI capability inspection timed out. Check the executable and working folder; see the launch details in the log.".into()}else{"CLI timed out. Tag state is unknown; read it again before retrying.".into()},
                    Outcome::Failed(e)=>format!("{}: {e}",self.last_label),
                };
                    self.evidence = r.evidence;
                    self.push(self.status.clone());
                    if self.basic.connect_after_probe && self.last_label == "Inspect CLI" {
                        self.basic.connect_after_probe = false;
                        if self.caps.has("hw version") {
                            self.start(
                                Job::Run(CommandSpec::simple("hw version", Kind::Device)),
                                "Check device",
                            );
                        }
                    }
                }
            }
        }
    }
    fn params(&self) -> DesfireParams<'_> {
        DesfireParams {
            aid: &self.aid,
            fid: &self.fid,
            key: &self.key,
            key_number: self.key_number,
            algorithm: self.algorithm,
            channel: self.channel,
            communication: self.communication,
            no_auth: self.no_auth,
            backup_file: self.backup_file,
        }
    }
    fn des_spec(&self, kind: Kind) -> Result<CommandSpec, String> {
        self.params().build(kind, self.offset, self.length, None)
    }
    fn des_action(&mut self, kind: Kind, label: &str) {
        let spec = self.des_spec(kind);
        self.run_guided(spec, label);
    }
    fn check(&self, spec: &CommandSpec) -> Result<(), String> {
        self.caps.validate(&spec.text)
    }
    fn review_write(&mut self, desfire: bool) {
        if !desfire && self.family == Family::Ntag {
            self.page = Page::Workbench;
            self.fail("Use Card memory for NTAG writes with model, UID and lock checks");
            return;
        }
        if desfire {
            if let Err(e) = proxmark_studio::backup::validate_uid(&self.admin.target_uid) {
                self.fail(e);
                return;
            }
        }
        let value = (|| -> Result<Review, String> {
            let (write, read, expected, target) = if desfire {
                let data = command::payload(&self.payload)?;
                let write = self.params().build(
                    Kind::Write,
                    self.offset,
                    data.len() as u32,
                    Some(&data),
                )?;
                let read = self
                    .params()
                    .build(Kind::Read, self.offset, data.len() as u32, None)?;
                (
                    write,
                    read,
                    data,
                    format!(
                        "DESFire application {} / file {} / offset {}",
                        self.aid, self.fid, self.offset
                    ),
                )
            } else if self.family == Family::Ntag {
                let data = command::payload(&self.payload)?;
                (
                    command::mfu_write(self.ntag_page, &data)?,
                    command::mfu_read(self.ntag_page)?,
                    data,
                    format!("Ultralight / NTAG page {}", self.ntag_page),
                )
            } else {
                let id = command::hex(&self.em_id, 5, "EM410x ID")?;
                let data = command::payload(&id)?;
                (
                    command::em_write(&id)?,
                    CommandSpec::simple("lf em 410x reader", Kind::Read),
                    data,
                    "EM410x encoding on a writable T5577 target".into(),
                )
            };
            self.check(&write)?;
            self.check(&read)?;
            Ok(Review{summary:format!("Port: {}\nTarget: {target}\nData: {}\n\nThe selected target will be altered. Studio will then run a new read and compare every returned byte. Keep the same tag on the antenna.",self.settings.port,command::encode(&expected)),job:Job::Programme{write,read,expected}})
        })();
        match value {
            Ok(mut r) => {
                if desfire {
                    r.job = Job::Bound {
                        uid: self.admin.target_uid.to_uppercase(),
                        job: Box::new(r.job),
                    };
                }
                self.review = Some(r);
                self.review_ack = false;
            }
            Err(e) => self.fail(e),
        }
    }
    fn verify(&mut self, desfire: bool) {
        let value = (|| -> Result<Job, String> {
            let expected = if !desfire && self.family == Family::Em410x {
                command::payload(&command::hex(&self.em_id, 5, "EM410x ID")?)?
            } else {
                command::payload(&self.payload)?
            };
            let read = if desfire {
                self.params()
                    .build(Kind::Read, self.offset, expected.len() as u32, None)?
            } else if self.family == Family::Ntag {
                command::mfu_read(self.ntag_page)?
            } else {
                CommandSpec::simple("lf em 410x reader", Kind::Read)
            };
            self.check(&read)?;
            Ok(Job::Verify { read, expected })
        })();
        match value {
            Ok(j) => self.start(j, "Verify target"),
            Err(e) => self.fail(e),
        }
    }
    fn save(&mut self) {
        match self.persist_ui() {
            Ok(()) => {
                self.status = "Settings saved. Authentication keys use the OS key store; PC images contain raw card data and may include Classic keys.".into();
                self.status_ok = true;
            }
            Err(e) => self.fail(e),
        }
    }
    fn title(ui: &mut egui::Ui, kicker: &str, title: &str, subtitle: &str) {
        let _ = kicker;
        ui.label(RichText::new(title).size(ui.text_style_height(&egui::TextStyle::Body) + 10.0));
        ui.label(RichText::new(subtitle).color(appearance::secondary(ui)));
        ui.add_space(24.0);
    }
    fn card<R>(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
        egui::Frame::new()
            .fill(design::surface(ui))
            .stroke(egui::Stroke::new(0.7_f32, design::line(ui)))
            .corner_radius(12)
            .inner_margin(20)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                f(ui)
            })
            .inner
    }
    fn available(&self, command: &str) -> bool {
        self.worker.is_none() && !self.settings.port.is_empty() && self.caps.has(command)
    }
    fn action(ui: &mut egui::Ui, enabled: bool, text: &str) -> bool {
        ui.add_enabled(enabled, egui::Button::new(text)).clicked()
    }
    fn overview(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "",
            "Workspace",
            "Your reader, card data and specialist tools.",
        );
        ui.horizontal(|ui| {
            for (name, value) in [
                (
                    "Reader",
                    if self.device_seen {
                        "Checked".to_owned()
                    } else {
                        "Not checked".to_owned()
                    },
                ),
                (
                    "Client",
                    if self.inspected {
                        format!("{} commands", self.caps.help.len())
                    } else {
                        "Not inspected".into()
                    },
                ),
                ("Key storage", "System key store".into()),
            ] {
                ui.vertical(|ui| {
                    ui.set_min_width(180.0);
                    ui.label(RichText::new(name).small().color(appearance::secondary(ui)));
                    ui.label(RichText::new(value).size(16.0));
                });
            }
        });
        ui.add_space(24.0);
        ui.separator();
        ui.add_space(20.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("Detect a card");
            if Self::action(ui, self.available("lf search"), "Scan LF") {
                self.run_guided(
                    Ok(CommandSpec::simple("lf search", Kind::Detect)),
                    "LF detection",
                );
            }
            if Self::action(ui, self.available("hf search"), "Scan HF") {
                self.run_guided(
                    Ok(CommandSpec::simple("hf search", Kind::Detect)),
                    "HF detection",
                );
            }
            if ui.link("Reader settings").clicked() {
                self.page = Page::Connection;
            }
        });
        ui.add_space(24.0);
        for (page, name, detail, icon) in [
            (
                Page::Classic,
                "Classic cards",
                "Recover keys and clone supported magic cards.",
                "Classic cards",
            ),
            (
                Page::Desfire,
                "DESFire",
                "Browse applications, authenticate and manage files.",
                "DESFire",
            ),
            (
                Page::Workbench,
                "Card memory",
                "Inspect, edit and compare complete card images.",
                "Card memory",
            ),
            (
                Page::Tools,
                "Signal analysis",
                "Capture waveforms and inspect decoded bits.",
                "Signal",
            ),
            (
                Page::Credentials,
                "Credentials & batch",
                "Work with facility codes, card numbers and provisioning.",
                "Credentials & batch",
            ),
        ] {
            if design::tool_row(ui, name, detail, icon) {
                self.page = page;
                if page == Page::Tools {
                    self.tools.tab = 3;
                }
            }
        }
        ui.add_space(16.0);
        ui.label(
            RichText::new("EV2 / LRP / EV3-specific operations remain unavailable.")
                .small()
                .color(appearance::secondary(ui)),
        );
    }
    fn tags(&mut self, ui: &mut egui::Ui) {
        Self::title(ui,"TAG TOOLS","Read, programme & verify","Guided actions for supported tag families. Use the console for the wider CLI command set.");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.family, Family::Ntag, "Ultralight / NTAG • HF");
            ui.selectable_value(&mut self.family, Family::Em410x, "EM410x to T5577 • LF");
        });
        ui.add_space(12.0);
        Self::card(ui, |ui| {
            ui.heading(if self.family == Family::Ntag {
                "Page workspace"
            } else {
                "125 kHz identity workspace"
            });
            if self.family == Family::Ntag {
                ui.label(RichText::new("Check the actual tag model and its memory boundaries before writing. Lock, OTP and configuration pages are not ordinary data.").color(ui.visuals().warn_fg_color));
                ui.horizontal(|ui| {
                    ui.label("Page (decimal)");
                    ui.add(egui::DragValue::new(&mut self.ntag_page).range(0..=255));
                    if Self::action(ui, self.available("hf mfu info"), "Tag information") {
                        self.run_guided(
                            Ok(CommandSpec::simple("hf mfu info", Kind::Detect)),
                            "Tag information",
                        );
                    }
                });
                ui.label("Expected / programme data (4 hexadecimal bytes)");
                ui.text_edit_singleline(&mut self.payload);
                ui.horizontal(|ui| {
                    if Self::action(ui, self.available("hf mfu rdbl"), "Read page") {
                        self.run_guided(command::mfu_read(self.ntag_page), "Read page");
                    }
                    if Self::action(
                        ui,
                        self.available("hf mfu wrbl") && self.caps.has("hf mfu rdbl"),
                        "Programme & verify…",
                    ) {
                        self.review_write(false);
                    }
                    if Self::action(ui, self.available("hf mfu rdbl"), "Verify expected data") {
                        self.verify(false);
                    }
                });
            } else {
                ui.label("EM410x is read-only. Programming creates an EM410x-compatible encoding on a writable T5577; it does not change an original EM410x tag.");
                ui.label("Expected / programme ID (5 hexadecimal bytes)");
                ui.text_edit_singleline(&mut self.em_id);
                ui.horizontal(|ui| {
                    if Self::action(ui, self.available("lf em 410x reader"), "Read EM410x") {
                        self.run_guided(
                            Ok(CommandSpec::simple("lf em 410x reader", Kind::Read)),
                            "Read EM410x",
                        );
                    }
                    if Self::action(
                        ui,
                        self.available("lf em 410x clone") && self.caps.has("lf em 410x reader"),
                        "Programme T5577 & verify…",
                    ) {
                        self.review_write(false);
                    }
                    if Self::action(
                        ui,
                        self.available("lf em 410x reader"),
                        "Verify expected ID",
                    ) {
                        self.verify(false);
                    }
                });
            }
        });
        ui.add_space(14.0);
        Self::card(ui, |ui| {
            ui.heading("Other HF tags");
            ui.label("MIFARE Classic and ISO14443A identification are available here. Known-key dump, restore and key maps are available in Card memory.");
            ui.horizontal(|ui| {
                if Self::action(ui, self.available("hf 14a info"), "ISO14443A information") {
                    self.run_guided(
                        Ok(CommandSpec::simple("hf 14a info", Kind::Detect)),
                        "ISO14443A information",
                    );
                }
                if Self::action(
                    ui,
                    self.available("hf mf info"),
                    "MIFARE Classic information",
                ) {
                    self.run_guided(
                        Ok(CommandSpec::simple("hf mf info", Kind::Detect)),
                        "MIFARE Classic information",
                    );
                }
            });
        });
        self.read_result(ui);
    }
    fn archive_auth(&self) -> proxmark_studio::desfire_archive::Auth {
        proxmark_studio::desfire_archive::Auth {
            key: self.key.clone(),
            key_number: self.key_number,
            algorithm: self.algorithm,
            channel: self.channel,
            kdf: self.admin.kdf.clone(),
            kdf_input: self.admin.kdf_input.clone(),
            no_auth: self.no_auth,
        }
    }
    fn desfire(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "DESFIRE EXPLORER",
            "Applications & files",
            "CLI-advertised actions • D40 / EV1 authentication • standard and backup data files",
        );
        ui.horizontal_wrapped(|ui| {
            for (i, tab) in [
                "Browse & data",
                "Authentication",
                "Backup & restore",
                "Management",
            ]
            .iter()
            .enumerate()
            {
                ui.selectable_value(&mut self.desfire_tab, i, *tab);
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label("Expected UID for writes");
            ui.text_edit_singleline(&mut self.admin.target_uid);
            if ui.button("Read and bind target UID").clicked() {
                self.run_guided(
                    Ok(CommandSpec::simple("hf 14a info", Kind::Detect)),
                    "Bind DESFire target",
                );
            }
        });
        ui.label("A random or changing UID blocks guided writes. Keep the same card on the antenna; UID binding is not cryptographic identity.");
        ui.label(RichText::new("EV2 / EV3 card identification does not establish support for their security features. Guided EV2, LRP, proximity checks and EV3-specific operations are not implemented.").color(ui.visuals().warn_fg_color));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if Self::action(ui, self.available("hf mfdes info"), "Card information") {
                self.run_guided(
                    Ok(CommandSpec::simple("hf mfdes info", Kind::Detect)),
                    "DESFire information",
                );
            }
            if Self::action(ui, self.available("hf mfdes getversion"), "Get version") {
                self.run_guided(
                    Ok(CommandSpec::simple("hf mfdes getversion", Kind::Detect)),
                    "DESFire version",
                );
            }
        });
        ui.add_space(10.0);
        if self.desfire_tab == 1 {
            Self::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Authentication");
                    ui.checkbox(&mut self.no_auth, "No authentication (free access)");
                });
                if self.no_auth {
                    ui.label(
                        RichText::new("Access still depends on the card's configured permissions.")
                            .color(appearance::secondary(ui)),
                    );
                }
                ui.add_enabled_ui(!self.no_auth, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Algorithm");
                        egui::ComboBox::from_id_salt("algo")
                            .selected_text(self.algorithm.cli().to_uppercase())
                            .show_ui(ui, |ui| {
                                for a in Algorithm::ALL {
                                    ui.selectable_value(
                                        &mut self.algorithm,
                                        a,
                                        a.cli().to_uppercase(),
                                    );
                                }
                            });
                        ui.label("Channel");
                        egui::ComboBox::from_id_salt("channel")
                            .selected_text(self.channel.cli().to_uppercase())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.channel, Channel::D40, "D40");
                                ui.selectable_value(&mut self.channel, Channel::Ev1, "EV1");
                            });
                        ui.label("Mode");
                        egui::ComboBox::from_id_salt("mode")
                            .selected_text(self.communication.cli())
                            .show_ui(ui, |ui| {
                                for c in [
                                    Communication::Plain,
                                    Communication::Mac,
                                    Communication::Encrypt,
                                ] {
                                    ui.selectable_value(&mut self.communication, c, c.cli());
                                }
                            });
                        ui.label("Key no.");
                        ui.add(egui::DragValue::new(&mut self.key_number).range(0..=13));
                    });
                    ui.horizontal(|ui| {
                        ui.label(format!("Key ({} bytes)", self.algorithm.key_bytes()));
                        ui.add(
                            egui::TextEdit::singleline(&mut *self.key)
                                .password(true)
                                .desired_width(340.0)
                                .hint_text("Hexadecimal key • never saved"),
                        );
                        if ui.button("Forget key").clicked() {
                            self.key.zeroize();
                        }
                    });
                });
                ui.label(RichText::new("Each operation authenticates independently. Testing a key does not create a persistent RF session. KDF controls are in Management; archive authentication can be set per application. ISO selection remains a console workflow.").color(appearance::secondary(ui)).small());
            });
            ui.add_space(12.0);
        }
        if self.desfire_tab == 0 {
            ui.columns(2, |cols| {
                Self::card(&mut cols[0], |ui| {
                    ui.heading("Applications");
                    if Self::action(
                        ui,
                        self.available("hf mfdes getaids"),
                        "Refresh application list",
                    ) {
                        self.des_action(Kind::Applications, "List applications");
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("app_list")
                        .max_height(95.0)
                        .show(ui, |ui| {
                            if self.apps.is_empty() {
                                ui.label(
                                    RichText::new("No applications loaded")
                                        .color(appearance::secondary(ui)),
                                );
                            }
                            for aid in self.apps.clone() {
                                if ui
                                    .selectable_label(self.aid == aid, format!("AID  {aid}"))
                                    .clicked()
                                {
                                    self.aid = aid;
                                    self.files.clear();
                                    self.file_settings.clear();
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label("AID");
                        if ui
                            .add(egui::TextEdit::singleline(&mut self.aid).desired_width(110.0))
                            .changed()
                        {
                            self.files.clear();
                            self.file_settings.clear();
                        }
                    });
                    if Self::action(
                        ui,
                        self.available("hf mfdes auth") && !self.no_auth,
                        "Test authentication",
                    ) {
                        self.des_action(Kind::Authenticate, "Test authentication");
                    }
                });
                Self::card(&mut cols[1], |ui| {
                    ui.heading("Files");
                    if Self::action(
                        ui,
                        self.available("hf mfdes getfileids"),
                        "Refresh file list",
                    ) {
                        self.des_action(Kind::Files, "List files");
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("file_list")
                        .max_height(95.0)
                        .show(ui, |ui| {
                            if self.files.is_empty() {
                                ui.label(
                                    RichText::new("No files loaded")
                                        .color(appearance::secondary(ui)),
                                );
                            }
                            for fid in self.files.clone() {
                                if ui
                                    .selectable_label(self.fid == fid, format!("File  {fid}"))
                                    .clicked()
                                {
                                    self.fid = fid;
                                    self.file_settings.clear();
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label("File ID");
                        if ui
                            .add(egui::TextEdit::singleline(&mut self.fid).desired_width(70.0))
                            .changed()
                        {
                            self.file_settings.clear();
                        }
                    });
                    if Self::action(
                        ui,
                        self.available("hf mfdes getfilesettings"),
                        "Inspect file settings",
                    ) {
                        self.des_action(Kind::Settings, "File settings");
                    }
                });
            });
            if !self.file_settings.is_empty() {
                Self::card(ui, |ui| {
                    for s in &self.file_settings {
                        ui.monospace(s);
                    }
                });
            }
            ui.add_space(12.0);
            Self::card(ui, |ui| {
                ui.heading("Data file workspace");
                ui.checkbox(
                    &mut self.backup_file,
                    "Backup data file (commit transaction after writing)",
                );
                ui.label(RichText::new("For standard / backup data files only. Inspect file type, access rights and size first. Value and record operations are in Management. Transaction-MAC operations are unavailable.").color(appearance::secondary(ui)));
                ui.horizontal(|ui| {
                    ui.label("Offset (decimal)");
                    ui.add(egui::DragValue::new(&mut self.offset).range(0..=0xffffff));
                    ui.label("Read length");
                    ui.add(egui::DragValue::new(&mut self.length).range(1..=4096));
                });
                ui.label("Expected / programme data (hexadecimal bytes)");
                ui.text_edit_singleline(&mut self.payload);
                ui.horizontal_wrapped(|ui| {
                    if Self::action(ui, self.available("hf mfdes read"), "Read data") {
                        self.des_action(Kind::Read, "Read DESFire data");
                    }
                    if Self::action(
                        ui,
                        self.available("hf mfdes write") && self.caps.has("hf mfdes read"),
                        "Programme & verify…",
                    ) {
                        self.review_write(true);
                    }
                    if Self::action(ui, self.available("hf mfdes read"), "Verify expected data") {
                        self.verify(true);
                    }
                });
            });
        }
        if self.desfire_tab == 1 {
            ui.collapsing("OS key store",|ui|{
            ui.label("macOS Keychain / Windows Credential Manager. Only the alias is stored in Studio settings. OS access prompts may appear.");
            ui.horizontal(|ui|{ui.label("Key alias");ui.text_edit_singleline(&mut self.admin.alias);});
            ui.horizontal(|ui|{
                if ui.button("Store current key").clicked(){match proxmark_studio::vault::store(&self.admin.alias,&self.key,self.algorithm.key_bytes()){
                    Ok(())=>{let record=proxmark_studio::vault::Record{alias:self.admin.alias.clone(),bytes:self.algorithm.key_bytes(),version:self.key_version,note:format!("DESFire {} key {}",self.aid,self.key_number)};if let Err(e)=proxmark_studio::vault::upsert(&mut self.settings.vault_records,record){self.admin.message=e;}else{self.settings.vault_alias=self.admin.alias.clone();self.save();self.admin.message="Key stored in native vault; only alias/version metadata saved in settings".into();}},Err(e)=>self.admin.message=e
                }}
                if ui.button("Load key").clicked(){match proxmark_studio::vault::load(&self.admin.alias,self.algorithm.key_bytes()){Ok(k)=>{self.key=k;self.admin.message="Key loaded into this session".into()},Err(e)=>self.admin.message=e}}
                if ui.button("Delete stored key").clicked(){match proxmark_studio::vault::delete(&self.admin.alias){Ok(())=>{self.settings.vault_records.retain(|r|r.alias!=self.admin.alias);self.save();self.admin.message="Stored key deleted".into();},Err(e)=>self.admin.message=e}}
                if ui.button("Clear session keys").clicked(){self.forget_session_keys();}
            });
            ui.horizontal(|ui|{ui.label("Stored key version (metadata)");ui.add(egui::DragValue::new(&mut self.key_version));ui.label("Forget idle session keys after seconds (0 disables)");ui.add(egui::DragValue::new(&mut self.settings.key_idle_secs).range(0..=3600));});
            ui.label("Studio vault catalogue (contains only aliases/metadata, not other OS credentials)");
            for record in self.settings.vault_records.clone(){if ui.button(format!("{} • {} bytes • version {} • {}",record.alias,record.bytes,record.version,record.note)).clicked(){self.admin.alias=record.alias;self.key_version=record.version;}}
            ui.label(&self.admin.message);

        });
        }
        if self.desfire_tab == 2 {
            let fallback = self.archive_auth();
            ui.label("Capture, restore and verify supported application data. Keys are supplied separately.");
            ui.push_id("archive_tools",|ui|{
            if let Some((request,write))=self.archive.ui(ui,&self.admin.target_uid,fallback,&self.caps){let job=Job::Desfire(Box::new(request));if write{self.review_ack=false;self.review=Some(Review{summary:format!("Restore applications/files on UID {}. Existing incompatible files are refused. An interrupted restore may be partial; resume uses fresh state checks and a journal. Supplied application keys and final policies will be applied. PICC master key/UID are excluded.",self.admin.target_uid),job});}else{self.start(job,"DESFire archive");}}
        });
        }
        if self.desfire_tab == 3 {
            ui.push_id("management_tools",|ui|{
            let observed=self.file_metadata.as_ref().filter(|(aid,fid,_)|aid==&self.aid&&fid==&self.fid).map(|(_,_,f)|f);
            if self.admin.ui(ui,observed,self.key_number,self.no_auth){match self.admin.build(&self.params()).and_then(|s|{self.check(&s)?;Ok(s)}){
                Ok(spec)=>{if self.admin.action.write(){if let Err(e)=proxmark_studio::backup::validate_uid(&self.admin.target_uid){self.fail(e);return;}
                    let mutation=proxmark_studio::desfire_archive::Mutation{admin:self.admin.clone(),auth:self.archive_auth(),aid:self.aid.clone(),fid:u8::from_str_radix(&self.fid,16).unwrap_or(255),write:spec};
                    let request=proxmark_studio::desfire_archive::Request{uid:self.admin.target_uid.to_uppercase(),creds:proxmark_studio::desfire_archive::Credentials{fallback:self.archive_auth(),keys:Default::default()},operation:proxmark_studio::desfire_archive::Operation::Admin(Box::new(mutation)),capabilities:self.caps.clone()};
                    self.review_ack=false;self.review=Some(Review{summary:format!("DESFire {:?}: application {}, file {}. A fresh result check follows the mutation. Changes can remove data/access.",self.admin.action,self.aid,self.fid),job:Job::Desfire(Box::new(request))});
                }else{self.start(Job::Run(spec),"DESFire management read");}},Err(e)=>self.fail(e)
            }}
        });
        }
        self.read_result(ui);
    }
    fn read_result(&mut self, ui: &mut egui::Ui) {
        for detail in &self.evidence.details {
            ui.monospace(detail);
        }
        if self.last_label == "Read EM410x" && self.evidence.bytes.len() == 5 {
            ui.heading("Decoded EM410x credential");
            ui.label(proxmark_studio::em410x::summary(
                &command::encode(&self.evidence.bytes),
                None,
            ));
            ui.label("Basic Read offers reader-format selection and a known-facility comparison.");
        }
        if !self.evidence.bytes.is_empty() {
            ui.add_space(14.0);
            Self::card(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "OBSERVED READ • {} bytes • {}",
                        self.evidence.bytes.len(),
                        self.last_label
                    ))
                    .color(ui.visuals().selection.stroke.color),
                );
                ui.monospace(command::encode(&self.evidence.bytes));
                ui.label(
                    RichText::new(
                        "This is the last operation's result, not a live view of the current tag.",
                    )
                    .color(appearance::secondary(ui))
                    .small(),
                );
            });
        }
        if self.evidence.authenticated {
            ui.label(
                RichText::new("Authentication success observed for the completed operation.")
                    .color(ui.visuals().selection.stroke.color),
            );
        }
    }
    fn current_credential(&self) -> Result<Credential, String> {
        Credential::from_decimal(
            self.credential_format,
            &self.facility_code,
            &self.card_number,
        )
    }
    fn load_credential_profile(&mut self, profile: &CredentialProfile) {
        if let Err(e) = profile.validate() {
            self.fail(e);
            return;
        }
        self.credential_format = profile.credential.format;
        self.facility_code = profile.credential.facility_code.to_string();
        self.card_number = profile.credential.card_number.to_string();
        self.credential_target = profile.target;
        self.credential_profile_name = profile.name.clone();
        self.decoded_credential = None;
        self.credential_decode_error = None;
        self.status = format!(
            "Loaded credential profile {}. No tag operation performed.",
            profile.name
        );
        self.status_ok = true;
    }
    fn credential_verify(&mut self, programme: bool) {
        let value = (|| -> Result<(Job, Credential), String> {
            let expected = self.current_credential()?;
            self.check(&credential::read_command())?;
            let job = if programme {
                let write = credential::write_command(&expected, self.credential_target)?;
                self.check(&write)?;
                Job::ProgrammeCredential {
                    write,
                    expected: expected.clone(),
                }
            } else {
                Job::VerifyCredential {
                    expected: expected.clone(),
                }
            };
            Ok((job, expected))
        })();
        match value {
            Ok((job, expected)) => {
                if programme {
                    self.review=Some(Review{summary:format!("Port: {}\nWritable target: {}\nFormat: {} ({} bits)\nFacility code: {}\nCard number: {}\nExpected HID raw: {}\n\nProgramming alters the selected writable tag. A separate HID read will then check its raw data, decoded fields and parity. Keep the same tag on the LF antenna.",self.settings.port,self.credential_target.name(),expected.format.name(),expected.format.bits(),expected.facility_code,expected.card_number,expected.encode().unwrap().hid_raw),job});
                    self.review_ack = false;
                } else {
                    self.start(job, "Verify HID credential");
                }
            }
            Err(e) => self.fail(e),
        }
    }
    fn credential_details(ui: &mut egui::Ui, value: &Decoded) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} • {} bits",
                    value.credential.format.name(),
                    value.credential.format.bits()
                ))
                .color(ui.visuals().selection.stroke.color),
            );
            ui.label(format!("Facility: {}", value.credential.facility_code));
            ui.label(format!("Card: {}", value.credential.card_number));
            ui.colored_label(
                if value.parity_valid {
                    ui.visuals().selection.stroke.color
                } else {
                    ui.visuals().warn_fg_color
                },
                if value.parity_valid {
                    "Parity valid"
                } else {
                    "PARITY INVALID"
                },
            );
        });
        ui.label("HID raw (includes transport header where applicable)");
        ui.monospace(&value.encoded.hid_raw);
        ui.label("Wiegand bits (includes parity; no transport header)");
        ui.monospace(&value.encoded.binary);
    }
    fn credential_workspace(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "HID / WIEGAND CREDENTIALS",
            "Facility & card codes",
            "Select a format, decode a credential and programme a compatible writable LF tag.",
        );
        ui.horizontal_wrapped(|ui| {
            for (i, name) in [
                "HID credential",
                "RRG catalogue",
                "Batch",
                "Custom layout",
                "AWID / ioProx",
                "Decode candidates",
            ]
            .iter()
            .enumerate()
            {
                ui.selectable_value(&mut self.credential_tab, i, *name);
            }
        });
        ui.add_space(12.0);
        if self.credential_tab != 0 {
            ui.horizontal_wrapped(|ui| {
                ui.label("Target");
                egui::ComboBox::from_id_salt("expert_target")
                    .selected_text(self.credential_target.name())
                    .show_ui(ui, |ui| {
                        for t in Target::ALL {
                            ui.selectable_value(&mut self.credential_target, t, t.name());
                        }
                    });
                ui.label("Facility");
                ui.add(egui::TextEdit::singleline(&mut self.facility_code).desired_width(100.0));
                ui.label("Card");
                ui.add(egui::TextEdit::singleline(&mut self.card_number).desired_width(130.0));
                egui::ComboBox::from_id_salt("expert_format")
                    .selected_text(self.credential_format.name())
                    .show_ui(ui, |ui| {
                        for f in Format::ALL {
                            ui.selectable_value(&mut self.credential_format, f, f.name());
                        }
                    });
            });
            if self.credential_tab == 3 || self.credential_tab == 5 {
                ui.label("Decode input");
                ui.text_edit_singleline(&mut self.credential_input);
                ui.checkbox(&mut self.credential_binary_input, "Input is binary");
            }
        }
        if self.credential_tab == 1 {
            ui.push_id("RRG format library, issue and OEM fields",|ui|{
            let before=self.settings.library_profiles.clone();
            let request=self.library.ui(ui,&mut self.settings.library_profiles);
            if self.settings.library_profiles!=before{self.save();}
            if let Some(job)=request{
                if let Job::FormatProgramme(value)=&job{let valid=value.encode().and_then(|s|self.check(&s)).and_then(|()|value.write("0").and_then(|s|self.check(&s))).and_then(|()|self.caps.require("lf hid reader",&[]));if let Err(e)=valid{self.fail(e);return;}self.review_ack=false;self.review=Some(Review{summary:format!("Encode current {} fields using the installed RRG codec, programme {} and compare a separate HID read. Physical LF identity and reader acceptance cannot be established.",value.format,value.target.name()),job});}else{self.start(job,"RRG Wiegand format tools");}
            }
        });
        }
        if self.credential_tab == 2 {
            ui.push_id("Credential batch provisioning", |ui| {
                if let Some(job) = self.provisioning.ui(
                    ui,
                    self.credential_format,
                    self.credential_target,
                    self.facility_code.parse().unwrap_or(0),
                ) {
                    match proxmark_studio::provision::check_job(&job, &self.caps) {
                        Ok(()) => {
                            if self.terminal.is_some() || self.maintenance.busy {
                                self.provisioning.finish(&Outcome::Cancelled);
                                self.fail("End the console/tool operation before provisioning");
                            } else {
                                self.start(job, "Provision credential batch row");
                            }
                        }
                        Err(e) => {
                            self.provisioning.finish(&Outcome::Failed(e.clone()));
                            self.fail(e);
                        }
                    }
                }
            });
        }
        if self.credential_tab == 3 {
            ui.push_id("Custom Wiegand layout",|ui|{
            ui.label("Zero-based MSB positions; parity range end is exclusive. Custom data is programmed using the CLI binary-payload adapter and verified against fresh HID raw data.");
            ui.horizontal(|ui|{ui.label("Name");ui.text_edit_singleline(&mut self.custom.name);});
            egui::Grid::new("custom_fields").num_columns(2).show(ui,|ui|{
                for (label,value) in [("Total bits",&mut self.custom.bits),("Facility start",&mut self.custom.facility_start),("Facility width",&mut self.custom.facility_bits),("Card start",&mut self.custom.card_start),("Card width",&mut self.custom.card_bits),("Even parity bit",&mut self.custom.even_bit),("Even range start",&mut self.custom.even_start),("Even range end",&mut self.custom.even_end),("Odd parity bit",&mut self.custom.odd_bit),("Odd range start",&mut self.custom.odd_start),("Odd range end",&mut self.custom.odd_end)]{ui.label(label);ui.add(egui::DragValue::new(value).range(0..=84));ui.end_row();}
            });
            let credential=(||{let fc=self.facility_code.parse::<u32>().map_err(|_|"Invalid facility code")?;let cn=self.card_number.parse::<u64>().map_err(|_|"Invalid card number")?;let e=self.custom.encode(fc,cn)?;Ok::<_,String>((fc,cn,e))})();
            match &credential{Ok((_,_,e))=>{ui.monospace(format!("{}\n{}",e.binary,e.hid_raw));},Err(e)=>{ui.colored_label(ui.visuals().warn_fg_color,e);}}
            ui.horizontal_wrapped(|ui|{
                if ui.button("Save custom credential profile").clicked(){if let Ok((fc,cn,_))=&credential{self.settings.custom_profiles.retain(|p|p.layout.name!=self.custom.name);self.settings.custom_profiles.push(proxmark_studio::custom_format::Profile{layout:self.custom.clone(),facility_code:*fc,card_number:*cn,target:self.credential_target});self.save();}}
                if ui.button("Programme custom & verify…").clicked(){match credential.and_then(|(fc,cn,_)|self.custom.write(fc,cn,self.credential_target)).and_then(|(spec,raw)|{self.check(&spec)?;self.caps.require("lf hid reader",&[])?;Ok((spec,raw))}){Ok((write,expected))=>{self.review_ack=false;self.review=Some(Review{summary:format!("Custom {} on {}. Verification checks the payload encoding only.",self.custom.name,self.credential_target.name()),job:Job::ProgrammeRaw{write,expected}})},Err(e)=>self.fail(e)}}
                if ui.button("Decode custom binary input").clicked(){self.status=match self.custom.decode(self.credential_input.trim()){Ok((fc,cn,p))=>{self.facility_code=fc.to_string();self.card_number=cn.to_string();format!("Custom decoded: FC {fc}, card {cn}, parity valid: {p}")},Err(e)=>e};}
            });
            for p in self.settings.custom_profiles.clone(){ui.horizontal(|ui|{if ui.button(format!("Load {}",p.layout.name)).clicked(){self.custom=p.layout.clone();self.facility_code=p.facility_code.to_string();self.card_number=p.card_number.to_string();self.credential_target=p.target;}
if ui.small_button(format!("Delete {}",p.layout.name)).clicked(){self.settings.custom_profiles.retain(|v|v.layout.name!=p.layout.name);self.save();}});}
        });
        }
        if self.credential_tab == 4 {
            ui.push_id("AWID and ioProx credentials",|ui|{
            use proxmark_studio::lf::Family;
            egui::ComboBox::from_label("LF family").selected_text(format!("{:?}",self.lf.family)).show_ui(ui,|ui|{for f in [Family::Awid26,Family::Awid34,Family::Awid37,Family::Awid50,Family::IoProx]{ui.selectable_value(&mut self.lf.family,f,format!("{f:?}"));}});
            ui.horizontal_wrapped(|ui|{ui.label("Facility code");ui.add(egui::DragValue::new(&mut self.lf.facility));ui.label("Card number");ui.add(egui::DragValue::new(&mut self.lf.number));ui.label("ioProx version");ui.add(egui::DragValue::new(&mut self.lf.version));});
            ui.label("Uses the writable target type selected above. Read-back verifies decoded family and fields; physical target identity cannot be established.");
            ui.horizontal(|ui|{if ui.button("Read LF credential").clicked(){self.run_guided(Ok(self.lf.read()),"Read LF credential");}
if ui.button("Programme LF & verify…").clicked(){self.lf.target=self.credential_target;match self.lf.write().and_then(|w|{self.check(&w)?;self.check(&self.lf.read())?;Ok(())}){Ok(())=>{self.review_ack=false;self.review=Some(Review{summary:format!("Programme {:?}, facility {}, card {} on {} and perform a fresh reader operation.",self.lf.family,self.lf.facility,self.lf.number,self.lf.target.name()),job:Job::ProgrammeLf(self.lf.clone())});},Err(e)=>self.fail(e)}}});
            ui.horizontal(|ui|{ui.label("LF profile name");ui.text_edit_singleline(&mut self.lf.name);if ui.button("Save LF profile").clicked() && !self.lf.name.trim().is_empty()&&self.lf.name.len()<=80{self.settings.lf_profiles.retain(|p|p.name!=self.lf.name);self.settings.lf_profiles.push(self.lf.clone());self.save();}});
            for p in self.settings.lf_profiles.clone(){ui.horizontal(|ui|{if ui.button(format!("Load {}",p.name)).clicked(){self.credential_target=p.target;self.lf=p.clone();}
if ui.small_button(format!("Delete {}",p.name)).clicked(){self.settings.lf_profiles.retain(|v|v.name!=p.name);self.save();}});}
        });
        }
        if self.credential_tab == 5 {
            ui.push_id("Automatic format candidates",|ui|{
            ui.label("Paste binary or HID raw data in the decode field above. Multiple parity-valid formats may match; select the intended layout before programming.");
            for d in credential::candidates(&self.credential_input,self.credential_binary_input){if ui.button(format!("{} • FC {} • card {}",d.credential.format.name(),d.credential.facility_code,d.credential.card_number)).clicked(){self.credential_format=d.credential.format;self.facility_code=d.credential.facility_code.to_string();self.card_number=d.credential.card_number.to_string();self.decoded_credential=Some(d);}}
        });
        }
        if self.credential_tab != 0 {
            return;
        }
        Self::card(ui, |ui| {
            ui.heading("Expected credential");
            ui.horizontal_wrapped(|ui| {
                ui.label("Format");
                egui::ComboBox::from_id_salt("credential_format")
                    .selected_text(format!(
                        "{} • {} bits",
                        self.credential_format.name(),
                        self.credential_format.bits()
                    ))
                    .show_ui(ui, |ui| {
                        for format in Format::ALL {
                            if ui
                                .selectable_value(
                                    &mut self.credential_format,
                                    format,
                                    format!("{} • {} bits", format.name(), format.bits()),
                                )
                                .changed()
                            {
                                self.decoded_credential = None;
                                self.credential_decode_error = None;
                            }
                        }
                    });
                ui.label("Writable target");
                egui::ComboBox::from_id_salt("credential_target")
                    .selected_text(self.credential_target.name())
                    .show_ui(ui, |ui| {
                        for target in Target::ALL {
                            ui.selectable_value(&mut self.credential_target, target, target.name());
                        }
                    });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Facility code (decimal)");
                ui.add(egui::TextEdit::singleline(&mut self.facility_code).desired_width(110.0));
                ui.label("Card number (decimal)");
                ui.add(egui::TextEdit::singleline(&mut self.card_number).desired_width(140.0));
            });
            ui.label(
                RichText::new(format!(
                    "{} limits: facility 0–{} • card 0–{}",
                    self.credential_format.name(),
                    self.credential_format.facility_max(),
                    self.credential_format.card_max()
                ))
                .color(appearance::secondary(ui))
                .small(),
            );
            match self.current_credential().and_then(|c| c.encode()) {
                Ok(encoded) => {
                    ui.label("Expected HID raw");
                    ui.monospace(&encoded.hid_raw);
                }
                Err(e) => {
                    ui.colored_label(ui.visuals().warn_fg_color, e);
                }
            }
            if self.credential_target == Target::Em4305 {
                ui.colored_label(ui.visuals().warn_fg_color,"EM4305/4469 support depends on the tag supporting FSK at RF/50. A target selection does not establish compatibility.");
            }
            ui.horizontal_wrapped(|ui| {
                if Self::action(ui, self.available("lf hid reader"), "Read & decode HID") {
                    self.start(
                        Job::ReadCredential {
                            format: self.credential_format,
                        },
                        "Read HID credential",
                    );
                }
                if Self::action(
                    ui,
                    self.available("lf hid clone")
                        && self.caps.has("lf hid reader")
                        && self.current_credential().is_ok(),
                    "Programme & verify…",
                ) {
                    self.credential_verify(true);
                }
                if Self::action(
                    ui,
                    self.available("lf hid reader") && self.current_credential().is_ok(),
                    "Verify expected credential",
                ) {
                    self.credential_verify(false);
                }
            });
            ui.label(RichText::new("HID Prox encoding only. This does not decode DESFire application-specific credentials or prove reader acceptance. Format selection is an interpretation: multiple Wiegand layouts can fit the same bit length.").color(appearance::secondary(ui)).small());
        });
        if let Some(value) = self.evidence.credential.clone() {
            ui.add_space(12.0);
            Self::card(ui, |ui| {
                ui.heading("Last observed HID read");
                Self::credential_details(ui, &value);
                ui.label(
                    RichText::new(format!(
                        "Completed operation: {}. This is a snapshot of that read.",
                        self.last_label
                    ))
                    .color(appearance::secondary(ui))
                    .small(),
                );
                if ui
                    .button("Use observed fields as expected credential")
                    .clicked()
                {
                    self.credential_format = value.credential.format;
                    self.facility_code = value.credential.facility_code.to_string();
                    self.card_number = value.credential.card_number.to_string();
                }
            });
        }
        ui.add_space(12.0);
        Self::card(ui, |ui| {
            ui.heading("Offline decoder");
            ui.label("Paste HID raw hexadecimal data or Wiegand bits. Decoding uses the selected format above and works without hardware.");
            ui.horizontal(|ui| {
                if ui
                    .selectable_value(&mut self.credential_binary_input, false, "HID raw hex")
                    .changed()
                {
                    self.decoded_credential = None;
                    self.credential_decode_error = None;
                }
                if ui
                    .selectable_value(&mut self.credential_binary_input, true, "Wiegand binary")
                    .changed()
                {
                    self.decoded_credential = None;
                    self.credential_decode_error = None;
                }
            });
            if ui
                .add(
                    egui::TextEdit::singleline(&mut self.credential_input)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(ui.available_width()),
                )
                .changed()
            {
                self.decoded_credential = None;
                self.credential_decode_error = None;
            }
            ui.horizontal(|ui| {
                if ui.button("Decode selected format").clicked() {
                    let result = if self.credential_binary_input {
                        credential::decode_binary(self.credential_format, &self.credential_input)
                    } else {
                        credential::decode_hid(self.credential_format, &self.credential_input)
                    };
                    match result {
                        Ok(value) => {
                            self.decoded_credential = Some(value);
                            self.credential_decode_error = None;
                        }
                        Err(e) => {
                            self.decoded_credential = None;
                            self.credential_decode_error = Some(e);
                        }
                    }
                }
                if ui.button("Load H10301 example").clicked() {
                    self.credential_format = Format::H10301;
                    self.credential_binary_input = false;
                    self.credential_input = "2006EC0C86".into();
                    self.decoded_credential = None;
                    self.credential_decode_error = None;
                }
            });
            if let Some(e) = &self.credential_decode_error {
                ui.colored_label(ui.visuals().warn_fg_color, e);
            }
            if let Some(value) = self.decoded_credential.clone() {
                Self::credential_details(ui, &value);
                if ui
                    .button("Use decoded fields as expected credential")
                    .clicked()
                {
                    self.facility_code = value.credential.facility_code.to_string();
                    self.card_number = value.credential.card_number.to_string();
                }
            }
        });
        ui.add_space(12.0);
        Self::card(ui, |ui| {
            ui.heading("Saved credential profiles");
            ui.label(RichText::new("Saves the format, facility code, card number and target type in readable local settings. No secret keys are stored.").color(appearance::secondary(ui)).small());
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.credential_profile_name)
                        .hint_text("Credential profile name")
                        .desired_width(240.0),
                );
                if ui.button("Save credential profile").clicked() {
                    let result = self.current_credential().and_then(|value| {
                        self.settings.save_credential_profile(CredentialProfile {
                            name: self.credential_profile_name.clone(),
                            credential: value,
                            target: self.credential_target,
                        })
                    });
                    match result {
                        Ok(()) => self.save(),
                        Err(e) => self.fail(e),
                    }
                }
            });
            for (i, profile) in self.settings.credential_profiles.clone().iter().enumerate() {
                ui.push_id(("credential_profile", i), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&profile.name).strong());
                        ui.label(format!(
                            "{} • FC {} • Card {} • {}",
                            profile.credential.format.name(),
                            profile.credential.facility_code,
                            profile.credential.card_number,
                            profile.target.name()
                        ));
                        if ui.button("Load").clicked() {
                            self.load_credential_profile(profile);
                        }
                        if ui.button("Delete").clicked() {
                            self.settings.credential_profiles.remove(i);
                            self.save();
                        }
                    });
                });
            }
        });
    }
    fn workbench_page(&mut self, ui: &mut egui::Ui) {
        if let Some((mut job, write)) = self.bench.ui(ui) {
            if write
                && self
                    .bench
                    .origin_simulated
                    .is_some_and(|s| s != self.simulation.is_some())
            {
                self.bench.pending = false;
                self.fail("The workbench draft belongs to another demo/physical context");
                return;
            }
            if let Job::ClassicTrailer(r) = &mut job {
                r.capabilities = self.caps.clone();
            }
            let valid = if let Job::Batch(b) = &job {
                b.steps.iter().try_for_each(|s| {
                    self.caps.validate(&s.read.text)?;
                    if let Some(w) = &s.write {
                        self.caps.validate(&w.text)?;
                    }
                    Ok::<(), String>(())
                })
            } else {
                Ok(())
            };
            if let Err(e) = valid {
                self.bench.pending = false;
                self.fail(e);
                return;
            }
            if write {
                self.review_ack = false;
                self.review = Some(Review {
                    summary: if matches!(job, Job::ClassicTrailer(_)) {
                        format!("Change Classic sector keys/access bytes on UID {}. A failed or interrupted write can affect access. Fresh new-key and access-byte checks follow. No UID/manufacturer change is performed.",self.bench.uid)
                    } else {
                        format!("Programme writable data units on UID {} ({:?}), then read each unit back. Manufacturer, lock/configuration and trailer memory is skipped. Interrupted restores can be partial.",self.bench.uid,self.bench.geometry)
                    },
                    job,
                });
            } else {
                self.start(job, "Card memory batch");
            }
        }
        self.read_result(ui);
    }
    fn maintenance_page(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(
            self.terminal.is_none() && !self.settings.rf_may_be_active,
            |ui| {
                self.maintenance.ui(ui, &mut self.settings);
            },
        );
    }
    fn console_page(&mut self, ui: &mut egui::Ui) {
        Self::title(ui,"COMMAND CONSOLE","A live RRG session","A native pseudo-terminal supports persistent state, prompts and long commands. No operating-system shell is started.");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.terminal_offline, "Offline session");
            if self.terminal.is_none()
                && ui
                    .add_enabled(
                        !self.settings.rf_may_be_active,
                        egui::Button::new("Start interactive session"),
                    )
                    .clicked()
            {
                match proxmark_studio::terminal::Session::start(
                    &self.settings,
                    self.terminal_offline,
                ) {
                    Ok(t) => {
                        self.terminal = Some(t);
                        self.terminal_private = false;
                        self.terminal_text.clear();
                        self.terminal_screen = vt100::Parser::new(36, 120, 200);
                    }
                    Err(e) => self.fail(e),
                }
            }
            if self.terminal.is_some() {
                if ui.button("Interrupt (Ctrl+C)").clicked() {
                    let _ = self
                        .terminal
                        .as_ref()
                        .unwrap()
                        .send(proxmark_studio::terminal::Input::Interrupt);
                }
                if ui.button("Escape").clicked() {
                    let _ = self
                        .terminal
                        .as_ref()
                        .unwrap()
                        .send(proxmark_studio::terminal::Input::Escape);
                }
                if ui.button("End session").clicked() {
                    self.terminal = None;
                    self.terminal_text.zeroize();
                    self.terminal_screen = vt100::Parser::new(36, 120, 200);
                }
            }
        });
        if self.terminal.is_some() {
            ui.colored_label(ui.visuals().warn_fg_color,"Interactive output can contain card data. Mask input before sensitive commands or prompt responses. Sending sensitive input clears the transcript and withholds all output until the session ends.");
            ui.checkbox(&mut self.console_mask, "Private input / prompt response");
            ui.add(
                egui::TextEdit::singleline(&mut *self.console)
                    .password(self.console_mask)
                    .desired_width(f32::INFINITY)
                    .hint_text("Command or prompt response (up to 1000 bytes)"),
            );
            ui.horizontal(|ui| {
                if ui.button("Send line").clicked() {
                    match proxmark_studio::terminal::validate_input(&self.console) {
                        Ok(()) => {
                            let sensitive = self.console_mask
                                || CommandSpec::console(self.console.to_string())
                                    .is_ok_and(|s| s.sensitive);
                            if sensitive {
                                self.terminal_private = true;
                                self.terminal_text.zeroize();
                                self.terminal_screen = vt100::Parser::new(36, 120, 200);
                            } else {
                                self.history.add(&self.console);
                                self.settings.console_history = self
                                    .history
                                    .lines
                                    .iter()
                                    .filter(|l| proxmark_studio::terminal::persistable(l))
                                    .cloned()
                                    .collect();
                                self.save();
                            }
                            let input = proxmark_studio::terminal::Input::Line(Zeroizing::new(
                                self.console.to_string(),
                            ));
                            if let Err(e) = self.terminal.as_ref().unwrap().send(input) {
                                self.fail(e)
                            }
                            self.console.zeroize();
                        }
                        Err(e) => self.fail(e),
                    }
                }
                if ui.button("Previous command").clicked() {
                    if let Some(v) = self.history.previous() {
                        *self.console = v.into();
                    }
                }
                if ui.button("Next command").clicked() {
                    *self.console = self.history.next_line().unwrap_or("").into();
                }
            });
            let matches: Vec<_> = self
                .caps
                .help
                .keys()
                .filter(|c| c.starts_with(self.console.as_str()) && !self.console.is_empty())
                .take(8)
                .cloned()
                .collect();
            ui.horizontal_wrapped(|ui| {
                for c in matches {
                    if ui.small_button(&c).clicked() {
                        *self.console = c;
                    }
                }
            });
            if self.terminal_private {
                ui.label("Private session: terminal output withheld");
            } else {
                let display = self
                    .terminal_text
                    .lines()
                    .map(proxmark_studio::parse::clean)
                    .collect::<Vec<_>>()
                    .join("\n");
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.monospace(display);
                    });
            }
            return;
        }
        Self::card(ui, |ui| {
            ui.label("Enter an RRG/Iceman command (maximum 4096 UTF-8 bytes). Over 1000 bytes uses a private stdin/Lua bridge. Semicolon-separated CLI commands are supported.");
            ui.label(RichText::new("Console commands can alter tags, run scripts or create files. Keys belong in the DESFire password field. Sensitive command output is withheld and history is not saved.").color(ui.visuals().warn_fg_color));
            ui.checkbox(&mut self.console_mask, "Mask console input");
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut *self.console)
                        .font(egui::TextStyle::Monospace)
                        .password(self.console_mask)
                        .desired_width(ui.available_width() - 100.0)
                        .hint_text("hf search"),
                );
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui
                    .add_enabled(
                        self.worker.is_none() && !self.settings.port.is_empty(),
                        egui::Button::new("Execute"),
                    )
                    .clicked()
                    || enter && self.worker.is_none()
                {
                    match CommandSpec::console(self.console.to_string()) {
                        Ok(s) => {
                            self.console.zeroize();
                            self.start(Job::Run(s), "Console command");
                        }
                        Err(e) => self.fail(e),
                    }
                }
            });
            ui.label(RichText::new("Enable masking before entering sensitive commands. Output appears in the live log below. Long scripts can be invoked through the CLI's script command.").color(appearance::secondary(ui)).small());
            ui.horizontal(|ui| {
                for text in ["hw version", "hf search", "lf search", "hf mfdes help"] {
                    if ui.button(text).clicked() {
                        *self.console = text.into();
                    }
                }
            });
        });
        ui.add_space(15.0);
        ui.label("Command reference");
        ui.hyperlink_to(
            "RRG/Iceman command documentation",
            "https://github.com/RfidResearchGroup/proxmark3/blob/master/doc/commands.md",
        );
    }
    fn profiles(&mut self, ui: &mut egui::Ui) {
        Self::title(ui,"SAVED WORKSPACES","Pick up where you left off.","Connection profiles save connection and DESFire parameters. Facility/card credential profiles are managed in Facility & card codes. Keys are never saved.");
        Self::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.profile_name)
                        .hint_text("Profile name")
                        .desired_width(240.0),
                );
                if ui.button("Save current profile").clicked() {
                    let name = self.profile_name.trim().to_string();
                    if name.is_empty() {
                        self.fail("Enter a profile name");
                    } else {
                        let p = Profile {
                            name: name.clone(),
                            cli: self.settings.cli.clone(),
                            port: self.settings.port.clone(),
                            working_dir: self.settings.working_dir.clone(),
                            aid: self.aid.clone(),
                            fid: self.fid.clone(),
                            algorithm: self.algorithm,
                            channel: self.channel,
                            communication: self.communication,
                            key_number: self.key_number,
                            backup_file: self.backup_file,
                            key_alias: self.admin.alias.clone(),
                        };
                        if let Some(old) =
                            self.settings.profiles.iter_mut().find(|p| p.name == name)
                        {
                            *old = p;
                        } else {
                            self.settings.profiles.push(p);
                        }
                        self.save();
                    }
                }
            });
        });
        ui.add_space(15.0);
        for (i, p) in self.settings.profiles.clone().iter().enumerate() {
            Self::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(&p.name);
                    ui.label(
                        RichText::new(format!(
                            "{} • AID {} • {} / {}",
                            p.port,
                            p.aid,
                            p.algorithm.cli().to_uppercase(),
                            p.channel.cli().to_uppercase()
                        ))
                        .color(appearance::secondary(ui)),
                    );
                });
                ui.horizontal(|ui| {
                    if ui.button("Load profile").clicked() {
                        self.settings.cli = p.cli.clone();
                        self.settings.port = p.port.clone();
                        self.settings.working_dir = p.working_dir.clone();
                        self.aid = p.aid.clone();
                        self.fid = p.fid.clone();
                        self.algorithm = p.algorithm;
                        self.channel = p.channel;
                        self.communication = p.communication;
                        self.key_number = p.key_number;
                        self.no_auth = true;
                        self.backup_file = p.backup_file;
                        self.admin.alias = p.key_alias.clone();
                        self.caps = Capabilities::default();
                        self.inspected = false;
                        self.clear_target();
                        self.status =
                            format!("Loaded {}. Inspect this CLI before guided actions.", p.name);
                    }
                    if ui.button("Delete").clicked() {
                        self.settings.profiles.remove(i);
                        self.save();
                    }
                });
            });
            ui.add_space(10.0);
        }
        if self.settings.profiles.is_empty() {
            ui.label(RichText::new("No profiles yet. Configure a connection and DESFire parameters, then save them here.").color(appearance::secondary(ui)));
        }
        ui.add_space(16.0);
        ui.label(
            RichText::new(format!("Stored in {}", self.path.display()))
                .color(appearance::secondary(ui))
                .small(),
        );
    }
    fn connection(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "CONNECTION",
            "Bring your Proxmark online.",
            "Use a native RRG/Iceman executable and firmware built for the same client version.",
        );
        Self::card(ui, |ui| {
            ui.heading("RRG/Iceman client");
            ui.horizontal_wrapped(|ui| {
                let bundled = proxmark_studio::simulation::bundled_mock();
                if Self::action(
                    ui,
                    bundled.is_some() && self.simulation.is_none(),
                    "Use bundled mock client",
                ) {
                    match proxmark_studio::simulation::configure(&self.settings, &bundled.unwrap())
                    {
                        Ok((settings, directory)) => {
                            self.clear_target();
                            self.settings = settings;
                            self.simulation = Some(directory);
                            self.caps = Capabilities::default();
                            self.inspected = false;
                            self.start(Job::Probe, "Inspect CLI");
                        }
                        Err(e) => self.fail(e),
                    }
                }
                if self.simulation.is_some() && ui.button("End simulation").clicked() {
                    self.settings = self
                        .simulation
                        .as_ref()
                        .unwrap()
                        .settings_to_save(&self.settings);
                    self.clear_target();
                    self.caps = Capabilities::default();
                    self.inspected = false;
                    self.status =
                        "Simulation ended. Your connection settings have been restored.".into();
                    self.status_ok = true;
                }
                ui.label(
                    RichText::new(
                        "Simulation uses a private temporary folder and never opens a device.",
                    )
                    .color(appearance::secondary(ui))
                    .small(),
                );
            });
            let cli_label =
                ui.label("Executable path (not pm3 shell wrapper or a command with arguments)");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut self.settings.cli)
                        .id(egui::Id::new("connection_cli_executable")),
                )
                .labelled_by(cli_label.id)
                .changed()
            {
                self.caps = Capabilities::default();
                self.inspected = false;
                self.clear_target();
            }
            ui.label(
                "Working folder (optional; for CLI assets, scripts and explicit file outputs)",
            );
            if ui
                .text_edit_singleline(&mut self.settings.working_dir)
                .changed()
            {
                self.caps = Capabilities::default();
                self.inspected = false;
                self.clear_target();
            }
            ui.horizontal(|ui| {
                ui.label("Command timeout (seconds)");
                ui.add(egui::DragValue::new(&mut self.settings.timeout_secs).range(2..=300));
                if ui.button("Inspect CLI capabilities").clicked() {
                    self.caps = Capabilities::default();
                    self.inspected = false;
                    self.start(Job::Probe, "Inspect CLI");
                }
            });
            ui.label(RichText::new("Windows: use proxmark3.exe from a working ProxSpace/native installation, with its required DLLs. macOS: use the client binary inside your RRG installation.").color(appearance::secondary(ui)).small());
        });
        ui.add_space(14.0);
        Self::card(ui, |ui| {
            ui.heading("Serial device");
            let old = self.settings.port.clone();
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("ports")
                    .width(350.0)
                    .selected_text(if self.settings.port.is_empty() {
                        "Select a device"
                    } else {
                        &self.settings.port
                    })
                    .show_ui(ui, |ui| {
                        for (name, desc) in &self.ports {
                            ui.selectable_value(
                                &mut self.settings.port,
                                name.clone(),
                                format!("{name} • {desc}"),
                            );
                        }
                    });
                if ui.button("Refresh devices").clicked() {
                    self.refresh_ports();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Manual port");
                ui.text_edit_singleline(&mut self.settings.port);
            });
            if old != self.settings.port {
                self.usb_identity = None;
                self.clear_target();
            }
            if self.ports.is_empty() {
                ui.label(RichText::new("No enumerated serial devices. Connect your reader or enter a port manually.").color(ui.visuals().warn_fg_color));
            }
            ui.horizontal(|ui| {
                if Self::action(ui, self.available("hw version"), "Check device & firmware") {
                    self.device_seen = false;
                    self.run_guided(
                        Ok(CommandSpec::simple("hw version", Kind::Device)),
                        "Check device",
                    );
                }
                if ui.button("Save settings").clicked() {
                    self.save();
                }
            });
        });
        ui.add_space(14.0);
        if self.inspected {
            Self::card(ui, |ui| {
                ui.heading("Advertised command capabilities");
                ui.label(RichText::new("Help confirms command syntax only. Card permissions, firmware and RF support are checked when an action runs.").color(appearance::secondary(ui)));
                egui::Grid::new("cap_grid")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for command in COMMANDS {
                            ui.monospace(*command);
                            ui.colored_label(
                                if self.caps.has(command) {
                                    ui.visuals().selection.stroke.color
                                } else {
                                    ui.visuals().warn_fg_color
                                },
                                if self.caps.has(command) {
                                    "Advertised"
                                } else {
                                    "Unconfirmed / unavailable"
                                },
                            );
                            ui.end_row();
                        }
                    });
            });
        }
    }
}
impl Studio {
    fn render(&mut self, ctx: &egui::Context) {
        appearance::refresh_system(ctx, &self.settings.ui_preferences);
        if ctx.input(|i| i.viewport().close_requested()) && self.worker.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = true;
            if let Some(worker) = &self.worker {
                worker.cancel();
            }
        }
        if ctx.input(|i| !i.events.is_empty()) {
            self.last_activity = Instant::now();
        }
        if self.worker.is_none()
            && self.terminal.is_none()
            && proxmark_studio::vault::expired(
                self.last_activity.elapsed(),
                self.settings.key_idle_secs,
            )
        {
            self.forget_session_keys();
            self.last_activity = Instant::now();
        }
        if self.page != Page::Tools || self.tools.tab != 3 {
            self.tools.live = false;
        }
        self.platform_actions(ctx);
        self.poll();
        if self.tools.live
            && Instant::now() >= self.tools.next_capture
            && self.worker.is_none()
            && self.page == Page::Tools
            && self.tools.tab == 3
            && !self.settings.rf_may_be_active
        {
            self.start_signal(false);
        }
        if self.closing && self.worker.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(if self.worker.is_some() {
            30
        } else {
            200
        }));
        egui::TopBottomPanel::top("topbar")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.faint_bg_color)
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    #[cfg(target_os = "macos")]
                    ui.add_space(72.0);
                    if design::icon_button(ui, "Sidebar").clicked() {
                        self.settings.ui_preferences.show_sidebar =
                            !self.settings.ui_preferences.show_sidebar;
                        let _ = self.persist_ui();
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("Proxmark Studio").strong().size(13.0));
                    let drag_width = (ui.available_width() - 517.0).max(8.0);
                    let (_, drag) =
                        ui.allocate_exact_size(Vec2::new(drag_width, 30.0), egui::Sense::drag());
                    if drag.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    let old = self.settings.ui_mode;
                    design::mode_picker(ui, &mut self.settings.ui_mode);
                    if old != self.settings.ui_mode {
                        if let Err(e) = self.persist_ui() {
                            self.fail(e);
                        }
                    }
                    ui.add_space(10.0);
                    let state = if self.simulation.is_some() {
                        "Demo reader"
                    } else if self.version_blocked {
                        "Firmware mismatch"
                    } else if self.device_seen {
                        "Reader ready"
                    } else {
                        "Reader not checked"
                    };
                    ui.label(
                        RichText::new(state)
                            .size(12.0)
                            .color(appearance::secondary(ui)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if design::icon_button(ui, "Settings").clicked() {
                            self.open_settings(ctx);
                        }
                        if design::icon_button(ui, "Logs").clicked() {
                            self.show_logs = !self.show_logs;
                        }
                    });
                });
            });
        if self.settings.ui_preferences.show_sidebar {
            egui::SidePanel::left("navigation")
                .default_width(208.0)
                .width_range(170.0..=300.0)
                .resizable(true)
                .frame(
                    egui::Frame::new()
                        .fill(ctx.style().visuals.faint_bg_color)
                        .inner_margin(12),
                )
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.add_space(14.0);
                    ui.label(
                        RichText::new("Workspace")
                            .small()
                            .color(appearance::secondary(ui)),
                    );
                    ui.add_space(10.0);
                    if self.settings.ui_mode == Mode::NetAccess {
                        if appearance::sidebar_row(ui, !self.netaccess.setup, "Users & fobs")
                            .clicked()
                        {
                            self.netaccess.setup = false;
                        }
                        if appearance::sidebar_row(ui, self.netaccess.setup, "Device setup")
                            .clicked()
                        {
                            self.netaccess.setup = true;
                        }
                    } else if self.settings.ui_mode == Mode::Basic {
                        for (route, label) in [
                            (basic::Route::Home, "Home"),
                            (basic::Route::Read, "Read"),
                            (basic::Route::Clone, "Clone"),
                            (basic::Route::Write, "Write"),
                            (basic::Route::Library, "Library"),
                            (basic::Route::History, "History"),
                            (basic::Route::Setup, "Device setup"),
                        ] {
                            if route == basic::Route::Library {
                                ui.add_space(22.0);
                                ui.label(
                                    RichText::new("Collection")
                                        .size(11.0)
                                        .color(appearance::secondary(ui)),
                                );
                                ui.add_space(4.0);
                            }
                            if route == basic::Route::Setup {
                                ui.add_space(22.0);
                            }
                            if appearance::sidebar_row(ui, self.basic.route == route, label)
                                .clicked()
                            {
                                if self.worker.is_none()
                                    && self.basic.route != route
                                    && matches!(
                                        route,
                                        basic::Route::Read
                                            | basic::Route::Clone
                                            | basic::Route::Write
                                    )
                                {
                                    self.basic.flow.step = basic::StepView::Source;
                                    self.basic.flow.invalidate();
                                }
                                self.basic.route = route;
                            }
                        }
                    } else {
                        for (page, label) in [
                            (Page::Overview, "Overview"),
                            (Page::Tags, "Tag tools"),
                            (Page::Credentials, "Credentials & batch"),
                            (Page::Desfire, "DESFire"),
                            (Page::Workbench, "Card memory"),
                            (Page::Classic, "Classic cards"),
                            (Page::Tools, "Advanced tools"),
                            (Page::Console, "Console"),
                            (Page::Profiles, "Saved profiles"),
                            (Page::Connection, "Device setup"),
                            (Page::Maintenance, "Client & firmware"),
                        ] {
                            if page == Page::Console || page == Page::Connection {
                                ui.add_space(20.0);
                            }
                            if appearance::sidebar_row(ui, self.page == page, label).clicked() {
                                self.page = page;
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                        ui.label(
                            RichText::new(concat!("Version ", env!("CARGO_PKG_VERSION")))
                                .small()
                                .color(appearance::secondary(ui)),
                        );
                        if ui.small_button("About").clicked() {
                            self.show_help = true;
                        }
                        ui.label(
                            RichText::new("EV2 / LRP / EV3-specific\noperations unavailable")
                                .small()
                                .color(appearance::secondary(ui)),
                        );
                    });
                });
        }
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.faint_bg_color)
                    .inner_margin(egui::Margin::symmetric(16, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if let Some(w) = &self.worker {
                        if ui.button("Stop operation").clicked() {
                            self.tools.live = false;
                            w.cancel();
                        }
                        ui.spinner();
                    }
                    ui.label(
                        RichText::new(if self.worker.is_some() && !self.basic.stage.is_empty() {
                            &self.basic.stage
                        } else {
                            &self.status
                        })
                        .size(12.0)
                        .color(if self.status_ok {
                            ui.visuals().selection.stroke.color
                        } else {
                            ui.visuals().warn_fg_color
                        }),
                    );
                });
            });
        if self.show_logs {
            egui::TopBottomPanel::bottom("logs")
                .default_height(185.0)
                .height_range(100.0..=400.0)
                .resizable(true)
                .frame(
                    egui::Frame::new()
                        .fill(ctx.style().visuals.extreme_bg_color)
                        .inner_margin(12),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("OPERATION DETAILS")
                                .small()
                                .color(appearance::secondary(ui)),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.log_filter)
                                .hint_text("Filter output")
                                .desired_width(170.0),
                        );
                        if ui.small_button("Clear log").clicked() {
                            self.logs.clear();
                        }
                        if ui.small_button("Export log…").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name("proxmark-studio.log")
                                .save_file()
                            {
                                let text = self.logs.iter().cloned().collect::<Vec<_>>().join("\n");
                                if let Err(e) = storage::write_private(&path, text.as_bytes()) {
                                    self.fail(e);
                                }
                            }
                        }
                        if ui.small_button("Close").clicked() {
                            self.show_logs = false;
                        }
                    });
                    let filtered = self
                        .logs
                        .iter()
                        .filter(|s| {
                            self.log_filter.is_empty()
                                || s.to_lowercase().contains(&self.log_filter.to_lowercase())
                        })
                        .collect::<Vec<_>>();
                    egui::ScrollArea::both()
                        .id_salt("live_log")
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show_rows(ui, 16.0, filtered.len(), |ui, range| {
                            for index in range {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(filtered[index])
                                            .monospace()
                                            .size(12.0)
                                            .color(appearance::secondary(ui)),
                                    )
                                    .extend(),
                                );
                            }
                        });
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(ctx.style().visuals.panel_fill).inner_margin(32))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt((
                        "page",
                        self.settings.ui_mode == Mode::Basic,
                        self.page,
                        self.basic.route,
                        self.desfire_tab,
                        self.credential_tab,
                    ))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width().min(1040.0));
                        ui.add_space(12.0);
                        if self.settings.sealed_library.is_some()&&self.library_passphrase.is_empty(){ui.horizontal(|ui|{ui.label("Saved card and emulator libraries are locked.");if ui.button("Unlock libraries…").clicked(){self.show_settings=true;}});}
                        if self.settings.rf_may_be_active {
                            ui.colored_label(ui.visuals().warn_fg_color,"RF simulation may be active or its state is unknown. Other device operations are paused.");
                            ui.horizontal_wrapped(|ui| {
                                if ui.add_enabled(self.worker.is_none(),egui::Button::new("Send RF stop request")).clicked(){self.start(Job::Emulator(proxmark_studio::emulator::Request{action:proxmark_studio::emulator::Action::Stop,caps:self.caps.clone(),simulated:self.simulation.is_some(),acknowledged:true}),"Request RF stop");}
                                if ui.add_enabled(self.worker.is_none(),egui::Button::new("I have stopped the device with its button")).clicked(){self.settings.rf_may_be_active=false;self.save();self.status="RF stopped according to your confirmation. No automatic RF inactivity claim is made.".into();}
                            });
                            ui.label("The client sends Stop without an acknowledgement. Use the physical button to establish a stopped device. Demo: the stop request clears the simulated RF state.");
                        }
                        ui.add_enabled_ui(self.worker.is_none()&&!self.settings.rf_may_be_active, |ui| self.demo_controls(ui));
                        if self.simulation.is_some() {
                            ui.add_space(12.0);
                        }
                        ui.add_enabled_ui(self.worker.is_none(), |ui| {
                            if self.settings.ui_mode == Mode::NetAccess {
                                if self.netaccess.setup {
                                    self.basic.route = basic::Route::Setup;
                                    self.basic_page(ui);
                                } else { self.netaccess_page(ui); }
                            } else if self.settings.ui_mode == Mode::Basic {
                                self.basic_page(ui);
                            } else {
                                match self.page {
                                    Page::Overview => self.overview(ui),
                                    Page::Tags => self.tags(ui),
                                    Page::Credentials => self.credential_workspace(ui),
                                    Page::Desfire => self.desfire(ui),
                                    Page::Console => self.console_page(ui),
                                    Page::Profiles => self.profiles(ui),
                                    Page::Connection => self.connection(ui),
                                    Page::Workbench => self.workbench_page(ui),
                                    Page::Classic => self.classic_page(ui),
                                    Page::Maintenance => self.maintenance_page(ui),
                                    Page::Tools => self.tools_page(ui),
                                }
                            }
                        });
                    });
            });
        if self.review.is_some() {
            let mut commit = false;
            let mut dismiss = false;
            egui::Modal::new(egui::Id::new("write_review")).show(ctx, |ui| {
                ui.heading(review_labels(&self.review.as_ref().unwrap().job).0);
                ui.set_max_width(500.0);
                ui.label(&self.review.as_ref().unwrap().summary);
                ui.add_space(8.0);
                ui.checkbox(
                    &mut self.review_ack,
                    "I have reviewed this operation, its target and its effects.",
                );
                ui.horizontal(|ui| {
                    commit = Self::action(
                        ui,
                        self.review_ack,
                        review_labels(&self.review.as_ref().unwrap().job).1,
                    );
                    dismiss = ui.button("Cancel").clicked();
                });
            });
            if commit {
                let r = self.review.take().unwrap();
                let label = review_labels(&r.job).2;
                self.start(r.job, label);
            } else if dismiss {
                self.review = None;
            }
        }
        if self.show_help {
            egui::Window::new("About Proxmark Studio").open(&mut self.show_help).show(ctx,|ui|{ui.heading(concat!("Proxmark Studio ", env!("CARGO_PKG_VERSION")));ui.label("An independent desktop front end for RRG/Iceman. No firmware or client is bundled. Hardware is opened by the CLI for each operation.");ui.label("Keys are held in zeroising app buffers and sent over stdin or a private terminal with --incognito. The external client and OS can still retain memory copies. Named keys can be saved explicitly in macOS Keychain or Windows Credential Manager.");ui.label("Guided writes compare a fresh read byte-for-byte. A parser mismatch is inconclusive, never success. A verified byte range is not a complete clone or a credential validation.");ui.hyperlink_to("RRG/Iceman project","https://github.com/RfidResearchGroup/proxmark3");});
        }
    }
}

impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.render(ctx);
        self.settings_window(ctx);
    }
}

fn review_labels(job: &Job) -> (&'static str, &'static str, &'static str) {
    match job {
        Job::Recover { .. } => (
            "Review key recovery",
            "Recover keys",
            "Recover Classic keys",
        ),
        Job::Magic { request, .. } => match request.action {
            proxmark_studio::magic::Action::Clone { .. } => (
                "Review full-card clone",
                "Clone & verify",
                "Clone magic card",
            ),
            _ => (
                "Inspect destination",
                "Inspect",
                "Inspect magic destination",
            ),
        },
        Job::ExplorerRun { .. } => (
            "Review client command",
            "Run reviewed command",
            "Run reviewed command",
        ),
        Job::Emulator(r) => match r.action {
            proxmark_studio::emulator::Action::Simulate(_) => (
                "Review RF simulation",
                "Request RF simulation",
                "Request RF simulation",
            ),
            _ => (
                "Review emulator buffer replacement",
                "Load & verify buffer",
                "Load & verify buffer",
            ),
        },
        _ => (
            "Review programming operation",
            "Programme & verify",
            "Programme & verify",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_tools_tabs_render_at_minimum_size() {
        let ctx = egui::Context::default();
        let dir = tempfile::tempdir().unwrap();
        let mut app = Studio::with_config(&ctx, Some(dir.path().join("settings.json")));
        app.settings.ui_mode = Mode::Advanced;
        app.page = Page::Tools;
        for tab in 0..4 {
            app.tools.tab = tab;
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(960.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.render(ctx),
            );
            assert!(out.shapes.len() > 10);
        }
    }
    #[test]
    fn native_pages_produce_egui_shapes_at_both_window_sizes() {
        for size in [Vec2::new(960.0, 700.0), Vec2::new(1280.0, 860.0)] {
            let ctx = egui::Context::default();
            let mut app = Studio::with_context(&ctx);
            app.settings = Settings::default();
            app.settings.ui_mode = Mode::Advanced;
            for page in [
                Page::Overview,
                Page::Tags,
                Page::Credentials,
                Page::Desfire,
                Page::Console,
                Page::Profiles,
                Page::Connection,
                Page::Workbench,
                Page::Maintenance,
                Page::Tools,
            ] {
                app.page = page;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        ..Default::default()
                    },
                    |ctx| app.render(ctx),
                );
                assert!(output.shapes.len() > 10);
            }
        }
    }
    #[test]
    fn basic_routes_and_specialist_tabs_render_at_minimum_size() {
        let ctx = egui::Context::default();
        let mut app = Studio::with_context(&ctx);
        app.settings = Settings::default();
        app.basic.flow.set_source(basic::Card {
            content: basic::Content::Em410x("0102030405".into()),
            captured_at: 1,
            simulated: true,
        });
        for route in [
            basic::Route::Home,
            basic::Route::Read,
            basic::Route::Clone,
            basic::Route::Write,
            basic::Route::Library,
            basic::Route::History,
            basic::Route::Setup,
        ] {
            app.basic.route = route;
            for step in [
                basic::StepView::Source,
                basic::StepView::Target,
                basic::StepView::Review,
                basic::StepView::Result,
            ] {
                app.basic.flow.step = step;
                app.basic.flow.destination = Some(basic::Destination::T55 {
                    target: Target::T5577,
                    block0: "000880E0".into(),
                });
                let out = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(960.0, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.render(ctx),
                );
                assert!(out.shapes.len() > 10);
            }
        }
        app.settings.ui_mode = Mode::Advanced;
        for page in [Page::Credentials, Page::Desfire] {
            app.page = page;
            for tab in 0..6 {
                app.credential_tab = tab;
                app.desfire_tab = tab.min(3);
                let out = ctx.run(egui::RawInput::default(), |ctx| app.render(ctx));
                assert!(out.shapes.len() > 10);
            }
        }
    }
    #[test]
    fn credential_profile_load_updates_fields_without_running_a_command() {
        let ctx = egui::Context::default();
        let mut app = Studio::with_context(&ctx);
        let profile = CredentialProfile {
            name: "Lab".into(),
            credential: Credential {
                format: Format::H10304,
                facility_code: 65535,
                card_number: 524287,
            },
            target: Target::Em4305,
        };
        app.load_credential_profile(&profile);
        assert_eq!(app.current_credential().unwrap(), profile.credential);
        assert_eq!(app.credential_target, Target::Em4305);
        assert!(app.worker.is_none());
        let invalid = CredentialProfile {
            credential: Credential {
                format: Format::H10301,
                facility_code: 256,
                card_number: 1,
            },
            ..profile.clone()
        };
        app.load_credential_profile(&invalid);
        assert_eq!(app.current_credential().unwrap(), profile.credential);
        assert!(!app.status_ok);
    }
    #[test]
    fn specialist_pages_render_in_both_appearances_with_large_text() {
        for theme in [
            proxmark_studio::appearance::Appearance::Light,
            proxmark_studio::appearance::Appearance::Dark,
        ] {
            let ctx = egui::Context::default();
            let dir = tempfile::tempdir().unwrap();
            let mut app = Studio::with_config(&ctx, Some(dir.path().join("settings.json")));
            app.settings.ui_preferences.appearance = theme;
            app.settings.ui_preferences.text_size = 20.0;
            app.settings.ui_mode = Mode::Advanced;
            appearance::apply(&ctx, &app.settings.ui_preferences);
            for (page, tab) in [(Page::Classic, 0), (Page::Classic, 1), (Page::Tools, 3)] {
                app.page = page;
                app.classic.tab = tab;
                app.tools.tab = tab;
                let out = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(960.0, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.render(ctx),
                );
                assert!(out.shapes.len() > 10);
            }
        }
    }
    #[test]
    fn locking_libraries_persists_ciphertext_and_keeps_envelope_on_settings_save() {
        let ctx = egui::Context::default();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut app = Studio::with_config(&ctx, Some(path.clone()));
        let image = proxmark_studio::magic::image(
            proxmark_studio::backup::Dump::new(
                proxmark_studio::backup::Geometry::Classic1K,
                "01020304",
                proxmark_studio::mock_specialist::card_bytes(false),
            )
            .unwrap(),
            true,
        );
        app.settings.emulator_images.push(image.clone());
        app.library_passphrase = Zeroizing::new("synthetic test passphrase".into());
        app.settings.sealed_library =
            Some(proxmark_studio::sealed::seal(b"{}", &app.library_passphrase).unwrap());
        app.lock_library();
        assert!(app.library_passphrase.is_empty());
        assert!(app.settings.emulator_images.is_empty());
        let envelope = app.settings.sealed_library.clone().unwrap();
        app.settings.ui_preferences.show_sidebar = false;
        app.persist_ui().unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("01020304"));
        assert!(!saved.contains("FFFFFFFFFFFF"));
        assert!(!saved.contains("synthetic test passphrase"));
        let parsed: Settings = serde_json::from_str(&saved).unwrap();
        assert_eq!(parsed.sealed_library, Some(envelope.clone()));
        let plain = proxmark_studio::sealed::open(&envelope, "synthetic test passphrase").unwrap();
        let library: proxmark_studio::sealed::Library = serde_json::from_slice(&plain).unwrap();
        assert_eq!(library.images, vec![image]);
        assert!(proxmark_studio::sealed::open(&envelope, "wrong test passphrase").is_err());
    }
    #[test]
    fn appearance_follows_system_changes_and_explicit_overrides() {
        let ctx = egui::Context::default();
        let mut p = proxmark_studio::appearance::Preferences::default();
        appearance::apply(&ctx, &p);
        for (input, expected) in [
            (egui::Theme::Light, false),
            (egui::Theme::Dark, true),
            (egui::Theme::Light, false),
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    system_theme: Some(input),
                    ..Default::default()
                },
                |ctx| {
                    assert_eq!(ctx.style().visuals.dark_mode, expected);
                },
            );
        }
        p.appearance = proxmark_studio::appearance::Appearance::Dark;
        appearance::apply(&ctx, &p);
        let _ = ctx.run(
            egui::RawInput {
                system_theme: Some(egui::Theme::Light),
                ..Default::default()
            },
            |ctx| assert!(ctx.style().visuals.dark_mode),
        );
        p.appearance = proxmark_studio::appearance::Appearance::Light;
        appearance::apply(&ctx, &p);
        let _ = ctx.run(
            egui::RawInput {
                system_theme: Some(egui::Theme::Dark),
                ..Default::default()
            },
            |ctx| assert!(!ctx.style().visuals.dark_mode),
        );
    }
}
