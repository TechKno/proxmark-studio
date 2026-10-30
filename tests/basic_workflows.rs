use proxmark_studio::{
    basic::{self, Action, Card, Content, Destination, Hint, Pending, StepView, Workflow},
    capabilities::Capabilities,
    credential::{Credential, Format, Target},
    model::{Evidence, Settings},
    runner::{Event, Job, JobResult, Outcome, Worker},
};
use std::time::Duration;
fn card() -> Card {
    Card {
        content: Content::Em410x("0102030405".into()),
        captured_at: 1,
        simulated: true,
    }
}
fn run(s: &Settings, job: Job) -> JobResult {
    let worker = Worker::start(s.clone(), job);
    loop {
        match worker
            .rx
            .recv_timeout(Duration::from_secs(30))
            .expect("bounded worker")
        {
            Event::Finished(r) => return *r,
            Event::Log(_) => {}
        }
    }
}
fn fixture(name: &str) -> (tempfile::TempDir, Settings, Capabilities) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".mock-pm3-card"), name).unwrap();
    std::fs::write(dir.path().join(".mock-pm3-enable-state"), "").unwrap();
    let s = Settings {
        cli: env!("CARGO_BIN_EXE_mock-pm3").into(),
        port: "SIMULATED".into(),
        working_dir: dir.path().to_string_lossy().into(),
        timeout_secs: 3,
        ..Default::default()
    };
    let r = run(&s, Job::Probe);
    assert_eq!(r.outcome, Outcome::Completed);
    (dir, s, r.capabilities.unwrap())
}
fn guided(s: &Settings, caps: &Capabilities, action: Action) -> JobResult {
    run(
        s,
        Job::Guided(Box::new(basic::Request {
            action,
            caps: caps.clone(),
            simulated: true,
        })),
    )
}
fn place(dir: &tempfile::TempDir, name: &str) {
    std::fs::write(dir.path().join(".mock-pm3-card"), name).unwrap();
}
#[test]
fn snapshot_survives_destination_failure_and_read_failure() {
    let mut f = Workflow::default();
    f.set_source(card());
    f.pending = Some(Pending::Check);
    f.receive(&Outcome::Failed("no target".into()), &Evidence::default());
    assert_eq!(f.source, Some(card()));
    assert!(f.destination.is_none());
    assert_eq!(f.step, StepView::Target);
    f.pending = Some(Pending::Read);
    f.receive(&Outcome::TimedOut, &Evidence::default());
    assert_eq!(f.source, Some(card()));
}
#[test]
fn new_source_and_finished_write_invalidate_previous_consent() {
    let mut f = Workflow {
        destination: Some(Destination::T55 {
            target: Target::T5577,
            block0: "000880E0".into(),
        }),
        acknowledged: true,
        ..Default::default()
    };
    f.set_source(card());
    assert!(f.destination.is_none());
    assert!(!f.acknowledged);
    f.pending = Some(Pending::Write);
    f.receive(&Outcome::Mismatch, &Evidence::default());
    assert_eq!(f.step, StepView::Result);
    assert_eq!(f.result, Some(Outcome::Mismatch));
    assert!(!f.acknowledged);
}
#[test]
fn destination_parser_requires_one_password_free_known_chip() {
    let good="[=] Chip type......... T55x7\n[=] Block0............ 000880E0 (auto detect)\n[=] Password set...... No";
    assert!(basic::parse_t55(good).is_ok());
    for bad in [
        good.replace("No", "Yes"),
        format!("{good}\n{good}"),
        good.replace("T55x7", "unknown"),
        format!("{good}\n[!!] Authentication failed"),
    ] {
        assert!(basic::parse_t55(&bad).is_err());
    }
}
#[test]
fn exact_model_and_unambiguous_source_are_required() {
    assert_eq!(
        basic::geometry("TYPE: NTAG 213"),
        Some(proxmark_studio::backup::Geometry::Ntag213)
    );
    assert!(basic::geometry("NTAG 213 and NTAG 215").is_none());
    assert!(basic::geometry("MIFARE Ultralight").is_none());
    assert!(basic::parse_lf("[+] EM 410x ID 0102030405\n[+] EM 410x ID 0102030406").is_err());
}
#[test]
fn desfire_and_unselected_hid_never_produce_a_basic_write() {
    let dest = Destination::T55 {
        target: Target::T5577,
        block0: "000880E0".into(),
    };
    let mut c = card();
    c.content = Content::Identified {
        family: "DESFire EV3".into(),
        uid: Some("04112233445566".into()),
        reason: "Unsupported EV2/EV3 operation".into(),
    };
    assert!(basic::write_job(&c, &dest).is_err());
    c.content = Content::Hid {
        raw: Credential {
            format: Format::H10301,
            facility_code: 118,
            card_number: 1603,
        }
        .encode()
        .unwrap()
        .hid_raw,
        credential: None,
    };
    assert!(c.writable().is_err());
    c.select_format(Format::H10301).unwrap();
    assert!(basic::write_job(&c, &dest).is_ok());
}
#[test]
fn library_round_trip_checks_integrity_and_names() {
    let items = vec![basic::SavedCard {
        name: "Lab source".into(),
        card: card(),
    }];
    basic::validate_library(&items).unwrap();
    let round =
        serde_json::from_slice::<Vec<basic::SavedCard>>(&serde_json::to_vec(&items).unwrap())
            .unwrap();
    assert_eq!(round, items);
    let mut bad = items.clone();
    bad[0].name.clear();
    assert!(basic::validate_library(&bad).is_err());
    bad = items;
    bad[0].card.content = Content::Em410x("not a card".into());
    assert!(basic::validate_library(&bad).is_err());
}
#[test]
fn mock_read_clone_em_and_mismatch() {
    let (dir, s, caps) = fixture("em");
    let r = guided(&s, &caps, Action::Read(Hint::Automatic));
    assert_eq!(r.outcome, Outcome::Completed);
    let c = r.evidence.card.unwrap();
    assert_eq!(c.content, card().content);
    place(&dir, "t5577");
    let checked = guided(&s, &caps, Action::Check(c.clone()));
    assert_eq!(checked.outcome, Outcome::Completed);
    let dest = checked.evidence.destination.unwrap();
    assert_eq!(
        guided(
            &s,
            &caps,
            Action::Write {
                card: c.clone(),
                destination: dest,
                acknowledged: true
            }
        )
        .outcome,
        Outcome::Verified
    );
    std::fs::remove_file(dir.path().join(".mock-pm3-em")).unwrap();
    place(&dir, "ignore");
    let dest = guided(&s, &caps, Action::Check(c.clone()))
        .evidence
        .destination
        .unwrap();
    assert_eq!(
        guided(
            &s,
            &caps,
            Action::Write {
                card: c,
                destination: dest,
                acknowledged: true
            }
        )
        .outcome,
        Outcome::Mismatch
    );
}
#[test]
fn mock_write_hid_and_fresh_decode() {
    let (_dir, s, caps) = fixture("t5577");
    let credential = Credential {
        format: Format::H10301,
        facility_code: 118,
        card_number: 1604,
    };
    let c = Card {
        content: Content::Hid {
            raw: credential.encode().unwrap().hid_raw,
            credential: Some(credential.clone()),
        },
        ..card()
    };
    let dest = guided(&s, &caps, Action::Check(c.clone()))
        .evidence
        .destination
        .unwrap();
    let result = guided(
        &s,
        &caps,
        Action::Write {
            card: c,
            destination: dest,
            acknowledged: true,
        },
    );
    assert_eq!(result.outcome, Outcome::Verified);
    assert_eq!(result.evidence.credential.unwrap().credential, credential);
}
#[test]
fn mock_auto_falls_back_to_hf_and_only_copies_user_pages() {
    let (dir, s, caps) = fixture("ntag");
    let r = guided(&s, &caps, Action::Read(Hint::Automatic));
    assert_eq!(r.outcome, Outcome::Completed);
    let c = r.evidence.card.unwrap();
    let Content::Memory(d) = &c.content else {
        panic!("expected dump")
    };
    assert_eq!(d.bytes.len(), 180);
    place(&dir, "ntag_blank");
    std::fs::write(dir.path().join(".mock-pm3-memory.bin"), vec![0; 4096]).unwrap();
    let check = guided(&s, &caps, Action::Check(c.clone()));
    assert_eq!(check.outcome, Outcome::Completed);
    let dest = check.evidence.destination.unwrap();
    let Job::Batch(batch) = basic::write_job(&c, &dest).unwrap() else {
        panic!("batch")
    };
    assert_eq!(batch.steps.len(), 36);
    assert!(batch.steps.iter().all(|s| s.write.is_some()));
    assert_eq!(
        guided(
            &s,
            &caps,
            Action::Write {
                card: c,
                destination: dest,
                acknowledged: true
            }
        )
        .outcome,
        Outcome::Verified
    );
    let memory = std::fs::read(dir.path().join(".mock-pm3-memory.bin")).unwrap();
    assert_eq!(&memory[16..20], &[0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(&memory[..16], &[0; 16]);
    assert_eq!(&memory[160..180], &[0; 20]);
}
#[test]
fn no_card_and_locked_destination_do_not_enable_write() {
    let (dir, s, caps) = fixture("none");
    assert!(matches!(
        guided(&s, &caps, Action::Read(Hint::Automatic)).outcome,
        Outcome::Failed(_)
    ));
    place(&dir, "ntag");
    let c = guided(&s, &caps, Action::Read(Hint::Hf))
        .evidence
        .card
        .unwrap();
    place(&dir, "locked");
    std::fs::write(dir.path().join(".mock-pm3-locked"), "").unwrap();
    let r = guided(&s, &caps, Action::Check(c));
    assert!(matches!(r.outcome, Outcome::Failed(_)));
    assert!(r.evidence.destination.is_none());
}
#[test]
fn stale_target_firmware_mismatch_missing_capabilities_and_mixed_simulation_block_writes() {
    let (dir, s, caps) = fixture("t5577");
    let dest = guided(&s, &caps, Action::Check(card()))
        .evidence
        .destination
        .unwrap();
    let attempt = |c: Card, d: Destination, ack| Action::Write {
        card: c,
        destination: d,
        acknowledged: ack,
    };
    assert!(matches!(
        guided(&s, &caps, attempt(card(), dest.clone(), false)).outcome,
        Outcome::Failed(_)
    ));
    let mut physical = card();
    physical.simulated = false;
    assert!(matches!(
        guided(&s, &caps, attempt(physical, dest.clone(), true)).outcome,
        Outcome::Failed(_)
    ));
    let changed = Destination::T55 {
        target: Target::T5577,
        block0: "11111111".into(),
    };
    assert!(matches!(
        guided(&s, &caps, attempt(card(), changed, true)).outcome,
        Outcome::Failed(_)
    ));
    let mut reduced = caps.clone();
    reduced.help.remove("lf em 410x clone");
    assert!(matches!(
        guided(&s, &reduced, attempt(card(), dest.clone(), true)).outcome,
        Outcome::Failed(_)
    ));
    std::fs::write(dir.path().join(".mock-pm3-version-mismatch"), "").unwrap();
    assert!(matches!(
        guided(&s, &caps, attempt(card(), dest, true)).outcome,
        Outcome::Failed(_)
    ));
    assert!(!dir.path().join(".mock-pm3-em").exists());
}
#[test]
fn mock_desfire_ev3_identification_has_no_basic_write() {
    let (_dir, s, caps) = fixture("desfire");
    let r = guided(&s, &caps, Action::Read(Hint::Automatic));
    assert_eq!(r.outcome, Outcome::Completed);
    let card = r.evidence.card.unwrap();
    assert!(card.writable().is_err());
    assert!(card.summary().contains("EV2/LRP/EV3"));
}
