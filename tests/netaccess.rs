use proxmark_studio::{
    basic::{Card, Content},
    em410x::ReaderFormat,
    netaccess::{self, CardRecord, Repository, State, Store, User},
    runner::Outcome,
};

fn roster() -> Store {
    Store {
        profile: netaccess::Profile {
            organisation: "Example Organisation".into(),
            site: "Main".into(),
            facility: 42,
            confirmed: true,
            ..Default::default()
        },
        users: vec![
            User {
                number: 1,
                name: "Example One".into(),
                controller_number: Some(1),
            },
            User {
                number: 2,
                name: "Example Two".into(),
                controller_number: Some(2),
            },
        ],
        cards: vec![CardRecord::imported(1001, 1), CardRecord::imported(5001, 2)],
        ..Store::default()
    }
}
fn scan(id: &str, simulated: bool) -> Card {
    Card {
        content: Content::Em410x(id.into()),
        captured_at: 1,
        simulated,
    }
}
#[test]
fn profile_encodes_em410x_with_facility_42_not_hid() {
    let card = netaccess::new_card(&roster().profile, 1001, false).unwrap();
    assert!(
        matches!(card.content, Content::Em410xMapped { ref id, format: ReaderFormat::Wiegand26 } if id == "00002A03E9")
    );
    assert_eq!(
        netaccess::identity(&roster().profile, &card, false).unwrap(),
        (1001, "00002A03E9".into())
    );
    assert!(netaccess::new_card(&roster().profile, 0, false).is_err());
}
#[test]
fn disabled_expired_reserved_and_failed_numbers_stay_taken() {
    let mut s = roster();
    s.cards[1].enabled = Some(false);
    s.cards[1].expires_on = "01/01/2020".into();
    assert_eq!(s.reserve(1).unwrap(), 5002);
    let id = netaccess::new_card(&roster().profile, 5002, false)
        .unwrap()
        .em_identity()
        .unwrap()
        .0
        .to_owned();
    s.begin_write(5002, 1, &id).unwrap();
    s.finish_write(5002, &Outcome::TimedOut).unwrap();
    assert_eq!(s.card(5002).unwrap().state, State::Uncertain);
    assert_eq!(s.reserve(2).unwrap(), 5003);
}
#[test]
fn scan_selects_roster_user_preserves_full_id_and_does_not_reassign() {
    let mut s = roster();
    let c = scan("12AB2A03E9", false);
    assert_eq!(s.link_scan(&c, None).unwrap(), 1);
    assert_eq!(s.card(1001).unwrap().observed_ids, vec!["12AB2A03E9"]);
    assert!(s.link_scan(&c, Some(2)).is_err());
    assert_eq!(s.card(1001).unwrap().user_no, 1);
    assert_eq!(s.card(1001).unwrap().state, State::Imported);
}
#[test]
fn unknown_scan_requires_explicit_user_and_blocks_later_allocation() {
    let mut s = roster();
    let c = scan("00002A2000", false);
    assert!(s.link_scan(&c, None).is_err());
    assert_eq!(s.link_scan(&c, Some(2)).unwrap(), 2);
    assert!(s.card(8192).unwrap().needs_enrolment);
    assert_eq!(s.reserve(1).unwrap(), 8193);
}
#[test]
fn wrong_facility_hid_and_demo_cannot_modify_physical_roster() {
    let mut s = roster();
    for c in [
        scan("00000103E9", false),
        scan("00002A03E9", true),
        Card {
            content: Content::Hid {
                raw: "1234".into(),
                credential: None,
            },
            captured_at: 1,
            simulated: false,
        },
    ] {
        assert!(s.link_scan(&c, Some(1)).is_err());
    }
    assert_eq!(s.cards.len(), 2);
}
#[test]
fn duplicates_and_unknown_owners_are_rejected() {
    let mut s = roster();
    s.cards.push(CardRecord::imported(1001, 2));
    assert!(s.validate().is_err());
    s.cards.pop();
    s.cards[0].user_no = 77;
    assert!(s.validate().is_err());
    assert!(s.reserve(77).is_err());
}

#[test]
fn scanned_unassigned_number_is_reserved_before_a_user_is_chosen() {
    let mut s = roster();
    assert_eq!(s.observe_scan(&scan("00002A138A", false)).unwrap(), None);
    assert_eq!(s.reserve(1).unwrap(), 5003);
}

#[test]
fn full_mock_scan_allocate_write_and_independent_readback() {
    use proxmark_studio::{
        basic::{self, Action, Hint},
        capabilities::Capabilities,
        model::Settings,
        runner::{Event, Job, JobResult, Worker},
    };
    fn run(settings: &Settings, job: Job) -> JobResult {
        let w = Worker::start(settings.clone(), job);
        loop {
            if let Event::Finished(r) =
                w.rx.recv_timeout(std::time::Duration::from_secs(30))
                    .unwrap()
            {
                return *r;
            }
        }
    }
    fn guided(settings: &Settings, caps: &Capabilities, action: Action) -> JobResult {
        run(
            settings,
            Job::Guided(Box::new(basic::Request {
                action,
                caps: caps.clone(),
                simulated: true,
            })),
        )
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".mock-pm3-enable-state"), "").unwrap();
    std::fs::write(dir.path().join(".mock-pm3-card"), "na1").unwrap();
    let settings = Settings {
        cli: env!("CARGO_BIN_EXE_mock-pm3").into(),
        port: "SIMULATED".into(),
        working_dir: dir.path().display().to_string(),
        timeout_secs: 5,
        ..Default::default()
    };
    let caps = run(&settings, Job::Probe).capabilities.unwrap();
    let mut s = roster();
    s.simulated = true;
    let scanned = guided(&settings, &caps, Action::Read(Hint::Lf));
    assert_eq!(scanned.outcome, Outcome::Completed);
    assert_eq!(
        s.observe_scan(&scanned.evidence.card.unwrap()).unwrap(),
        Some(1)
    );
    // Clone the scanned original, preserving its identity and existing assignment.
    let original = netaccess::new_card(&roster().profile, 1001, true).unwrap();
    std::fs::write(dir.path().join(".mock-pm3-card"), "t5577").unwrap();
    let destination = guided(&settings, &caps, Action::Check(original.clone()))
        .evidence
        .destination
        .unwrap();
    s.begin_clone(1001, 1, original.em_identity().unwrap().0)
        .unwrap();
    let cloned = guided(
        &settings,
        &caps,
        Action::Write {
            card: original,
            destination,
            acknowledged: true,
        },
    );
    assert_eq!(cloned.outcome, Outcome::Verified);
    s.finish_write(1001, &cloned.outcome).unwrap();
    assert_eq!(s.cards.len(), 2);
    // Rewrite that credential to a reserved new number through the same checked adapter.
    s.change_number(1001, 1, 5002).unwrap();
    let replacement = netaccess::new_card(&roster().profile, 5002, true).unwrap();
    let destination = guided(&settings, &caps, Action::Check(replacement.clone()))
        .evidence
        .destination
        .unwrap();
    s.begin_write(5002, 1, replacement.em_identity().unwrap().0)
        .unwrap();
    let changed = guided(
        &settings,
        &caps,
        Action::Write {
            card: replacement,
            destination,
            acknowledged: true,
        },
    );
    assert_eq!(changed.outcome, Outcome::Verified);
    s.finish_write(5002, &changed.outcome).unwrap();
    assert!(s.card(1001).unwrap().deleted);
    for (fixture, verified) in [("t5577", true), ("ignore", false)] {
        let number = s.reserve(2).unwrap();
        let card = netaccess::new_card(&roster().profile, number, true).unwrap();
        std::fs::write(dir.path().join(".mock-pm3-card"), fixture).unwrap();
        let destination = guided(&settings, &caps, Action::Check(card.clone()))
            .evidence
            .destination
            .unwrap();
        s.begin_write(number, 2, card.em_identity().unwrap().0)
            .unwrap();
        let result = guided(
            &settings,
            &caps,
            Action::Write {
                card,
                destination,
                acknowledged: true,
            },
        );
        assert_eq!(result.outcome == Outcome::Verified, verified);
        s.finish_write(number, &result.outcome).unwrap();
        assert_eq!(
            s.card(number).unwrap().state,
            if verified {
                State::Verified
            } else {
                State::Uncertain
            }
        );
    }
}
#[test]
fn only_independent_verification_marks_programmed_and_requires_enrolment() {
    let mut s = roster();
    let n = s.reserve(1).unwrap();
    let id = netaccess::new_card(&roster().profile, n, false)
        .unwrap()
        .em_identity()
        .unwrap()
        .0
        .to_owned();
    assert!(s.begin_write(n, 2, &id).is_err());
    assert!(s.begin_write(n, 1, "00002A0001").is_err());
    s.begin_write(n, 1, &id).unwrap();
    s.finish_write(n, &Outcome::Completed).unwrap();
    assert_eq!(s.card(n).unwrap().state, State::Uncertain);
    s.begin_write(n, 1, &id).unwrap();
    s.finish_write(n, &Outcome::Verified).unwrap();
    assert_eq!(s.card(n).unwrap().state, State::Verified);
    assert!(s.card(n).unwrap().needs_enrolment);
    assert!(s.begin_write(n, 1, &id).is_err());
}
#[test]
fn repository_lock_reservations_and_interruption_survive_restart() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("na1.json");
    netaccess::save_roster(&path, &roster()).unwrap();
    let mut repo = Repository::open(&path, false).unwrap();
    assert!(Repository::open(&path, false).is_err());
    let n = repo.update(|s| s.reserve(1)).unwrap();
    let id = netaccess::new_card(&roster().profile, n, false)
        .unwrap()
        .em_identity()
        .unwrap()
        .0
        .to_owned();
    repo.update(|s| s.begin_write(n, 1, &id)).unwrap();
    drop(repo);
    let mut reopened = Repository::open(&path, false).unwrap();
    assert_eq!(reopened.data.card(n).unwrap().state, State::Uncertain);
    assert_eq!(reopened.update(|s| s.reserve(2)).unwrap(), n + 1);
}
#[test]
fn failed_transaction_leaves_previous_roster_intact() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("na1.json");
    netaccess::save_roster(&path, &roster()).unwrap();
    let mut repo = Repository::open(&path, false).unwrap();
    let before = std::fs::read(&path).unwrap();
    let result: Result<(), String> = repo.update(|s| {
        s.reserve(1)?;
        Err("abort".into())
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(repo.data.cards.len(), 2);
}
#[test]
fn allocator_checks_entire_16_bit_space_and_reports_exhaustion() {
    let mut s = roster();
    s.cards = (1..=u16::MAX).map(|n| CardRecord::imported(n, 1)).collect();
    assert!(s.reserve(2).is_err());
    s.cards.retain(|c| c.number != 400);
    assert_eq!(s.reserve(2).unwrap(), 400);
}

#[test]
fn user_edit_preserves_card_links_and_unknown_controller_number_stays_blank() {
    let mut s = roster();
    let key = s.add_user(" New Holder ", None).unwrap();
    assert_eq!(s.user(key).unwrap().name, "New Holder");
    assert_eq!(s.user(key).unwrap().controller_number, None);
    s.edit_user(1, "Renamed Person", Some(17)).unwrap();
    assert_eq!(s.card(1001).unwrap().user_no, 1);
    assert_eq!(s.user(1).unwrap().controller_number, Some(17));
    assert!(s.edit_user(key, "New Holder", Some(17)).is_err());
    assert!(s.add_user("", None).is_err());
    assert!(s.add_user("Person\nInjected", None).is_err());
    assert!(s.add_user("Another", Some(0)).is_err());
    s.reassign(1001, 1, key).unwrap();
    assert!(s.export_csv().contains(",\"New Holder\",42,1001,"));
    assert_eq!(s.changes.len(), 3);
    s.validate().unwrap();
}

#[test]
fn reassignment_preserves_ids_reservations_flags_and_changes_scan_owner() {
    let mut s = roster();
    let c = scan("12AB2A03E9", false);
    s.observe_scan(&c).unwrap();
    s.cards[0].enabled = Some(false);
    s.cards[0].expires_on = "01/01/2020".into();
    s.reassign(1001, 1, 2).unwrap();
    let assigned = s.card(1001).unwrap();
    assert_eq!(assigned.state, State::Imported);
    assert_eq!(assigned.observed_ids, vec!["12AB2A03E9"]);
    assert_eq!(assigned.enabled, Some(false));
    assert_eq!(assigned.expires_on, "01/01/2020");
    assert!(assigned.needs_enrolment);
    assert_eq!(s.observe_scan(&c).unwrap(), Some(2));
    assert!(s.changes[0].detail.contains("Example One → Example Two"));
    assert_eq!(s.reserve(1).unwrap(), 5002);
    assert_eq!(s.users.len(), 2);
}

#[test]
fn stale_invalid_or_inflight_reassignment_cannot_change_owner() {
    let mut s = roster();
    assert!(s.reassign(1001, 2, 1).is_err());
    assert!(s.reassign(1001, 1, 99).is_err());
    assert!(s.reassign(1001, 1, 1).is_err());
    assert!(s.reassign(999, 1, 2).is_err());
    assert!(s.changes.is_empty());
    let n = s.reserve(1).unwrap();
    let id = s.card(n).unwrap().planned_id.clone().unwrap();
    s.begin_write(n, 1, &id).unwrap();
    assert!(s.reassign(n, 1, 2).is_err());
    assert!(s.edit_user(1, "Changed", None).is_err());
    s.finish_write(n, &Outcome::Cancelled).unwrap();
    s.reassign(n, 1, 2).unwrap();
    assert!(s.begin_write(n, 1, &id).is_err());
    s.begin_write(n, 2, &id).unwrap();
}

#[test]
fn legacy_roster_upgrades_with_backup_and_edits_survive_restart() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("netaccess-na1.json");
    let mut legacy = serde_json::to_value(roster()).unwrap();
    legacy["schema"] = 1.into();
    legacy["site"] = "Legacy Site".into();
    legacy["facility"] = 42.into();
    legacy["encoding"] = "EM410x/Wiegand26".into();
    legacy.as_object_mut().unwrap().remove("profile");
    legacy.as_object_mut().unwrap().remove("changes");
    for u in legacy["users"].as_array_mut().unwrap() {
        u.as_object_mut().unwrap().remove("controller_number");
    }
    let original = serde_json::to_vec_pretty(&legacy).unwrap();
    std::fs::write(&path, &original).unwrap();
    let mut repo = Repository::open(&path, false).unwrap();
    assert_eq!(repo.data.schema, 3);
    assert_eq!(repo.data.user(1).unwrap().controller_number, Some(1));
    let backups = std::fs::read_dir(d.path())
        .unwrap()
        .map(|f| f.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("organisation-before-")
        })
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(std::fs::read(&backups[0]).unwrap(), original);
    repo.update(|s| {
        let new = s.add_user("New Recipient", None)?;
        s.reassign(1001, 1, new)?;
        s.edit_user(2, "Edited Recipient", Some(20))
    })
    .unwrap();
    let final_json = std::fs::read(&path).unwrap();
    drop(repo);
    let reopened = Repository::open(&path, false).unwrap();
    assert_eq!(reopened.data.changes.len(), 3);
    assert_eq!(reopened.data.card(1001).unwrap().user_no, 3);
    assert_eq!(reopened.data.user(2).unwrap().name, "Edited Recipient");
    assert_eq!(std::fs::read(&path).unwrap(), final_json);
}

#[test]
fn deleted_fobs_cannot_be_scanned_reassigned_or_reused() {
    let mut s = roster();
    s.delete_fob(5001, 2).unwrap();
    assert!(s.card(5001).unwrap().deleted);
    assert!(s.observe_scan(&scan("00002A1389", false)).is_err());
    assert!(s.reassign(5001, 2, 1).is_err());
    assert!(s.edit_fob(5001, "changed").is_err());
    assert!(s.delete_fob(5001, 2).is_err());
    assert_eq!(s.reserve(1).unwrap(), 5002);
    assert!(s.card(5001).unwrap().needs_enrolment);
    s.validate().unwrap();
}

#[test]
fn notes_edit_preserves_encoding_and_escapes_csv() {
    let mut s = roster();
    s.observe_scan(&scan("12AB2A03E9", false)).unwrap();
    s.edit_fob(1001, "=office, spare").unwrap();
    assert_eq!(s.card(1001).unwrap().observed_ids, vec!["12AB2A03E9"]);
    assert!(s.export_csv().contains("\"'=office, spare\""));
    assert!(s.edit_fob(1001, &"x".repeat(2001)).is_err());
}

#[test]
fn cloning_preserves_full_original_id_and_does_not_allocate_number() {
    let mut s = roster();
    assert!(s.begin_clone(1001, 1, "12AB2A03E9").is_err());
    s.observe_scan(&scan("12AB2A03E9", false)).unwrap();
    assert!(s.begin_clone(1001, 2, "12AB2A03E9").is_err());
    s.begin_clone(1001, 1, "12AB2A03E9").unwrap();
    assert!(s.delete_fob(1001, 1).is_err());
    s.finish_write(1001, &Outcome::Mismatch).unwrap();
    assert_eq!(s.card(1001).unwrap().state, State::Uncertain);
    assert_eq!(
        s.card(1001).unwrap().planned_id.as_deref(),
        Some("12AB2A03E9")
    );
    s.begin_clone(1001, 1, "12AB2A03E9").unwrap();
    s.finish_write(1001, &Outcome::Verified).unwrap();
    assert_eq!(s.cards.len(), 2);
    assert!(!s.card(1001).unwrap().deleted);
    assert!(!s.card(1001).unwrap().needs_enrolment);
}

#[test]
fn number_change_keeps_old_active_until_verified_and_never_reuses_it() {
    let mut s = roster();
    s.change_number(1001, 1, 5002).unwrap();
    assert!(s.change_number(1001, 1, 5001).is_err());
    assert!(s.change_number(1001, 1, 0).is_err());
    assert_eq!(s.card(5002).unwrap().replaces, Some(1001));
    let id = s.card(5002).unwrap().planned_id.clone().unwrap();
    s.begin_write(5002, 1, &id).unwrap();
    s.finish_write(5002, &Outcome::Failed("ignored".into()))
        .unwrap();
    assert!(!s.card(1001).unwrap().deleted);
    assert_eq!(s.card(5002).unwrap().state, State::Uncertain);
    s.begin_write(5002, 1, &id).unwrap();
    s.finish_write(5002, &Outcome::Verified).unwrap();
    assert!(s.card(1001).unwrap().deleted);
    assert_eq!(s.card(5002).unwrap().state, State::Verified);
    assert_eq!(s.observe_scan(&scan(&id, false)).unwrap(), Some(1));
    assert!(s.change_number(5002, 1, 1001).is_err());
    s.validate().unwrap();
}

#[test]
fn number_change_rejects_observed_numbers_and_stale_original_owner() {
    let mut s = roster();
    s.observe_scan(&scan("00002A138A", false)).unwrap();
    assert!(s.change_number(1001, 1, 5002).is_err());
    s.change_number(1001, 1, 5003).unwrap();
    let id = s.card(5003).unwrap().planned_id.clone().unwrap();
    s.reassign(1001, 1, 2).unwrap();
    assert!(s.begin_write(5003, 1, &id).is_err());
    assert_eq!(s.card(5003).unwrap().state, State::Reserved);
}
