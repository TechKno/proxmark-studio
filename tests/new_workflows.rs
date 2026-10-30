use proxmark_studio::{
    backup::{Batch, Geometry, Step},
    capabilities::COMMANDS,
    command::{CommandSpec, Kind},
    credential::Target,
    custom_format::Layout,
    lf::{Credential, Family},
    model::Settings,
    runner::{Event, Job, JobResult, Outcome, Worker},
};
use std::time::{Duration, Instant};
fn setup() -> (tempfile::TempDir, Settings) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(".mock-pm3-enable-state"), "").unwrap();
    let s = Settings {
        cli: env!("CARGO_BIN_EXE_mock-pm3").into(),
        port: "SIMULATED".into(),
        working_dir: d.path().to_string_lossy().into(),
        timeout_secs: 5,
        ..Default::default()
    };
    (d, s)
}
fn wait(w: &Worker) -> (JobResult, Vec<String>) {
    let mut logs = vec![];
    loop {
        match w.rx.recv_timeout(Duration::from_secs(20)).unwrap() {
            Event::Log(s) => logs.push(s),
            Event::Finished(r) => return (*r, logs),
        }
    }
}
fn batch(write: bool, uid: &str) -> Job {
    let g = Geometry::Ntag213;
    Job::Batch(Batch {
        geometry: g,
        uid: uid.into(),
        writing: write,
        steps: vec![Step {
            read: g.read(4, "", false).unwrap(),
            write: write.then(|| g.write(4, &[1, 2, 3, 4], "", false).unwrap()),
            expected: write.then_some(vec![1, 2, 3, 4]),
        }],
    })
}
#[test]
fn batch_writes_then_fresh_reads_and_checks_uid() {
    let (_d, s) = setup();
    let r = wait(&Worker::start(s, batch(true, "04112233445566"))).0;
    assert_eq!(r.outcome, Outcome::Verified);
    assert_eq!(r.evidence.bytes, [1, 2, 3, 4]);
}
#[test]
fn wrong_uid_stops_before_any_programming() {
    let (d, s) = setup();
    let (r, logs) = wait(&Worker::start(s, batch(true, "04992233445566")));
    assert!(matches!(r.outcome, Outcome::Failed(_)));
    assert!(!logs.iter().any(|l| l.contains("Programme data")));
    assert!(!d.path().join(".mock-pm3-memory.bin").exists());
}
#[test]
fn locked_tag_stops_before_any_programming() {
    let (d, s) = setup();
    std::fs::write(d.path().join(".mock-pm3-locked"), "").unwrap();
    let r = wait(&Worker::start(s, batch(true, "04112233445566"))).0;
    assert!(matches!(r.outcome,Outcome::Failed(e)if e.contains("Static lock")));
    assert!(!d.path().join(".mock-pm3-memory.bin").exists());
}
#[test]
fn full_dump_is_exact_selected_size() {
    let (_d, s) = setup();
    let g = Geometry::Ntag213;
    let job = Job::Batch(Batch {
        geometry: g,
        uid: "04112233445566".into(),
        writing: false,
        steps: (0..g.units())
            .map(|u| Step {
                read: g.read(u, "", false).unwrap(),
                write: None,
                expected: None,
            })
            .collect(),
    });
    let r = wait(&Worker::start(s, job)).0;
    assert_eq!(r.outcome, Outcome::Completed);
    assert_eq!(r.evidence.bytes.len(), 180);
}
#[test]
fn custom_binary_programming_requires_exact_fresh_raw() {
    let (_d, s) = setup();
    let (write, expected) = Layout::default().write(118, 1603, Target::T5577).unwrap();
    let r = wait(&Worker::start(s, Job::ProgrammeRaw { write, expected })).0;
    assert_eq!(r.outcome, Outcome::Verified);
}
#[test]
fn all_lf_formats_program_then_parse_a_fresh_reader() {
    for family in [
        Family::Awid26,
        Family::Awid34,
        Family::Awid37,
        Family::Awid50,
        Family::IoProx,
    ] {
        let (_d, s) = setup();
        let c = Credential {
            family,
            facility: 118,
            number: 1603,
            version: 1,
            ..Default::default()
        };
        assert_eq!(
            wait(&Worker::start(s, Job::ProgrammeLf(c))).0.outcome,
            Outcome::Verified
        );
    }
}
#[test]
fn persistent_terminal_processes_several_lines() {
    use proxmark_studio::terminal::{Input, Session};
    let (_d, s) = setup();
    let session = Session::start(&s, true).unwrap();
    let start = Instant::now();
    let mut out = String::new();
    while !out.contains("pm3 -->") {
        let text = session
            .output
            .recv_timeout(Duration::from_secs(3))
            .unwrap_or_else(|e| panic!("Mock terminal startup failed: {e}; output: {out:?}"));
        out.push_str(&text);
        assert!(start.elapsed() < Duration::from_secs(5));
    }
    for command in ["hw version", "hf mfu info"] {
        session
            .send(Input::Line(zeroize::Zeroizing::new(command.into())))
            .unwrap();
    }
    while !out.contains("NTAG 213") {
        let text = session
            .output
            .recv_timeout(Duration::from_secs(3))
            .unwrap_or_else(|e| panic!("Mock terminal command failed: {e}; output: {out:?}"));
        out.push_str(&text);
    }
    assert!(out.contains("abcdef0"));
    assert!(out.contains("0411") || out.contains("04 11"));
}
#[test]
fn long_command_uses_terminal_transport_and_secret_output_stays_private() {
    let (_d, s) = setup();
    let marker = "AB".repeat(300);
    let spec = CommandSpec::new(
        format!("mock echo {marker}"),
        "Synthetic secret test",
        true,
        Kind::Read,
    )
    .unwrap();
    let (r, logs) = wait(&Worker::start(s, Job::Run(spec)));
    assert_eq!(r.outcome, Outcome::Completed);
    assert!(!logs.iter().any(|l| l.contains(&marker)));
}
#[test]
#[ignore = "Requires PROXMARK_RRG_TEST_CLIENT pointing at a built official RRG client"]
fn real_rrg_help_and_long_terminal_adapter() {
    let cli = std::env::var("PROXMARK_RRG_TEST_CLIENT").unwrap();
    let s = Settings {
        cli,
        timeout_secs: 15,
        ..Default::default()
    };
    let (r, _) = wait(&Worker::start(s.clone(), Job::Probe));
    assert_eq!(r.outcome, Outcome::Completed);
    let caps = r.capabilities.unwrap();
    for command in COMMANDS {
        assert!(caps.has(command), "{command} missing");
    }
    use proxmark_studio::{
        command::DesfireParams,
        desfire_admin::{Action, Admin},
        model::{Algorithm, Channel, Communication},
    };
    let p = DesfireParams {
        aid: "112233",
        fid: "01",
        key: "00000000000000000000000000000000",
        key_number: 0,
        algorithm: Algorithm::Aes,
        channel: Channel::Ev1,
        communication: Communication::Encrypt,
        no_auth: false,
        backup_file: false,
    };
    for action in Action::ALL {
        let a = Admin {
            action,
            newkey: zeroize::Zeroizing::new("11".repeat(16)),
            oldkey: zeroize::Zeroizing::new("00".repeat(16)),
            data: "DEADBEEF".into(),
            ..Default::default()
        };
        let spec = a.build(&p).unwrap();
        caps.validate(&spec.text)
            .unwrap_or_else(|e| panic!("{action:?}: {e}"));
    }
    let spec = CommandSpec::new(
        format!("hw version -h{}", " ".repeat(400)),
        "Long official CLI help",
        false,
        Kind::Help,
    )
    .unwrap();
    let (r, logs) = wait(&Worker::start(s, Job::Run(spec)));
    assert_eq!(r.outcome, Outcome::Completed, "CLI output: {logs:?}");
}
#[test]
#[ignore = "Writes and deletes a synthetic temporary credential in the native OS key store"]
fn native_key_store_roundtrip() {
    let alias = format!("Studio-validation-{}", std::process::id());
    let result = (|| {
        proxmark_studio::vault::store(&alias, "00112233445566778899AABBCCDDEEFF", 16)?;
        let key = proxmark_studio::vault::load(&alias, 16)?;
        if key.as_str() != "00112233445566778899AABBCCDDEEFF" {
            return Err("Stored synthetic key differs".to_string());
        }
        Ok::<(), String>(())
    })();
    let cleanup = proxmark_studio::vault::delete(&alias);
    result.unwrap();
    cleanup.unwrap();
    assert!(proxmark_studio::vault::load(&alias, 16).is_err());
}
#[test]
fn terminal_nonzero_exit_cannot_be_completed() {
    let (_d, s) = setup();
    let spec = CommandSpec::new(
        format!("mock exit{}", " ".repeat(300)),
        "Synthetic exit-status check",
        false,
        Kind::Read,
    )
    .unwrap();
    let r = wait(&Worker::start(s, Job::Run(spec))).0;
    assert!(matches!(r.outcome,Outcome::Failed(e)if e.contains("status 7")));
}
#[test]
fn version_mismatch_refuses_the_guided_write() {
    let (d, s) = setup();
    std::fs::write(d.path().join(".mock-pm3-version-mismatch"), "").unwrap();
    let r = wait(&Worker::start(s, batch(true, "04112233445566"))).0;
    assert!(matches!(r.outcome,Outcome::Failed(e)if e.contains("firmware mismatch")));
    assert!(!d.path().join(".mock-pm3-memory.bin").exists());
}
#[test]
fn read_only_batch_verification_reports_verified_only_on_equal_bytes() {
    let (_d, s) = setup();
    let g = Geometry::Ntag213;
    let job = Job::Batch(Batch {
        geometry: g,
        uid: "04112233445566".into(),
        writing: false,
        steps: vec![Step {
            read: g.read(4, "", false).unwrap(),
            write: None,
            expected: Some(vec![0xde, 0xad, 0xbe, 0xef]),
        }],
    });
    assert_eq!(wait(&Worker::start(s, job)).0.outcome, Outcome::Verified);
}
#[test]
#[ignore = "Requires PROXMARK_RRG_TEST_CLIENT for the native stdin/Lua bridge"]
fn real_rrg_secure_lua_bridge_exceeds_terminal_fallback_limit() {
    let cli = std::env::var("PROXMARK_RRG_TEST_CLIENT").unwrap();
    let s = Settings {
        cli,
        timeout_secs: 15,
        ..Default::default()
    };
    let spec = CommandSpec::new(
        format!("hw version -h{}", " ".repeat(1800)),
        "Long CLI help bridge",
        false,
        Kind::Help,
    )
    .unwrap();
    let r = wait(&Worker::start(s, Job::Run(spec))).0;
    assert_eq!(r.outcome, Outcome::Completed);
}
#[test]
fn thousand_byte_record_restore_uses_secret_free_command_bridge() {
    use proxmark_studio::desfire_archive::{self as d, Auth, Credentials, Operation, Request};
    let (dir, s) = setup();
    std::fs::write(dir.path().join(".mock-pm3-desfire"), "").unwrap();
    let mut card = proxmark_studio::mock_desfire::Card::default();
    let mut f = d::File::empty(2);
    f.kind = 3;
    f.record_size = 1000;
    f.max_records = 2;
    f.records = vec![vec![0x42; 1000]];
    card.archive.applications[0].files.push(f);
    card.archive.seal();
    let archive = card.archive.clone();
    card.archive.applications.clear();
    std::fs::write(
        dir.path().join(".mock-pm3-desfire-state.json"),
        serde_json::to_vec(&card).unwrap(),
    )
    .unwrap();
    let caps = wait(&Worker::start(s.clone(), Job::Probe))
        .0
        .capabilities
        .unwrap();
    let creds = Credentials {
        fallback: Auth::zero(
            proxmark_studio::model::Algorithm::Aes,
            proxmark_studio::model::Channel::Ev1,
        ),
        keys: Default::default(),
    };
    let r = wait(&Worker::start(
        s,
        Job::Desfire(Box::new(Request {
            uid: archive.uid.clone(),
            creds,
            operation: Operation::Restore {
                archive,
                journal: dir.path().join("restore.json"),
            },
            capabilities: caps,
        })),
    ))
    .0;
    assert_eq!(r.outcome, Outcome::Verified);
}
#[test]
fn real_worker_captures_a_complete_secret_free_archive() {
    use proxmark_studio::desfire_archive::{Archive, Auth, Credentials, Operation, Request};
    let (dir, s) = setup();
    std::fs::write(dir.path().join(".mock-pm3-desfire"), "").unwrap();
    let caps = wait(&Worker::start(s.clone(), Job::Probe))
        .0
        .capabilities
        .unwrap();
    let path = dir.path().join("card.json");
    let creds = Credentials {
        fallback: Auth::zero(
            proxmark_studio::model::Algorithm::Aes,
            proxmark_studio::model::Channel::Ev1,
        ),
        keys: Default::default(),
    };
    let (r, logs) = wait(&Worker::start(
        s,
        Job::Desfire(Box::new(Request {
            uid: "04112233445566".into(),
            creds,
            operation: Operation::Capture { path: path.clone() },
            capabilities: caps,
        })),
    ));
    assert_eq!(r.outcome, Outcome::Verified);
    let a = Archive::load(&path).unwrap();
    assert_eq!(a.applications[0].files[0].data, [0xde, 0xad, 0xbe, 0xef]);
    assert!(!logs
        .iter()
        .any(|s| s.contains("00000000000000000000000000000000")));
}
#[test]
#[ignore = "Requires a real RRG client for wider format catalogue/issue/OEM encoding"]
fn real_rrg_wider_format_catalogue_and_issue_fields() {
    let cli = std::env::var("PROXMARK_RRG_TEST_CLIENT").unwrap();
    let s = Settings {
        cli,
        timeout_secs: 15,
        ..Default::default()
    };
    let r = wait(&Worker::start(s.clone(), Job::FormatCatalogue)).0;
    assert_eq!(r.outcome, Outcome::Completed);
    assert_eq!(r.evidence.format_catalogue.len(), 43);
    let c = proxmark_studio::format_catalogue::Credential {
        format: "Kastle".into(),
        facility: 101,
        card: 1337,
        issue: 1,
        oem: 0,
        ..Default::default()
    };
    let r = wait(&Worker::start(s, Job::FormatEncode(c))).0;
    assert_eq!(r.outcome, Outcome::Completed);
    assert_eq!(
        r.evidence.encoded_wiegand.unwrap(),
        "000000000000002142CA0A72"
    );
}
#[test]
fn library_programming_encodes_offline_and_checks_a_separate_read() {
    let (_dir, s) = setup();
    let value = proxmark_studio::format_catalogue::Credential::default();
    let (r, logs) = wait(&Worker::start(s, Job::FormatProgramme(value)));
    assert_eq!(r.outcome, Outcome::Verified);
    assert_eq!(r.evidence.hid_raw, ["000000000000002006EC0C86"]);
    assert!(logs.iter().any(|s| s.contains("raw clone")));
}

#[test]
fn named_hid_clone_supersedes_a_previous_raw_clone_in_simulation() {
    let (_dir, settings) = setup();
    let value = proxmark_studio::format_catalogue::Credential::default();
    assert_eq!(
        wait(&Worker::start(
            settings.clone(),
            Job::FormatProgramme(value)
        ))
        .0
        .outcome,
        Outcome::Verified
    );
    let credential = proxmark_studio::credential::Credential::from_decimal(
        proxmark_studio::credential::Format::H10301,
        "17",
        "1234",
    )
    .unwrap();
    let spec = proxmark_studio::credential::write_command(&credential, Target::T5577).unwrap();
    assert_eq!(
        wait(&Worker::start(settings.clone(), Job::Run(spec)))
            .0
            .outcome,
        Outcome::Completed
    );
    let r = wait(&Worker::start(
        settings,
        Job::Run(proxmark_studio::credential::read_command()),
    ))
    .0;
    assert_eq!(r.evidence.hid_raw, [credential.encode().unwrap().hid_raw]);
}
