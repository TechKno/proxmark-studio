use proxmark_studio::{
    capabilities::Capabilities,
    desfire_archive::{Auth, Credentials, Operation, Request},
    model::{Algorithm, Channel, Settings},
    runner::{Event, Job, JobResult, Outcome, Worker},
};
use std::time::Duration;
fn wait(worker: Worker) -> (JobResult, Vec<String>) {
    let mut logs = vec![];
    loop {
        match worker.rx.recv_timeout(Duration::from_secs(25)).unwrap() {
            Event::Log(s) => logs.push(s),
            Event::Finished(r) => return (*r, logs),
        }
    }
}
fn setup() -> (tempfile::TempDir, Settings, Capabilities) {
    let dir = tempfile::tempdir().unwrap();
    for marker in [".mock-pm3-desfire", ".mock-pm3-enable-state"] {
        std::fs::write(dir.path().join(marker), "").unwrap();
    }
    let settings = Settings {
        cli: env!("CARGO_BIN_EXE_mock-pm3").into(),
        port: "SIMULATED".into(),
        working_dir: dir.path().to_string_lossy().into(),
        timeout_secs: 5,
        ..Default::default()
    };
    let caps = wait(Worker::start(settings.clone(), Job::Probe))
        .0
        .capabilities
        .unwrap();
    (dir, settings, caps)
}
fn creds() -> Credentials {
    Credentials {
        fallback: Auth::zero(Algorithm::Aes, Channel::Ev1),
        keys: Default::default(),
    }
}
#[test]
fn failed_second_capture_never_replaces_an_existing_backup() {
    let (dir, s, caps) = setup();
    let path = dir.path().join("archive.json");
    std::fs::write(&path, "original backup").unwrap();
    let card = proxmark_studio::mock_desfire::Card {
        fail_after: Some(23),
        ..Default::default()
    };
    std::fs::write(
        dir.path().join(".mock-pm3-desfire-state.json"),
        serde_json::to_vec(&card).unwrap(),
    )
    .unwrap();
    let r = wait(Worker::start(
        s,
        Job::Desfire(Box::new(Request {
            uid: card.archive.uid,
            creds: creds(),
            operation: Operation::Capture { path: path.clone() },
            capabilities: caps,
        })),
    ))
    .0;
    assert!(matches!(r.outcome, Outcome::Failed(_)));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "original backup");
}
#[test]
fn missing_verifier_capability_blocks_mutation_before_the_write() {
    let (dir, s, mut caps) = setup();
    caps.help.remove("hf mfdes getkeyversions");
    let admin = proxmark_studio::desfire_admin::Admin {
        action: proxmark_studio::desfire_admin::Action::DeleteFile,
        ..Default::default()
    };
    let auth = Auth::zero(Algorithm::Aes, Channel::Ev1);
    let write = auth
        .spec(
            "deletefile",
            "112233",
            Some(1),
            "",
            proxmark_studio::command::Kind::Write,
            proxmark_studio::model::Communication::Plain,
        )
        .unwrap();
    let mutation = proxmark_studio::desfire_archive::Mutation {
        admin,
        auth: auth.clone(),
        aid: "112233".into(),
        fid: 1,
        write,
    };
    let (r, logs) = wait(Worker::start(
        s,
        Job::Desfire(Box::new(Request {
            uid: "04112233445566".into(),
            creds: creds(),
            operation: Operation::Admin(Box::new(mutation)),
            capabilities: caps,
        })),
    ));
    assert!(matches!(r.outcome, Outcome::Failed(_)));
    assert!(!logs.iter().any(|l| l.contains("deletefile")));
    assert!(!dir.path().join(".mock-pm3-desfire-state.json").exists());
}
#[test]
fn provisioning_result_survives_reload_and_verified_row_is_not_queued_again() {
    let (dir, s, caps) = setup();
    let mut workspace = proxmark_studio::provision::Workspace {
        path: dir.path().join("batch.json").to_string_lossy().into(),
        batch: Some(
            proxmark_studio::provision::Batch::range(
                proxmark_studio::credential::Format::H10301,
                proxmark_studio::credential::Target::T5577,
                118,
                1603,
                2,
            )
            .unwrap(),
        ),
        active: Some(0),
        ..Default::default()
    };
    workspace.batch.as_mut().unwrap().rows[0].status =
        proxmark_studio::provision::Status::InProgress;
    workspace.persist().unwrap();
    let job = workspace.batch.as_ref().unwrap().rows[0]
        .value
        .job()
        .unwrap();
    proxmark_studio::provision::check_job(&job, &caps).unwrap();
    let r = wait(Worker::start(s, job)).0;
    workspace.finish(&r.outcome);
    let reloaded =
        proxmark_studio::provision::Batch::load(&std::fs::read(&workspace.path).unwrap()).unwrap();
    assert_eq!(r.outcome, Outcome::Verified);
    assert_eq!(
        reloaded.rows[0].status,
        proxmark_studio::provision::Status::Verified
    );
    assert_eq!(reloaded.next(), Some(1));
}
#[test]
fn restore_rejects_wrong_or_modified_archive_before_target_mutations() {
    let (dir, s, caps) = setup();
    let mut archive = proxmark_studio::mock_desfire::Card::default().archive;
    archive.sha256 = "wrong".into();
    let r = wait(Worker::start(
        s,
        Job::Desfire(Box::new(Request {
            uid: "04112233445566".into(),
            creds: creds(),
            operation: Operation::Restore {
                archive,
                journal: dir.path().join("restore.json"),
            },
            capabilities: caps,
        })),
    ))
    .0;
    assert!(matches!(r.outcome, Outcome::Failed(_)));
    assert!(!dir.path().join(".mock-pm3-desfire-state.json").exists());
}
#[test]
fn source_and_destination_uid_may_differ_but_contents_must_match() {
    let (dir, s, caps) = setup();
    let source = proxmark_studio::mock_desfire::Card::default().archive;
    let mut card = proxmark_studio::mock_desfire::Card::default();
    card.archive.uid = "04992233445566".into();
    card.archive.applications.clear();
    std::fs::write(
        dir.path().join(".mock-pm3-desfire-state.json"),
        serde_json::to_vec(&card).unwrap(),
    )
    .unwrap();
    let r = wait(Worker::start(
        s,
        Job::Desfire(Box::new(Request {
            uid: card.archive.uid,
            creds: creds(),
            operation: Operation::Restore {
                archive: source,
                journal: dir.path().join("restore.json"),
            },
            capabilities: caps,
        })),
    ))
    .0;
    assert_eq!(r.outcome, Outcome::Verified);
}
#[test]
fn classic_trailer_update_verifies_new_keys_and_never_exports_them() {
    use proxmark_studio::{
        backup::Geometry,
        classic::{encode_access, KeyType, TrailerRequest},
    };
    let (dir, s, caps) = setup();
    let mut memory = vec![0; 4096];
    memory[48..64].copy_from_slice(&[
        255, 255, 255, 255, 255, 255, 255, 7, 128, 0x69, 255, 255, 255, 255, 255, 255,
    ]);
    std::fs::write(dir.path().join(".mock-pm3-memory.bin"), memory).unwrap();
    std::fs::write(dir.path().join(".mock-pm3-classic-auth"), "").unwrap();
    let request = TrailerRequest {
        uid: "04112233445566".into(),
        geometry: Geometry::Classic1K,
        sector: 0,
        current_type: KeyType::A,
        current_key: zeroize::Zeroizing::new("FFFFFFFFFFFF".into()),
        new_a: zeroize::Zeroizing::new("A0A1A2A3A4A5".into()),
        new_b: zeroize::Zeroizing::new("B0B1B2B3B4B5".into()),
        access: encode_access([0, 0, 0, 3]).unwrap(),
        gpb: 0x69,
        capabilities: caps,
    };
    let (r, logs) = wait(Worker::start(s, Job::ClassicTrailer(request)));
    assert_eq!(r.outcome, Outcome::Verified);
    assert!(r.evidence.bytes.is_empty());
    assert!(!logs
        .iter()
        .any(|s| s.contains("A0A1A2A3A4A5") || s.contains("B0B1B2B3B4B5")));
}
#[test]
fn incompatible_classic_trailer_permission_stops_before_write() {
    use proxmark_studio::{
        backup::Geometry,
        classic::{encode_access, KeyType, TrailerRequest},
    };
    let (dir, s, caps) = setup();
    let mut memory = vec![0; 4096];
    let acl = encode_access([0, 0, 0, 2]).unwrap();
    memory[48..54].fill(255);
    memory[54..57].copy_from_slice(&acl);
    memory[58..64].fill(255);
    std::fs::write(dir.path().join(".mock-pm3-memory.bin"), &memory).unwrap();
    let request = TrailerRequest {
        uid: "04112233445566".into(),
        geometry: Geometry::Classic1K,
        sector: 0,
        current_type: KeyType::A,
        current_key: zeroize::Zeroizing::new("FFFFFFFFFFFF".into()),
        new_a: zeroize::Zeroizing::new("A0A1A2A3A4A5".into()),
        new_b: zeroize::Zeroizing::new("B0B1B2B3B4B5".into()),
        access: encode_access([0, 0, 0, 1]).unwrap(),
        gpb: 0x69,
        capabilities: caps,
    };
    let r = wait(Worker::start(s, Job::ClassicTrailer(request))).0;
    assert!(matches!(r.outcome, Outcome::Failed(_)));
    assert_eq!(
        std::fs::read(dir.path().join(".mock-pm3-memory.bin")).unwrap(),
        memory
    );
}
