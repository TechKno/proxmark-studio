use crate::{
    capabilities::{Capabilities, COMMANDS},
    command::{CommandSpec, Kind},
    credential::{self, Credential, Format},
    model::{Evidence, Settings},
    parse,
};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

pub enum Event {
    Log(String),
    Finished(Box<JobResult>),
}
pub struct JobResult {
    pub outcome: Outcome,
    pub evidence: Evidence,
    pub capabilities: Option<Capabilities>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Completed,
    Failed(String),
    Cancelled,
    TimedOut,
    Verified,
    Mismatch,
}
pub enum Job {
    Recover {
        request: crate::recovery::Request,
        caps: Capabilities,
    },
    CaptureClassic {
        keys: crate::recovery::Keys,
        simulated: bool,
        caps: Capabilities,
    },
    Magic {
        request: crate::magic::Request,
        caps: Capabilities,
    },
    Signal(crate::acquisition::Request),
    CommandCatalogue,
    CommandHelp(String),
    ExplorerRun {
        spec: CommandSpec,
        device: bool,
    },
    Emulator(crate::emulator::Request),
    Guided(Box<crate::basic::Request>),
    Probe,
    ClassicTrailer(crate::classic::TrailerRequest),
    FormatCatalogue,
    FormatEncode(crate::format_catalogue::Credential),
    FormatProgramme(crate::format_catalogue::Credential),
    FormatDecode(String),
    Desfire(Box<crate::desfire_archive::Request>),
    ProgrammeLf(crate::lf::Credential),
    Bound {
        uid: String,
        job: Box<Job>,
    },
    ProgrammeRaw {
        write: CommandSpec,
        expected: String,
    },
    Batch(crate::backup::Batch),
    ReadCredential {
        format: Format,
    },
    VerifyCredential {
        expected: Credential,
    },
    ProgrammeCredential {
        write: CommandSpec,
        expected: Credential,
    },
    Run(CommandSpec),
    Verify {
        read: CommandSpec,
        expected: Vec<u8>,
    },
    Programme {
        write: CommandSpec,
        read: CommandSpec,
        expected: Vec<u8>,
    },
}
pub struct Worker {
    pub rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}
impl Worker {
    pub fn start(settings: Settings, job: Job) -> Self {
        let (tx, rx) = mpsc::sync_channel(512);
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        thread::spawn(move || {
            let result = execute(&settings, job, &tx, &c);
            let _ = tx.send(Event::Finished(Box::new(result)));
        });
        Self { rx, cancel }
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
    }
}
fn log(tx: &SyncSender<Event>, line: String) {
    let _ = tx.try_send(Event::Log(line));
}
fn result(outcome: Outcome, evidence: Evidence) -> JobResult {
    JobResult {
        outcome,
        evidence,
        capabilities: None,
    }
}

fn execute(
    settings: &Settings,
    job: Job,
    tx: &SyncSender<Event>,
    cancel: &AtomicBool,
) -> JobResult {
    match job {
        Job::Recover { request, caps } => {
            let mut extended = settings.clone();
            extended.timeout_secs = extended.timeout_secs.max(900);
            let mut c = ArchiveClient {
                settings: &extended,
                tx,
                cancel,
                caps: &caps,
            };
            match crate::recovery::recover(&mut c, request) {
                Ok((o, e)) => result(o, e),
                Err(e) => result(e, Evidence::default()),
            }
        }
        Job::CaptureClassic {
            keys,
            simulated,
            caps,
        } => {
            let mut c = ArchiveClient {
                settings,
                tx,
                cancel,
                caps: &caps,
            };
            match crate::recovery::capture(&mut c, keys, simulated) {
                Ok((o, e)) => result(o, e),
                Err(e) => result(e, Evidence::default()),
            }
        }
        Job::Magic { request, caps } => {
            let mut c = ArchiveClient {
                settings,
                tx,
                cancel,
                caps: &caps,
            };
            match crate::magic::execute(&mut c, request) {
                Ok((o, e)) => result(o, e),
                Err(e) => result(e, Evidence::default()),
            }
        }
        Job::Signal(request) => run_signal(settings, request, tx, cancel),
        Job::CommandCatalogue => {
            let spec = CommandSpec::simple("XX_internal_command_dump_XX", Kind::Help);
            let (out, raw, mut e) = run_process(settings, &spec, false, tx, cancel);
            if out != Outcome::Completed {
                return result(out, e);
            }
            match crate::explorer::parse_catalogue(&raw) {
                Ok(entries) => {
                    e.command_catalogue = entries;
                    result(out, e)
                }
                Err(err) => result(Outcome::Failed(err), e),
            }
        }
        Job::CommandHelp(name) => {
            let spec = match crate::explorer::help_command(&name) {
                Ok(s) => s,
                Err(e) => return result(Outcome::Failed(e), Evidence::default()),
            };
            let (out, raw, mut e) = run_process(settings, &spec, false, tx, cancel);
            if out == Outcome::Completed {
                e.command_help = Some((
                    name,
                    raw.lines()
                        .map(crate::parse::clean)
                        .collect::<Vec<_>>()
                        .join("\n"),
                ));
            }
            result(out, e)
        }
        Job::ExplorerRun { spec, device } => {
            if let Err(e) = crate::command::validate_line(&spec.text) {
                return result(Outcome::Failed(e), Evidence::default());
            }
            if spec.text.contains(';') {
                return result(
                    Outcome::Failed("The explorer runs one command at a time".into()),
                    Evidence::default(),
                );
            }
            if spec.text.split_whitespace().take(3).eq(["hf", "mf", "sim"]) {
                return result(
                    Outcome::Failed(
                        "Use the managed emulator workspace for Classic RF simulation".into(),
                    ),
                    Evidence::default(),
                );
            }
            if device {
                let (out, raw, e) = run_process(
                    settings,
                    &CommandSpec::simple("hw version", Kind::Device),
                    true,
                    tx,
                    cancel,
                );
                if out != Outcome::Completed {
                    return result(out, e);
                }
                if !matches!(
                    crate::maintenance::compatibility(&raw),
                    crate::maintenance::Compatibility::Matched(_)
                ) {
                    return result(
                        Outcome::Failed(
                            "Check matching client and firmware before running device commands"
                                .into(),
                        ),
                        e,
                    );
                }
            }
            let (out, _, e) = run_process(settings, &spec, device, tx, cancel);
            result(out, e)
        }
        Job::Emulator(request) => {
            let caps = request.caps.clone();
            let mut client = ArchiveClient {
                settings,
                tx,
                cancel,
                caps: &caps,
            };
            match crate::emulator::execute(&mut client, request) {
                Ok((out, e)) => result(out, e),
                Err(out) => result(out, Evidence::default()),
            }
        }
        Job::Guided(request) => {
            let mut client = GuidedClient {
                settings,
                tx,
                cancel,
                caps: &request.caps,
            };
            crate::basic::execute(&mut client, request.action, request.simulated)
        }
        Job::ClassicTrailer(request) => {
            let caps = request.capabilities.clone();
            let mut client = ArchiveClient {
                settings,
                tx,
                cancel,
                caps: &caps,
            };
            match crate::classic::programme_trailer(&mut client,request){Ok(())=>result(Outcome::Verified,Evidence{verification:Some("fresh Classic key authentication, access bytes and observable trailer fields match".into()),..Default::default()}),Err(e)=>result(e,Evidence::default())}
        }
        Job::FormatCatalogue => {
            let (out, raw, mut e) = run_process(
                settings,
                &CommandSpec::simple("wiegand list", Kind::Help),
                false,
                tx,
                cancel,
            );
            if out != Outcome::Completed {
                return result(out, e);
            }
            match crate::format_catalogue::list(&raw) {
                Ok(list) => {
                    e.format_catalogue = list;
                    result(Outcome::Completed, e)
                }
                Err(err) => result(Outcome::Failed(err), e),
            }
        }
        Job::FormatDecode(raw) => {
            let raw = match crate::credential::normalise_raw(&raw) {
                Ok(s) => s,
                Err(e) => return result(Outcome::Failed(e), Evidence::default()),
            };
            let spec = CommandSpec::new(
                format!("wiegand decode -r {raw}"),
                "Decode Wiegand candidates",
                false,
                Kind::Help,
            )
            .unwrap();
            let (out, raw, mut e) = run_process(settings, &spec, false, tx, cancel);
            if out == Outcome::Completed {
                e.decoded_wiegand = Some(
                    raw.lines()
                        .map(crate::parse::clean)
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            result(out, e)
        }
        Job::FormatEncode(value) => encode_format(settings, value, false, tx, cancel),
        Job::FormatProgramme(value) => encode_format(settings, value, true, tx, cancel),
        Job::Desfire(request) => run_archive(settings, *request, tx, cancel),
        Job::ProgrammeLf(value) => {
            let write = match value.write() {
                Ok(s) => s,
                Err(e) => return result(Outcome::Failed(e), Evidence::default()),
            };
            let (out, _, e) = run_process(settings, &write, true, tx, cancel);
            if out != Outcome::Completed {
                return result(out, e);
            }
            let (out, raw, mut e) = run_process(settings, &value.read(), true, tx, cancel);
            if out != Outcome::Completed {
                return result(out, e);
            }
            result(
                if value.matches(&raw) {
                    e.verification = Some(
                        "decoded LF format, facility/card fields and applicable checksum match"
                            .into(),
                    );
                    Outcome::Verified
                } else {
                    Outcome::Mismatch
                },
                e,
            )
        }
        Job::Bound { uid, job } => {
            let check = || {
                let (out, raw, _) = run_process(
                    settings,
                    &CommandSpec::simple("hf 14a info", Kind::Detect),
                    true,
                    tx,
                    cancel,
                );
                if out != Outcome::Completed {
                    return Err(out);
                }
                if crate::backup::uid(&raw).as_deref() != Some(uid.as_str()) {
                    return Err(Outcome::Failed(
                        "Target UID changed or is unreadable; stopped".into(),
                    ));
                }
                Ok(())
            };
            if let Err(out) = check() {
                return result(out, Evidence::default());
            }
            let r = execute(settings, *job, tx, cancel);
            if matches!(r.outcome, Outcome::Completed | Outcome::Verified) {
                if let Err(out) = check() {
                    return result(out, r.evidence);
                }
            }
            r
        }
        Job::ProgrammeRaw { write, expected } => {
            let (out, _, e) = run_process(settings, &write, true, tx, cancel);
            if out != Outcome::Completed {
                return result(out, e);
            }
            let (out, _, e) = run_process(settings, &credential::read_command(), true, tx, cancel);
            if out != Outcome::Completed {
                return result(out, e);
            }
            result(
                if e.hid_raw.len() == 1 && e.hid_raw[0] == expected {
                    Outcome::Verified
                } else {
                    Outcome::Mismatch
                },
                e,
            )
        }
        Job::Batch(batch) => execute_batch(settings, batch, tx, cancel),
        Job::Probe => {
            log(tx, format!("CLI executable: {}", settings.cli));
            log(
                tx,
                format!(
                    "CLI working folder: {}",
                    if settings.working_dir.is_empty() {
                        "(inherited)"
                    } else {
                        &settings.working_dir
                    }
                ),
            );
            let mut caps = Capabilities::default();
            for cmd in COMMANDS {
                if cancel.load(Ordering::SeqCst) {
                    return result(Outcome::Cancelled, Evidence::default());
                }

                let spec = CommandSpec::simple(&format!("{cmd} -h"), Kind::Help);
                let (out, raw, _) = run_process(settings, &spec, false, tx, cancel);
                if matches!(out, Outcome::Cancelled | Outcome::TimedOut) {
                    return result(out, Evidence::default());
                }
                if out == Outcome::Completed {
                    caps.record(cmd, &raw);
                } else {
                    return result(out, Evidence::default());
                }
            }
            log(
                tx,
                format!(
                    "CLI inspection complete: {} / {} guided commands advertised",
                    caps.help.len(),
                    COMMANDS.len()
                ),
            );
            JobResult {
                outcome: Outcome::Completed,
                evidence: Evidence::default(),
                capabilities: Some(caps),
            }
        }
        Job::ReadCredential { format } => credential_read(settings, format, None, tx, cancel),
        Job::VerifyCredential { expected } => {
            credential_read(settings, expected.format, Some(&expected), tx, cancel)
        }
        Job::ProgrammeCredential { write, expected } => {
            if let Err(e) = expected.validate() {
                return result(Outcome::Failed(e), Evidence::default());
            }
            let (out, _, e) = run_process(settings, &write, true, tx, cancel);
            if out != Outcome::Completed {
                return result(out, e);
            }
            if cancel.load(Ordering::SeqCst) {
                return result(Outcome::Cancelled, e);
            }
            log(tx,"Write command ended. Starting a fresh HID read for credential and raw-data verification…".into());
            credential_read(settings, expected.format, Some(&expected), tx, cancel)
        }
        Job::Run(spec) => {
            let (out, _, e) = run_process(settings, &spec, spec.kind != Kind::Help, tx, cancel);
            result(out, e)
        }
        Job::Verify { read, expected } => verify(settings, &read, &expected, tx, cancel),
        Job::Programme {
            write,
            read,
            expected,
        } => {
            let (out, _, e) = run_process(settings, &write, true, tx, cancel);
            if out != Outcome::Completed {
                return result(out, e);
            }
            if cancel.load(Ordering::SeqCst) {
                return result(Outcome::Cancelled, e);
            }
            log(
                tx,
                "Write command ended. Reading the same target back for byte comparison…".into(),
            );
            verify(settings, &read, &expected, tx, cancel)
        }
    }
}
fn credential_read(
    settings: &Settings,
    format: Format,
    expected: Option<&Credential>,
    tx: &SyncSender<Event>,
    cancel: &AtomicBool,
) -> JobResult {
    if let Some(value) = expected {
        if let Err(e) = value.validate() {
            return result(Outcome::Failed(e), Evidence::default());
        }
    }
    let (out, _, mut evidence) =
        run_process(settings, &credential::read_command(), true, tx, cancel);
    if out != Outcome::Completed {
        return result(out, evidence);
    }
    if evidence.hid_raw.len() != 1 {
        return result(Outcome::Failed("No single unambiguous HID raw value was read. Credential verification is inconclusive.".into()),evidence);
    }
    let decoded = match credential::decode_hid(format, &evidence.hid_raw[0]) {
        Ok(d) => d,
        Err(e) => return result(Outcome::Failed(e), evidence),
    };
    let parity_valid = decoded.parity_valid;
    let matches = expected.is_none_or(|value| {
        decoded.credential == *value
            && value
                .encode()
                .is_ok_and(|encoded| encoded == decoded.encoded)
    });
    evidence.credential = Some(decoded);
    if !parity_valid {
        return result(
            Outcome::Failed(
                "Selected credential format has invalid parity; verification failed".into(),
            ),
            evidence,
        );
    }
    evidence.tag_seen = true;
    result(
        if expected.is_none() {
            Outcome::Completed
        } else if matches {
            Outcome::Verified
        } else {
            Outcome::Mismatch
        },
        evidence,
    )
}

fn verify(
    settings: &Settings,
    read: &CommandSpec,
    expected: &[u8],
    tx: &SyncSender<Event>,
    cancel: &AtomicBool,
) -> JobResult {
    let (out, _, e) = run_process(settings, read, true, tx, cancel);
    if out != Outcome::Completed {
        return result(out, e);
    }
    if e.bytes.is_empty() {
        return result(Outcome::Failed("No recognised read-back bytes. Verification is inconclusive; inspect the CLI/version.".into()),e);
    }
    // Exact length is essential. A prefix match is not verification.
    let outcome = if e.bytes == expected {
        Outcome::Verified
    } else {
        Outcome::Mismatch
    };
    result(outcome, e)
}

fn reader<R: std::io::Read + Send + 'static>(
    pipe: R,
    tx: SyncSender<String>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        // Cap each line as well as total output; a broken CLI must not exhaust memory.
        let mut bytes = Vec::new();
        loop {
            use std::io::Read;
            bytes.clear();
            let n = reader
                .by_ref()
                .take(16384)
                .read_until(b'\n', &mut bytes)
                .unwrap_or(0);
            if n == 0 {
                break;
            }
            let line = String::from_utf8_lossy(&bytes).into_owned();
            bytes.zeroize();
            if tx.send(line).is_err() {
                break;
            }
        }
    })
}
pub fn validate_cli_settings(settings: &Settings) -> Result<(), String> {
    if settings.cli.trim().is_empty() {
        return Err("Choose a CLI executable first".into());
    }
    if let Ok(path) = std::fs::canonicalize(&settings.cli) {
        if !path.is_file() {
            return Err("The CLI path is a folder. Select the native proxmark3 or mock-pm3 executable inside it.".into());
        }
        if std::env::current_exe()
            .ok()
            .and_then(|exe| std::fs::canonicalize(exe).ok())
            .as_ref()
            == Some(&path)
        {
            return Err(
                "The selected CLI is Proxmark Studio itself. Select proxmark3 or mock-pm3 instead."
                    .into(),
            );
        }
    }
    if !settings.working_dir.trim().is_empty()
        && !std::path::Path::new(&settings.working_dir).is_dir()
    {
        return Err("The CLI working folder does not exist or is not a folder".into());
    }
    Ok(())
}
fn run_process(
    settings: &Settings,
    spec: &CommandSpec,
    device: bool,
    tx: &SyncSender<Event>,
    cancel: &AtomicBool,
) -> (Outcome, Zeroizing<String>, Evidence) {
    if spec.kind == Kind::Write {
        let (out, raw, e) = run_process(
            settings,
            &CommandSpec::simple("hw version", Kind::Device),
            true,
            tx,
            cancel,
        );
        if out != Outcome::Completed {
            return (out, raw, e);
        }
        if e.version_mismatch {
            return (
                Outcome::Failed("Client and firmware mismatch; guided writes disabled".into()),
                raw,
                e,
            );
        }
        if matches!(
            crate::maintenance::compatibility(&raw),
            crate::maintenance::Compatibility::Unknown
        ) {
            log(tx,"Version compatibility could not be established from CLI output. No matching-version claim is made.".into());
        }
    }
    if spec.text.len() > 254 && spec.text.len() <= 1000 {
        return run_terminal_once(settings, spec, device, tx, cancel);
    }
    let empty = || Zeroizing::new(String::new());
    if let Err(e) = validate_cli_settings(settings) {
        return (Outcome::Failed(e), empty(), Evidence::default());
    }
    if device && settings.port.trim().is_empty() {
        return (
            Outcome::Failed("Select a serial port first".into()),
            empty(),
            Evidence::default(),
        );
    }
    if !matches!(spec.kind, Kind::Help | Kind::Recovery) {
        log(tx, format!("→ {}", spec.label));
    }
    let mut bridge = None;
    if spec.text.len() > 1000 {
        if spec.text.contains(';') {
            return (Outcome::Failed("Long one-shot commands must be single commands; use the interactive console for sequences".into()),empty(),Evidence::default());
        }
        let mut file = match tempfile::Builder::new()
            .prefix("studio-command-bridge-")
            .suffix(".lua")
            .tempfile()
        {
            Ok(f) => f,
            Err(e) => return (Outcome::Failed(e.to_string()), empty(), Evidence::default()),
        };
        let source=br#"local line=io.read('*l')
if not line or #line>4096 then error('Studio input length invalid') end
local probe=core.console('hw version -h',true,true)
if type(probe)~='string' or not probe:find('hw version',1,true) then error('Studio Lua capture unsupported') end
local output=core.console(line,true,true)
if type(output)~='string' then error('Studio capture result missing') end
io.write(output)
io.write('\n[STUDIO_BRIDGE_DONE]\n')
"#;
        if let Err(e) = file.write_all(source) {
            return (Outcome::Failed(e.to_string()), empty(), Evidence::default());
        }
        bridge = Some(file);
    }
    let mut cmd = Command::new(&settings.cli);
    cmd.args(["--incognito", "-f"]);
    if let Some(file) = &bridge {
        cmd.arg("-l").arg(file.path());
    }
    if device {
        cmd.arg("-p").arg(&settings.port);
    }
    if !settings.working_dir.trim().is_empty() {
        cmd.current_dir(&settings.working_dir);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    crate::process_tree::isolate(&mut cmd);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return (
                Outcome::Failed(format!("Cannot start configured CLI: {e}")),
                empty(),
                Evidence::default(),
            )
        }
    };
    if spec.kind == Kind::Help {
        log(
            tx,
            format!("Inspecting {} (PID {})", spec.label, child.id()),
        );
    } else {
        log(tx, format!("CLI process started (PID {})", child.id()));
    }
    let (line_tx, line_rx) = mpsc::sync_channel(256);
    let stdout = reader(child.stdout.take().unwrap(), line_tx.clone());
    let stderr = reader(child.stderr.take().unwrap(), line_tx);
    // Write on a separate thread: blocked stdin cannot freeze cancellation/UI.
    let mut input = Zeroizing::new(spec.text.to_string());
    input.push_str("\nquit\n");
    let mut stdin = child.stdin.take().unwrap();
    let (input_tx, input_rx) = mpsc::sync_channel(1);
    let writer = thread::spawn(move || {
        let res = stdin.write_all(input.as_bytes());
        drop(stdin);
        let _ = input_tx.send(res.map_err(|e| e.to_string()));
    });
    let start = Instant::now();
    let mut raw = empty();
    let mut outcome = Outcome::Completed;
    let mut exited = false;
    let mut sensitive_notice = false;
    let mut input_reported = false;
    loop {
        if !input_reported {
            if let Ok(result) = input_rx.try_recv() {
                input_reported = true;
                match result {
                    Ok(()) => {
                        if !matches!(spec.kind, Kind::Help | Kind::Recovery) {
                            log(tx, "CLI stdin delivered and closed".into());
                        }
                    }
                    Err(e) => {
                        outcome = Outcome::Failed(format!("Cannot deliver CLI input: {e}"));
                        crate::process_tree::terminate(child.id());
                        let _ = child.kill();
                        break;
                    }
                }
            }
        }
        for mut line in line_rx.try_iter().take(256) {
            if raw.len() + line.len() <= 2 * 1024 * 1024 {
                raw.push_str(&line);
            } else {
                outcome = Outcome::Failed("CLI output exceeded 2 MiB; result is incomplete".into());
            }
            if !matches!(spec.kind, Kind::Help | Kind::Recovery) {
                if spec.sensitive {
                    if !sensitive_notice {
                        log(
                            tx,
                            "[security] Sensitive CLI output withheld; structured results only"
                                .into(),
                        );
                        sensitive_notice = true;
                    }
                } else {
                    log(tx, parse::log_line(&line));
                }
            }
            line.zeroize();
        }
        if cancel.load(Ordering::SeqCst) {
            outcome = Outcome::Cancelled;
            crate::process_tree::terminate(child.id());
            let _ = child.kill();
            break;
        }
        if start.elapsed() > Duration::from_secs(settings.timeout_secs.clamp(2, 300)) {
            log(
                tx,
                format!(
                    "CLI deadline reached: stdin delivered={}, captured output={} bytes",
                    input_reported,
                    raw.len()
                ),
            );
            outcome = Outcome::TimedOut;
            crate::process_tree::terminate(child.id());
            let _ = child.kill();
            break;
        }
        match child.try_wait() {
            Ok(Some(s)) => {
                exited = true;
                if !matches!(spec.kind, Kind::Help | Kind::Recovery) {
                    log(tx, format!("CLI process exited ({s})"));
                }
                if !s.success() {
                    outcome = Outcome::Failed(format!("CLI exited with {s}"));
                }
                break;
            }
            Err(e) => {
                outcome = Outcome::Failed(e.to_string());
                crate::process_tree::terminate(child.id());
                let _ = child.kill();
                break;
            }
            _ => {}
        }
        thread::sleep(Duration::from_millis(20));
    }
    if !exited {
        let _ = child.wait();
    }
    // Drain until the direct child's pipes close. Do not join readers indefinitely if
    // a console-launched descendant inherited its handles.
    let drain_start = Instant::now();
    loop {
        match line_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(mut line) => {
                if raw.len() + line.len() <= 2 * 1024 * 1024 {
                    raw.push_str(&line);
                } else {
                    outcome = Outcome::Failed("CLI output exceeded 2 MiB".into());
                }
                if spec.kind != Kind::Help && !spec.sensitive {
                    log(tx, parse::log_line(&line));
                }
                line.zeroize();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            _ => {
                if drain_start.elapsed() > Duration::from_millis(500) {
                    break;
                }
            }
        }
    }
    drop(line_rx);
    drop(stdout);
    drop(stderr);
    drop(writer);
    if bridge.is_some() && !raw.contains("[STUDIO_BRIDGE_DONE]") && outcome == Outcome::Completed {
        outcome = Outcome::Failed("CLI did not complete the secure long-command bridge".into());
    }
    let e = parse::evidence(&raw, &spec.kind);
    if outcome == Outcome::Completed
        && e.reported_error
        && !matches!(spec.kind, Kind::Help | Kind::Recovery)
    {
        outcome = Outcome::Failed("CLI reported an error; no success is assumed".into());
    }
    if outcome == Outcome::Completed && spec.kind == Kind::Authenticate && !e.authenticated {
        outcome = Outcome::Failed("No recognised authentication success response".into());
    }
    (outcome, raw, e)
}

fn execute_batch(
    settings: &Settings,
    batch: crate::backup::Batch,
    tx: &SyncSender<Event>,
    cancel: &AtomicBool,
) -> JobResult {
    use crate::backup::{self, Geometry};
    if let Err(e) = backup::validate_uid(&batch.uid) {
        return result(Outcome::Failed(e), Evidence::default());
    }
    let bind = |tx: &SyncSender<Event>| -> Result<(), Outcome> {
        let (out, raw, _) = run_process(
            settings,
            &CommandSpec::simple("hf 14a info", Kind::Detect),
            true,
            tx,
            cancel,
        );
        if out != Outcome::Completed {
            return Err(out);
        }
        if backup::uid(&raw).as_deref() != Some(batch.uid.as_str()) {
            return Err(Outcome::Failed(
                "Target UID changed or could not be read; operation stopped".into(),
            ));
        }
        Ok(())
    };
    let mut all = Evidence::default();
    if let Err(out) = bind(tx) {
        return result(out, all);
    }
    if batch.writing {
        let info = if batch.geometry.classic() {
            "hf mf info"
        } else {
            "hf mfu info"
        };
        let (out, raw, _) = run_process(
            settings,
            &CommandSpec::simple(info, Kind::Detect),
            true,
            tx,
            cancel,
        );

        if out != Outcome::Completed || !batch.geometry.matches_model(&raw) {
            return result(
                Outcome::Failed(
                    "Detected tag model does not match selected memory geometry".into(),
                ),
                all,
            );
        }
    }
    if batch.writing && !batch.geometry.classic() {
        let (out, _, page2) = run_process(
            settings,
            &batch.geometry.read(2, "", false).unwrap(),
            true,
            tx,
            cancel,
        );
        if out != Outcome::Completed {
            return result(out, all);
        }
        let dynamic = if batch.geometry != Geometry::Ultralight {
            let (out, _, e) = run_process(
                settings,
                &batch
                    .geometry
                    .read(batch.geometry.data_end() + 1, "", false)
                    .unwrap(),
                true,
                tx,
                cancel,
            );
            if out != Outcome::Completed {
                return result(out, all);
            }
            Some(e.bytes)
        } else {
            None
        };
        if let Err(e) = backup::check_locks(batch.geometry, &page2.bytes, dynamic.as_deref()) {
            return result(Outcome::Failed(e), all);
        }
    }
    let verifying = !batch.steps.is_empty() && batch.steps.iter().all(|s| s.expected.is_some());
    for (index, step) in batch.steps.into_iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return result(Outcome::Cancelled, all);
        }
        if let Err(out) = bind(tx) {
            return result(out, all);
        }
        log(tx, format!("Batch data unit {}", index + 1));
        if let Some(write) = step.write {
            let (out, _, _) = run_process(settings, &write, true, tx, cancel);
            if out != Outcome::Completed {
                return result(out, all);
            }
        }
        let (out, _, e) = run_process(settings, &step.read, true, tx, cancel);
        if out != Outcome::Completed {
            return result(out, all);
        }
        if e.bytes.len() != batch.geometry.width() {
            return result(Outcome::Failed("Read returned an incomplete data unit; no complete backup/verification claimed".into()),all);
        }
        if let Some(expected) = step.expected {
            if expected != e.bytes {
                return result(Outcome::Mismatch, all);
            }
        }
        all.bytes.extend(e.bytes);
        if let Err(out) = bind(tx) {
            return result(out, all);
        }
    }
    result(
        if batch.writing || verifying {
            Outcome::Verified
        } else {
            Outcome::Completed
        },
        all,
    )
}

fn run_terminal_once(
    settings: &Settings,
    spec: &CommandSpec,
    device: bool,
    tx: &SyncSender<Event>,
    cancel: &AtomicBool,
) -> (Outcome, Zeroizing<String>, Evidence) {
    use crate::terminal::{Input, Session};
    let session = match Session::start(settings, !device) {
        Ok(s) => s,
        Err(e) => {
            return (
                Outcome::Failed(e),
                Zeroizing::new(String::new()),
                Evidence::default(),
            )
        }
    };
    // Wait for the CLI prompt before sending a long command to its terminal editor.
    let start = Instant::now();
    let mut raw = Zeroizing::new(String::new());
    let mut sent = false;
    let mut ready = String::new();
    let mut out = Outcome::Completed;
    loop {
        if cancel.load(Ordering::SeqCst) {
            out = Outcome::Cancelled;
            break;
        }
        if start.elapsed() > Duration::from_secs(settings.timeout_secs.clamp(2, 300)) {
            out = Outcome::TimedOut;
            break;
        }
        match session.output.recv_timeout(Duration::from_millis(25)) {
            Ok(mut text) => {
                if raw.len() + text.len() > 2 * 1024 * 1024 {
                    out = Outcome::Failed("CLI output exceeded 2 MiB".into());
                    break;
                }
                if !sent {
                    ready.push_str(&text);
                    if ready.len() > 65536 {
                        out = Outcome::Failed(
                            "Unrecognised terminal startup output exceeded 64 KiB".into(),
                        );
                        break;
                    }
                    if ready.contains("pm3 -->") || ready.contains("pm3 ➜") {
                        let mut input = Zeroizing::new(spec.text.to_string());
                        input.push_str("; quit");
                        if let Err(e) = session.send(Input::Line(input)) {
                            out = Outcome::Failed(e);
                            break;
                        }
                        sent = true;
                        ready.zeroize();
                        text.zeroize();
                        continue;
                    }
                }
                if sent {
                    raw.push_str(&text);
                    if !spec.sensitive {
                        for line in text.lines() {
                            log(tx, parse::log_line(line));
                        }
                    }
                }
                text.zeroize();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    if out == Outcome::Completed {
        match session.exit.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(0)) => {}
            Ok(Ok(code)) => out = Outcome::Failed(format!("CLI exited with status {code}")),
            Ok(Err(e)) => out = Outcome::Failed(e),
            Err(_) => {
                out = Outcome::Failed(
                    "CLI closed its terminal without a confirmed exit status".into(),
                )
            }
        }
    }
    drop(session);
    ready.zeroize();
    if !sent && out == Outcome::Completed {
        out = Outcome::Failed("CLI exited before a terminal prompt was observed".into());
    }
    let evidence = parse::evidence(&raw, &spec.kind);
    if evidence.reported_error && out == Outcome::Completed && spec.kind != Kind::Recovery {
        out = Outcome::Failed("CLI reported an error".into());
    }
    (out, raw, evidence)
}

struct ArchiveClient<'a> {
    settings: &'a Settings,
    tx: &'a SyncSender<Event>,
    cancel: &'a AtomicBool,
    caps: &'a Capabilities,
}
struct GuidedClient<'a> {
    settings: &'a Settings,
    tx: &'a SyncSender<Event>,
    cancel: &'a AtomicBool,
    caps: &'a Capabilities,
}
impl crate::basic::Client for GuidedClient<'_> {
    fn run(&mut self, spec: CommandSpec) -> (Outcome, Zeroizing<String>) {
        if self.cancel.load(Ordering::SeqCst) {
            return (Outcome::Cancelled, Zeroizing::new(String::new()));
        }
        if let Err(e) = self.caps.validate(&spec.text) {
            return (Outcome::Failed(e), Zeroizing::new(String::new()));
        }
        let (out, raw, _) = run_process(self.settings, &spec, true, self.tx, self.cancel);
        (out, raw)
    }
    fn execute(&mut self, job: Job) -> JobResult {
        let checked = (|| -> Result<(), String> {
            self.caps.validate("hw version")?;
            match &job {
                Job::Batch(batch) => {
                    self.caps.validate("hf 14a info")?;
                    self.caps.validate("hf mfu info")?;
                    for step in &batch.steps {
                        self.caps.validate(&step.read.text)?;
                        if let Some(write) = &step.write {
                            self.caps.validate(&write.text)?;
                        }
                    }
                    Ok(())
                }
                Job::Programme { write, read, .. } => {
                    self.caps.validate(&write.text)?;
                    self.caps.validate(&read.text)
                }
                Job::ProgrammeCredential { write, .. } => {
                    self.caps.validate(&write.text)?;
                    self.caps.validate("lf hid reader")
                }
                Job::ProgrammeLf(value) => {
                    self.caps.validate(&value.write()?.text)?;
                    self.caps.validate(&value.read().text)
                }
                _ => Err("Unsupported Basic operation".into()),
            }
        })();
        if let Err(e) = checked {
            return result(Outcome::Failed(e), Evidence::default());
        }
        execute(self.settings, job, self.tx, self.cancel)
    }
    fn stage(&mut self, message: &str) {
        log(self.tx, format!("[stage] {message}"));
    }
}
impl crate::desfire_archive::Client for ArchiveClient<'_> {
    fn inspect(&self, command: &str, flags: &[&str]) -> Result<(), Outcome> {
        self.caps.require(command, flags).map_err(Outcome::Failed)
    }
    fn run(&mut self, spec: CommandSpec) -> Result<Zeroizing<String>, Outcome> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(Outcome::Cancelled);
        }
        self.caps.validate(&spec.text).map_err(Outcome::Failed)?;
        let (out, raw, _) = run_process(self.settings, &spec, true, self.tx, self.cancel);
        if out == Outcome::Completed {
            Ok(raw)
        } else {
            Err(out)
        }
    }
}
fn run_archive(
    settings: &Settings,
    request: crate::desfire_archive::Request,
    tx: &SyncSender<Event>,
    cancel: &AtomicBool,
) -> JobResult {
    use crate::desfire_archive::{self, Operation};
    let mut client = ArchiveClient {
        settings,
        tx,
        cancel,
        caps: &request.capabilities,
    };
    let outcome=match request.operation{
  Operation::Admin(mutation)=>desfire_archive::verify_mutation(&mut client,&request.uid,*mutation).map(|()|"Fresh DESFire state/authentication and requested mutation result verified"),
  Operation::Capture{path}=>desfire_archive::capture(&mut client,&request.uid,&request.creds).and_then(|a|{let fresh=desfire_archive::capture(&mut client,&request.uid,&request.creds)?;if a.applications!=fresh.applications{return Err(Outcome::Mismatch)}a.save(&path).map_err(Outcome::Failed)}).map(|()|"Readable application data/settings archive saved; secret keys and immutable PICC data excluded"),
  Operation::Restore{archive,journal}=>desfire_archive::restore(&mut client,&archive,&request.uid,&request.creds,&journal).map(|_|"Restored application settings, supplied keys/versions and readable payload verified by a fresh capture"),
  Operation::Verify{archive}=>archive.validate().map_err(Outcome::Failed).and_then(|()|desfire_archive::capture(&mut client,&request.uid,&request.creds)).and_then(|a|if a.applications==archive.applications{Ok("Fresh application inventory, settings and readable contents match archive")}else{Err(Outcome::Mismatch)}),
 };
    match outcome {
        Ok(label) => {
            let e = Evidence {
                verification: Some(label.into()),
                ..Default::default()
            };
            result(Outcome::Verified, e)
        }
        Err(out) => result(out, Evidence::default()),
    }
}

fn encode_format(
    settings: &Settings,
    value: crate::format_catalogue::Credential,
    programme: bool,
    tx: &SyncSender<Event>,
    cancel: &AtomicBool,
) -> JobResult {
    let spec = match value.encode() {
        Ok(s) => s,
        Err(e) => return result(Outcome::Failed(e), Evidence::default()),
    };
    let (out, raw, mut e) = run_process(settings, &spec, false, tx, cancel);
    if out != Outcome::Completed {
        return result(out, e);
    }
    let expected = match crate::format_catalogue::encoded(&raw) {
        Ok(s) => s,
        Err(err) => return result(Outcome::Failed(err), e),
    };
    e.encoded_wiegand = Some(expected.clone());
    if programme {
        let write = match value.write(&expected) {
            Ok(s) => s,
            Err(err) => return result(Outcome::Failed(err), e),
        };
        let mut r = execute(settings, Job::ProgrammeRaw { write, expected }, tx, cancel);
        r.evidence.encoded_wiegand = e.encoded_wiegand;
        r.evidence.verification = Some(
            "fresh HID raw matches the CLI-generated encoding, including issue/OEM fields".into(),
        );
        r
    } else {
        result(Outcome::Completed, e)
    }
}

fn run_signal(
    settings: &Settings,
    r: crate::acquisition::Request,
    tx: &SyncSender<Event>,
    cancel: &AtomicBool,
) -> JobResult {
    use crate::acquisition as a;
    let attempt = || -> Result<Evidence, Outcome> {
        let temp = tempfile::Builder::new()
            .prefix("pm3-signal-")
            .tempdir()
            .map_err(|e| Outcome::Failed(e.to_string()))?;
        let base = temp.path().join("capture");
        let file = base.with_extension("pm3");
        let analysis = a::analysis_spec(&r, &file).map_err(Outcome::Failed)?;
        let live = r.saved.is_none();
        let signal = if let Some(signal) = &r.saved {
            let bytes = a::text(signal);
            crate::storage::write_private(&file, bytes.as_bytes()).map_err(Outcome::Failed)?;
            signal.clone()
        } else {
            let spec = a::capture_spec(&r, &base).map_err(Outcome::Failed)?;
            let (out, _, _) = run_process(settings, &spec, true, tx, cancel);
            if out != Outcome::Completed {
                return Err(out);
            }
            let bytes =
                crate::storage::read_bounded(&file, 8 * 1024 * 1024).map_err(Outcome::Failed)?;
            let signal = crate::signal::Signal::parse(
                std::str::from_utf8(&bytes)
                    .map_err(|_| Outcome::Failed("Signal file is not text".into()))?,
            )
            .map_err(Outcome::Failed)?;
            if signal.samples.len() > r.samples as usize {
                return Err(Outcome::Failed(
                    "Capture returned more samples than requested".into(),
                ));
            }
            signal
        };
        let mut bits = String::new();
        let mut error = None;
        if let Some(spec) = analysis {
            let (out, raw, _) = run_process(settings, &spec, false, tx, cancel);
            match out {
                Outcome::Completed => match a::bits(&raw) {
                    Ok(v) => bits = v,
                    Err(e) => error = Some(e),
                },
                Outcome::Cancelled | Outcome::TimedOut => return Err(out),
                _ => {
                    error =
                        Some("Demodulation did not complete; raw samples remain available".into())
                }
            }
        }
        let decoding=a::em410x_frames(&bits).into_iter().map(|id|format!("EM410x ID {id} — frame row/column parity valid; reader facility/card layout remains unconfirmed")).collect();
        Ok(Evidence {
            verification: Some(format!(
                "{} samples {}. No card memory was written.",
                signal.samples.len(),
                if live {
                    "acquired from the selected reader"
                } else {
                    "analysed from the saved waveform"
                }
            )),
            signal: Some(a::Capture {
                signal,
                bits,
                decoding,
                analysis_error: error,
                simulated: r.simulated,
                live,
            }),
            ..Default::default()
        })
    };
    match attempt() {
        Ok(e) => result(Outcome::Completed, e),
        Err(o) => result(o, Evidence::default()),
    }
}
