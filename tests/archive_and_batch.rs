use proxmark_studio::{
    backup::Geometry,
    classic::{self, KeyMap, KeyType},
    command::CommandSpec,
    credential::{Format, Target},
    desfire_admin::{Action, Admin},
    desfire_archive::{self as d, Archive, Auth, Client, Credentials, File},
    mock_desfire::Card,
    model::{Algorithm, Channel, Communication},
    provision::{self, Batch, Status},
    runner::Outcome,
};
use std::collections::BTreeMap;
use zeroize::Zeroizing;
struct Device {
    card: Card,
    commands: Vec<String>,
}
impl Client for Device {
    fn run(&mut self, spec: CommandSpec) -> Result<Zeroizing<String>, Outcome> {
        self.commands.push(spec.text.to_string());
        self.card
            .run(&spec.text)
            .map(Zeroizing::new)
            .map_err(|e| Outcome::Failed(format!("{}: {e}", spec.label)))
    }
}
fn auth() -> Auth {
    Auth::zero(Algorithm::Aes, Channel::Ev1)
}
fn creds() -> Credentials {
    Credentials {
        fallback: auth(),
        keys: BTreeMap::new(),
    }
}
fn device() -> Device {
    Device {
        card: Card::default(),
        commands: vec![],
    }
}
fn rich_archive() -> Archive {
    let mut app = Card::default().archive.applications.remove(0);
    app.files[0].size = 500;
    app.files[0].data = (0..500).map(|n| (n % 251) as u8).collect();
    let mut backup = app.files[0].clone();
    backup.id = 2;
    backup.kind = 1;
    app.files.push(backup);
    let mut value = File::empty(3);
    value.kind = 2;
    value.lower = -50;
    value.upper = 500;
    value.value = Some(123);
    app.files.push(value);
    for kind in [3, 4] {
        let mut f = File::empty(kind + 1);
        f.kind = kind;
        f.record_size = 4;
        f.max_records = 5;
        f.records = vec![vec![9, 8, 7, 6], vec![1, 2, 3, 4]];
        app.files.push(f);
    }
    let mut out = Archive {
        schema: 1,
        uid: "04112233445566".into(),
        applications: vec![app],
        sha256: String::new(),
    };
    out.seal();
    out
}
fn restore_to_empty(a: &Archive) -> (Device, tempfile::TempDir) {
    let mut dev = device();
    dev.card.archive.applications.clear();
    dev.card.hashes.retain(|k, _| k.starts_with("000000"));
    let dir = tempfile::tempdir().unwrap();
    d::restore(
        &mut dev,
        a,
        &a.uid,
        &creds(),
        &dir.path().join("journal.json"),
    )
    .unwrap();
    (dev, dir)
}
#[test]
fn capture_all_file_types_and_sizes_matches_card_state() {
    let a = rich_archive();
    let mut dev = device();
    dev.card.archive = a.clone();
    let out = d::capture(&mut dev, &a.uid, &creds()).unwrap();
    assert_eq!(out.applications, a.applications);
    out.validate().unwrap();
    let json = serde_json::to_string(&out).unwrap();
    assert!(!json.contains("00000000000000000000000000000000"));
    assert!(!json.contains("alias"));
}
#[test]
fn restore_all_supported_file_types_then_fresh_capture() {
    let a = rich_archive();
    let (dev, dir) = restore_to_empty(&a);
    assert_eq!(dev.card.archive.applications, a.applications);
    let j: d::Journal =
        serde_json::from_slice(&std::fs::read(dir.path().join("journal.json")).unwrap()).unwrap();
    assert!(j.completed.contains("complete"));
    assert!(j.pending.is_none());
    assert!(
        dev.commands
            .iter()
            .filter(|s| s.contains("hf mfdes read"))
            .count()
            > 5
    );
}
#[test]
fn second_restore_rechecks_but_does_not_rewrite_completed_state() {
    let a = rich_archive();
    let (mut dev, dir) = restore_to_empty(&a);
    let before = dev.commands.len();
    d::restore(
        &mut dev,
        &a,
        &a.uid,
        &creds(),
        &dir.path().join("journal.json"),
    )
    .unwrap();
    let writes = &dev.commands[before..];
    assert!(!writes.iter().any(|s| [
        " createapp ",
        " write ",
        " changekey ",
        " clearrecfile ",
        " chfilesettings "
    ]
    .iter()
    .any(|p| s.contains(p))));
    assert!(writes.iter().any(|s| s.contains(" read ")));
}
#[test]
fn interruption_resume_rechecks_values_and_records_without_double_application() {
    let a = rich_archive();
    for fail in [12, 25, 40, 60, 80] {
        let mut dev = device();
        dev.card.archive.applications.clear();
        dev.card.fail_after = Some(fail);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.json");
        let first = d::restore(&mut dev, &a, &a.uid, &creds(), &path);
        assert!(first.is_err(), "injection {fail} not reached");
        dev.card.fail_after = None;
        d::restore(&mut dev, &a, &a.uid, &creds(), &path)
            .unwrap_or_else(|e| panic!("resume at {fail}: {e:?}"));
        assert_eq!(dev.card.archive.applications, a.applications);
    }
}
#[test]
fn wrong_uid_refuses_restore_before_any_mutation() {
    let a = rich_archive();
    let mut dev = device();
    let dir = tempfile::tempdir().unwrap();
    assert!(d::restore(
        &mut dev,
        &a,
        "04992233445566",
        &creds(),
        &dir.path().join("j.json")
    )
    .is_err());
    assert!(!dev.commands.iter().any(|s| s.contains(" write ")));
}
#[test]
fn stale_journal_is_bound_to_target_and_archive() {
    let a = rich_archive();
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("j.json");
    d::Journal::start(&a, &a.uid, &p).unwrap();
    assert!(d::Journal::start(&a, "04992233445566", &p).is_err());
    let mut altered = a.clone();
    altered.applications[0].files[0].data[0] ^= 1;
    altered.seal();
    assert!(d::Journal::start(&altered, &a.uid, &p).is_err());
}
#[test]
fn corrupt_or_incomplete_archives_are_rejected() {
    let a = rich_archive();
    let mut broken = a.clone();
    broken.applications[0].files[0].data.pop();
    broken.seal();
    assert!(broken.validate().is_err());
    let mut broken = a.clone();
    broken.applications[0].files[0].kind = 5;
    broken.seal();
    assert!(broken.validate().is_err());
    let mut broken = a;
    broken.sha256 = "fake".into();
    assert!(broken.validate().is_err());
}
#[test]
fn no_known_key_means_no_complete_backup() {
    let mut dev = device();
    let mut bad = creds();
    bad.fallback.key = Zeroizing::new("11".repeat(16));
    assert!(d::capture(&mut dev, "04112233445566", &bad).is_err());
}
#[test]
fn extended_settings_are_not_silently_downgraded() {
    assert!(d::app_settings("[+] GetKeySettings[3]: 0F 91 00").is_err());
    assert!(d::file_settings("[+] settings[7]: 00 80 EE EE 04 00 00", 1).is_err());
    assert!(d::file_settings("[+] settings[7]: 00 00 EE EE 04 00", 1).is_err());
}
#[test]
fn key_versions_are_preserved_and_verified() {
    let mut a = rich_archive();
    a.applications[0].key_versions[0] = 0x42;
    a.seal();
    let (dev, _) = restore_to_empty(&a);
    assert_eq!(dev.card.archive.applications[0].key_versions, [0x42]);
    assert!(dev.commands.iter().any(|s| s.contains("--newver 42")));
}
#[test]
fn ignored_mutation_cannot_verify() {
    let mut dev = device();
    dev.card.ignore_writes = true;
    let admin = Admin {
        action: Action::WriteData,
        data: "01020304".into(),
        ..Default::default()
    };
    let p = params();
    let write = admin.build(&p).unwrap();
    let m = d::Mutation {
        admin,
        auth: auth(),
        aid: "112233".into(),
        fid: 1,
        write,
    };
    assert!(matches!(
        d::verify_mutation(&mut dev, "04112233445566", m),
        Err(Outcome::Mismatch)
    ));
}
fn params() -> proxmark_studio::command::DesfireParams<'static> {
    proxmark_studio::command::DesfireParams {
        aid: "112233",
        fid: "01",
        key: "00000000000000000000000000000000",
        key_number: 0,
        algorithm: Algorithm::Aes,
        channel: Channel::Ev1,
        communication: Communication::Plain,
        no_auth: false,
        backup_file: false,
    }
}
#[test]
fn key_change_requires_new_authentication_and_version() {
    let mut dev = device();
    let admin = Admin {
        action: Action::ChangeKey,
        newkey: Zeroizing::new("11".repeat(16)),
        oldkey: Zeroizing::new("00".repeat(16)),
        version: "17".into(),
        ..Default::default()
    };
    let write = admin.build(&params()).unwrap();
    d::verify_mutation(
        &mut dev,
        "04112233445566",
        d::Mutation {
            admin,
            auth: auth(),
            aid: "112233".into(),
            fid: 1,
            write,
        },
    )
    .unwrap();
    assert_eq!(dev.card.archive.applications[0].key_versions, [0x17]);
    assert!(dev
        .commands
        .iter()
        .any(|s| s.contains("hf mfdes auth") && s.contains(&"11".repeat(16))));
}
#[test]
fn file_delete_is_verified_from_new_inventory() {
    let mut dev = device();
    let admin = Admin {
        action: Action::DeleteFile,
        ..Default::default()
    };
    let write = admin.build(&params()).unwrap();
    d::verify_mutation(
        &mut dev,
        "04112233445566",
        d::Mutation {
            admin,
            auth: auth(),
            aid: "112233".into(),
            fid: 1,
            write,
        },
    )
    .unwrap();
    assert!(dev.card.archive.applications[0].files.is_empty());
}
#[test]
fn classic_4k_sector_boundaries_and_key_roles() {
    assert_eq!(classic::sector(127).unwrap(), 31);
    assert_eq!(classic::sector(128).unwrap(), 32);
    assert_eq!(classic::sector(143).unwrap(), 32);
    assert_eq!(classic::sector(144).unwrap(), 33);
    assert_eq!(classic::trailer(39).unwrap(), 255);
    assert!(classic::sector(256).is_err());
    let m = KeyMap::parse(
        "32,A,FFFFFFFFFFFF,B,A0A1A2A3A4A5\n33,B,112233445566,A,000000000000",
        Geometry::Classic4K,
    )
    .unwrap();
    assert_eq!(m.get(143).unwrap().write_type, KeyType::B);
    assert_eq!(m.get(144).unwrap().read_type, KeyType::B);
    assert_eq!(m.get(144).unwrap().read_key.as_str(), "112233445566");
    assert!(m.get(0).is_err());
}
#[test]
fn classic_map_rejects_duplicates_invalid_keys_and_wrong_geometry() {
    for input in [
        "0,A,FFFFFFFFFFFF,B,FFFFFFFFFFFF\n0,A,FFFFFFFFFFFF,B,FFFFFFFFFFFF",
        "16,A,FFFFFFFFFFFF,B,FFFFFFFFFFFF",
        "0,C,FFFFFFFFFFFF,B,FFFFFFFFFFFF",
        "0,A,no-key,B,FFFFFFFFFFFF",
    ] {
        assert!(KeyMap::parse(input, Geometry::Classic1K).is_err());
    }
}
#[test]
fn classic_access_bit_redundancy_is_validated() {
    assert_eq!(
        classic::access_bits(&[0xff, 0x07, 0x80]).unwrap(),
        [0, 0, 0, 1]
    );
    assert!(classic::access_bits(&[0xfe, 0x07, 0x80]).is_err());
}
#[test]
fn csv_duplicate_credentials_and_ids_are_rejected() {
    let header = "id,format,facility,card,version,target\r\n";
    assert!(Batch::csv(&format!(
        "{header}a,H10301,118,1603,,T5577\r\nb,H10301,118,1603,,Q5"
    ))
    .is_err());
    assert!(Batch::csv(&format!(
        "{header}a,H10301,118,1603,,T5577\r\na,H10301,118,1604,,Q5"
    ))
    .is_err());
    assert!(Batch::csv(&format!("{header}a,H10301,256,1603,,T5577")).is_err());
}
#[test]
fn csv_quotes_crlf_and_report_formula_safety() {
    let b =
        Batch::csv("id,format,facility,card,version,target\r\n\"a,b\",H10301,118,1603,,T5577\r\n")
            .unwrap();
    assert_eq!(b.rows[0].id, "a,b");
    assert!(provision::csv_cell("=2+2").starts_with("\"'="));
    assert!(provision::parse_csv("\"unclosed").is_err());
}
#[test]
fn ranges_refuse_overflow_and_format_truncation() {
    assert!(Batch::range(Format::H10301, Target::T5577, 118, 65535, 2).is_err());
    assert!(Batch::range(Format::H10302, Target::T5577, 0, u64::MAX, 2).is_err());
    let b = Batch::range(Format::H10304, Target::Q5, 1234, 100, 3).unwrap();
    assert_eq!(b.rows.len(), 3);
    assert_eq!(b.next(), Some(0));
}
#[test]
fn interrupted_provisioning_does_not_auto_replay_or_skip_unknown_rows() {
    let mut b = Batch::range(Format::H10301, Target::T5577, 118, 1, 3).unwrap();
    b.rows[0].status = Status::Verified;
    b.rows[1].status = Status::InProgress;
    let loaded = Batch::load(&serde_json::to_vec(&b).unwrap()).unwrap();
    assert_eq!(loaded.rows[1].status, Status::Unknown);
    assert_eq!(loaded.next(), Some(2));
    assert_eq!(loaded.rows[0].status, Status::Verified);
}
#[test]
fn equivalent_custom_and_standard_payload_is_duplicate() {
    let mut b = Batch::range(Format::H10301, Target::T5577, 118, 1603, 1).unwrap();
    let p = proxmark_studio::custom_format::Profile {
        layout: Default::default(),
        facility_code: 118,
        card_number: 1603,
        target: Target::T5577,
    };
    b.rows.push(provision::Row {
        id: "custom".into(),
        value: provision::Value::Custom(p),
        status: Status::Pending,
        result: String::new(),
    });
    assert!(b.validate().is_err());
}
#[test]
fn classic_workbench_uses_distinct_read_and_write_keys() {
    let mut w = proxmark_studio::workbench::Workbench {
        geometry: Geometry::Classic4K,
        uid: "04112233445566".into(),
        unit: 144,
        use_map: true,
        key_map: Zeroizing::new("33,A,112233445566,B,A0A1A2A3A4A5".into()),
        ..Default::default()
    };
    w.dump = Some(proxmark_studio::backup::Dump::new(w.geometry, &w.uid, vec![0; 4096]).unwrap());
    let job = w.build(true, true, false).unwrap();
    if let proxmark_studio::runner::Job::Batch(b) = job {
        assert!(b.steps[0].read.text.contains(" -a -k 112233445566"));
        assert!(b.steps[0]
            .write
            .as_ref()
            .unwrap()
            .text
            .contains(" -b -k A0A1A2A3A4A5"));
    } else {
        panic!("Expected memory batch")
    }
}
#[test]
fn geometry_change_cannot_panic_when_selected_block_is_outside_new_card() {
    let w = proxmark_studio::workbench::Workbench {
        geometry: Geometry::Classic1K,
        uid: "04112233445566".into(),
        unit: 200,
        ..Default::default()
    };
    assert!(w.build(false, true, false).is_err());
}
#[test]
fn credential_csv_exports_and_reimports_without_losing_numbers() {
    let b = Batch::range(Format::H10302, Target::Em4305, 0, 34359738365, 3).unwrap();
    let loaded = Batch::csv(&b.export_csv().unwrap()).unwrap();
    assert_eq!(
        loaded.rows[2].value.validate().unwrap(),
        b.rows[2].value.validate().unwrap()
    );
}
#[test]
fn verification_of_value_and_record_mutations_is_semantic() {
    for action in [
        Action::Credit,
        Action::Debit,
        Action::LimitedCredit,
        Action::ClearValue,
        Action::WriteRecord,
        Action::ClearRecords,
    ] {
        let mut dev = device();
        dev.card.archive = rich_archive();
        if action == Action::LimitedCredit {
            dev.card.archive.applications[0].files[2].limited = 1;
        }
        let record = matches!(action, Action::WriteRecord | Action::ClearRecords);
        let fid = if record { 4 } else { 3 };
        let p = proxmark_studio::command::DesfireParams {
            fid: if record { "04" } else { "03" },
            ..params()
        };
        let admin = Admin {
            action,
            value: 2,
            data: "05060708".into(),
            ..Default::default()
        };
        let write = admin.build(&p).unwrap();
        d::verify_mutation(
            &mut dev,
            "04112233445566",
            d::Mutation {
                admin,
                auth: auth(),
                aid: "112233".into(),
                fid,
                write,
            },
        )
        .unwrap();
    }
}
#[test]
fn protected_files_resume_before_keys_have_finished_installing() {
    let mut a = rich_archive();
    a.applications[0].key_settings2 = 0x82;
    a.applications[0].key_versions = vec![3, 4];
    for f in &mut a.applications[0].files {
        f.rights = 0x1100;
    }
    a.seal();
    let mut credentials = creds();
    for n in 0..2 {
        let mut key = auth();
        key.key_number = n;
        key.key = Zeroizing::new(if n == 0 {
            "11".repeat(16)
        } else {
            "22".repeat(16)
        });
        credentials.keys.insert(("112233".into(), n), key);
    }
    for fail in [20, 45, 70, 100] {
        let mut dev = device();
        dev.card.archive.applications.clear();
        dev.card.fail_after = Some(fail);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("j.json");
        let _ = d::restore(&mut dev, &a, &a.uid, &credentials, &p);
        dev.card.fail_after = None;
        d::restore(&mut dev, &a, &a.uid, &credentials, &p)
            .unwrap_or_else(|e| panic!("Resume with protected keys at {fail}: {e:?}"));
        assert_eq!(dev.card.archive.applications, a.applications);
    }
}
#[test]
fn iso_application_and_file_ids_are_preserved() {
    let mut a = rich_archive();
    a.applications[0].key_settings2 |= 0x20;
    a.applications[0].iso_id = Some("DF01".into());
    a.applications[0].df_name = Some("617070".into());
    a.applications[0].files[0].iso_id = Some(0xe103);
    a.seal();
    let (mut dev, _) = restore_to_empty(&a);
    let captured = d::capture(&mut dev, &a.uid, &creds()).unwrap();
    assert_eq!(captured.applications, a.applications);
}
#[test]
fn memory_eml_and_rrg_binary_headers_import_with_exact_geometry() {
    let d =
        proxmark_studio::backup::Dump::new(Geometry::Ntag213, "04112233445566", vec![0x42; 180])
            .unwrap();
    let eml = d.eml().unwrap();
    assert_eq!(
        proxmark_studio::backup::import_memory(eml.as_bytes(), "eml", d.geometry, &d.uid)
            .unwrap()
            .bytes,
        d.bytes
    );
    let mut native = vec![0; 56];
    native.extend_from_slice(&d.bytes);
    assert_eq!(
        proxmark_studio::backup::import_memory(&native, "bin", d.geometry, &d.uid)
            .unwrap()
            .bytes,
        d.bytes
    );
    native.pop();
    assert!(proxmark_studio::backup::import_memory(&native, "bin", d.geometry, &d.uid).is_err());
}
#[test]
fn file_type_and_access_rules_filter_management_actions() {
    let mut f = File::empty(1);
    f.size = 4;
    f.data = vec![0; 4];
    f.rights = 0x1200;
    assert!(Action::ReadData.available(Some(&f), 1, false));
    assert!(!Action::ReadData.available(Some(&f), 2, false));
    assert!(!Action::ReadValue.available(Some(&f), 1, false));
    assert!(!Action::ReadData.available(Some(&f), 1, true));
    f.rights = 0xeeee;
    assert!(Action::ReadData.available(Some(&f), 0, true));
}
#[test]
fn idle_key_expiry_and_catalogue_do_not_include_secrets() {
    use std::time::Duration;
    assert!(!proxmark_studio::vault::expired(
        Duration::from_secs(299),
        300
    ));
    assert!(proxmark_studio::vault::expired(
        Duration::from_secs(300),
        300
    ));
    assert!(!proxmark_studio::vault::expired(
        Duration::from_secs(10000),
        0
    ));
    let mut records = vec![];
    proxmark_studio::vault::upsert(
        &mut records,
        proxmark_studio::vault::Record {
            alias: "test-key".into(),
            bytes: 16,
            version: 1,
            note: "App key reference".into(),
        },
    )
    .unwrap();
    let json = serde_json::to_string(&records).unwrap();
    assert!(!json.contains("password"));
    assert!(!json.contains("secret"));
    assert!(json.contains("test-key"));
}
#[test]
fn malicious_root_application_or_algorithm_mismatch_is_refused() {
    let mut a = rich_archive();
    a.applications[0].aid = "000000".into();
    a.seal();
    assert!(a.validate().is_err());
    let mut a = rich_archive();
    a.applications[0].algorithm = Algorithm::Des;
    a.seal();
    assert!(a.validate().is_err());
}
#[test]
fn rrg_format_catalogue_preserves_named_formats_and_strict_encodings() {
    use proxmark_studio::format_catalogue as f;
    let list =
        f::list("[=] Name Description\n[=] Kastle Kastle 32-bit\n[=] H10301 HID H10301 26-bit\n")
            .unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(
        f::encoded("[+] Wiegand: 2142CA0A72").unwrap(),
        "000000000000002142CA0A72"
    );
    assert!(f::encoded("[!] The card data could not be encoded").is_err());
    assert!(f::encoded("[+] Wiegand: 01\n[+] Wiegand: 02").is_err());
    assert!(f::list("[=] Fake descriptive text without a bit count").is_err());
}
#[test]
fn rrg_format_names_cannot_inject_another_command() {
    let mut c = proxmark_studio::format_catalogue::Credential {
        format: "H10301;quit".into(),
        ..Default::default()
    };
    assert!(c.encode().is_err());
    c.format = "Kastle".into();
    c.issue = 1;
    c.oem = 2;
    let command = c.encode().unwrap();
    assert!(command.text.contains("--issue 1 --oem 2"));
}
#[test]
fn unsupported_free_value_flags_are_not_accepted_as_ev1() {
    let mut a = rich_archive();
    a.applications[0].files[2].limited = 2;
    a.seal();
    assert!(a.validate().is_err());
    let admin = Admin {
        limited_credit: 2,
        ..Default::default()
    };
    assert!(admin.build(&params()).is_err());
}
#[test]
fn classic_access_encoder_matches_transport_configuration_and_roundtrips_all_groups() {
    assert_eq!(
        classic::encode_access([0, 0, 0, 1]).unwrap(),
        [0xff, 0x07, 0x80]
    );
    assert_eq!(
        classic::encode_access([0, 0, 0, 3]).unwrap(),
        [0x7f, 0x07, 0x88]
    );
    for value in 0..4096 {
        let groups = [
            (value & 7) as u8,
            ((value >> 3) & 7) as u8,
            ((value >> 6) & 7) as u8,
            ((value >> 9) & 7) as u8,
        ];
        assert_eq!(
            classic::access_bits(&classic::encode_access(groups).unwrap()).unwrap(),
            groups
        );
    }
}
#[test]
fn resume_after_key_change_was_sent_but_confirmation_was_interrupted() {
    let mut archive = rich_archive();
    archive.applications[0].key_settings2 = 0x82;
    archive.applications[0].key_versions = vec![5, 7];
    for f in &mut archive.applications[0].files {
        f.rights = 0x1100;
    }
    archive.seal();
    let mut credentials = creds();
    for n in 0..2 {
        let mut k = auth();
        k.key_number = n;
        k.key = Zeroizing::new(if n == 0 {
            "11".repeat(16)
        } else {
            "22".repeat(16)
        });
        credentials.keys.insert(("112233".into(), n), k);
    }
    let mut initial = device();
    initial.card.archive.applications.clear();
    let dir = tempfile::tempdir().unwrap();
    d::restore(
        &mut initial,
        &archive,
        &archive.uid,
        &credentials,
        &dir.path().join("base.json"),
    )
    .unwrap();
    let fault_positions = initial
        .commands
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.contains(" changekey ").then_some(i + 2))
        .collect::<Vec<_>>();
    assert_eq!(fault_positions.len(), 2);
    for fail in fault_positions {
        let mut dev = device();
        dev.card.archive.applications.clear();
        dev.card.fail_after = Some(fail);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("restore.json");
        assert!(d::restore(&mut dev, &archive, &archive.uid, &credentials, &path).is_err());
        dev.card.fail_after = None;
        d::restore(&mut dev, &archive, &archive.uid, &credentials, &path).unwrap();
        assert_eq!(dev.card.archive.applications, archive.applications);
    }
}
#[test]
fn concurrent_restore_cannot_share_one_journal() {
    struct Contention {
        inner: Device,
        archive: Archive,
        path: std::path::PathBuf,
        checked: bool,
    }
    impl Client for Contention {
        fn run(&mut self, spec: CommandSpec) -> Result<Zeroizing<String>, Outcome> {
            if !self.checked {
                self.checked = true;
                let mut other = device();
                let outcome = d::restore(
                    &mut other,
                    &self.archive,
                    &self.archive.uid,
                    &creds(),
                    &self.path,
                );
                assert!(
                    matches!(outcome,Err(Outcome::Failed(e))if e.contains("restore")||e.contains("locked"))
                );
                assert!(other.commands.is_empty());
            }
            self.inner.run(spec)
        }
    }
    let archive = rich_archive();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.json");
    let mut inner = device();
    inner.card.archive.applications.clear();
    let mut c = Contention {
        inner,
        archive: archive.clone(),
        path: path.clone(),
        checked: false,
    };
    d::restore(&mut c, &archive, &archive.uid, &creds(), &path).unwrap();
    assert!(c.checked);
}

#[test]
fn binary_iso_names_refuse_restore_before_any_mutation() {
    let mut a = rich_archive();
    a.applications[0].key_settings2 |= 0x20;
    a.applications[0].iso_id = Some("DF01".into());
    a.applications[0].df_name = Some("D2760000850101".into());
    a.seal();
    let mut dev = device();
    dev.card.archive.applications.clear();
    let dir = tempfile::tempdir().unwrap();
    let result = d::restore(&mut dev, &a, &a.uid, &creds(), &dir.path().join("j.json"));
    assert!(matches!(result, Err(Outcome::Failed(e)) if e.contains("binary ISO DF")));
    assert!(dev.commands.is_empty());
}

#[test]
fn app_creation_verifies_iso_identifiers_from_fresh_inventory() {
    let mut dev = device();
    dev.card.archive.applications.clear();
    let admin = Admin {
        action: Action::CreateApp,
        iso_id: "DF01".into(),
        df_name: "617070".into(),
        ..Default::default()
    };
    let write = admin.build(&params()).unwrap();
    d::verify_mutation(
        &mut dev,
        "04112233445566",
        d::Mutation {
            admin,
            auth: auth(),
            aid: "112233".into(),
            fid: 1,
            write,
        },
    )
    .unwrap();
    assert_eq!(
        dev.card.archive.applications[0].iso_id.as_deref(),
        Some("DF01")
    );
    assert!(dev.commands.iter().any(|c| c.contains("getappnames")));
}
