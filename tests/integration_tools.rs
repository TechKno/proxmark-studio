#![allow(clippy::field_reassign_with_default)]
use proxmark_studio::{
    backup::{self, Dump, Geometry},
    capabilities::Capabilities,
    emulator::{self, Action, Image, Request},
    explorer, memory_tools,
    model::Settings,
    runner::{Event, Job, JobResult, Outcome, Worker},
    signal::Signal,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use zeroize::Zeroizing;

fn image(g: Geometry) -> Image {
    let mut b = vec![0; g.units() as usize * 16];
    b[..4].copy_from_slice(&[1, 2, 3, 4]);
    for s in 0..g.sectors() {
        let o = proxmark_studio::classic::trailer(s).unwrap() as usize * 16;
        b[o..o + 6].fill(255);
        b[o + 6..o + 10].copy_from_slice(&[255, 7, 128, 105]);
        b[o + 10..o + 16].fill(255);
    }
    Image {
        name: "Synthetic lab image".into(),
        dump: Dump::new(g, "01020304", b).unwrap(),
        simulated: true,
    }
}
#[test]
fn image_file_roundtrip_preserves_demo_marker_in_physical_context() {
    let original = image(Geometry::ClassicMini);
    let bytes = serde_json::to_vec(&original).unwrap();
    let imported = emulator::import_image(
        &bytes,
        "json",
        Geometry::Classic4K,
        "11223344",
        "Replacement",
        false,
    )
    .unwrap();
    assert_eq!(imported, original);
    let raw = serde_json::to_vec(&original.dump).unwrap();
    let imported_raw = emulator::import_image(
        &raw,
        "json",
        Geometry::ClassicMini,
        "01020304",
        "Raw import",
        false,
    )
    .unwrap();
    assert!(!imported_raw.simulated);
    assert_eq!(imported_raw.dump, original.dump);
    let mut broken: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    broken.as_object_mut().unwrap().remove("simulated");
    assert!(emulator::import_image(
        &serde_json::to_vec(&broken).unwrap(),
        "json",
        Geometry::ClassicMini,
        "01020304",
        "Raw import",
        false
    )
    .is_err());
}

#[test]
fn explorer_cannot_bypass_managed_simulation_workflow() {
    let result = run(
        &Settings::default(),
        Job::ExplorerRun {
            spec: proxmark_studio::command::CommandSpec::simple(
                "hf mf sim --mini",
                proxmark_studio::command::Kind::Console,
            ),
            device: false,
        },
    );
    assert!(matches!(result.outcome, Outcome::Failed(ref e) if e.contains("managed emulator")));
}
fn run(s: &Settings, job: Job) -> JobResult {
    let w = Worker::start(s.clone(), job);
    loop {
        match w.rx.recv_timeout(Duration::from_secs(40)).unwrap() {
            Event::Finished(r) => return *r,
            Event::Log(_) => {}
        }
    }
}
fn fixture() -> (tempfile::TempDir, Settings, Capabilities) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(".mock-pm3-enable-state"), b"").unwrap();
    let s = Settings {
        cli: env!("CARGO_BIN_EXE_mock-pm3").into(),
        port: "SIMULATED".into(),
        working_dir: d.path().display().to_string(),
        timeout_secs: 3,
        ..Default::default()
    };
    let r = run(&s, Job::Probe);
    assert_eq!(r.outcome, Outcome::Completed);
    (d, s, r.capabilities.unwrap())
}
fn request(action: Action, caps: &Capabilities) -> Job {
    Job::Emulator(Request {
        action,
        caps: caps.clone(),
        simulated: true,
        acknowledged: true,
    })
}

#[test]
fn catalogue_search_is_dynamic_and_does_not_accept_injected_names() {
    let e=explorer::parse_catalogue("command | offline | description\nhf search | N | Find HF tags\nwiegand decode | Y | Decode credentials\nhf search;quit | Y | bad").unwrap();
    assert_eq!(e.len(), 2);
    assert_eq!(
        explorer::parse_catalogue("hf search | N | same\nhf search | N | same")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(explorer::filtered(&e, "find tags", "hf", None).len(), 1);
    assert!(explorer::filtered(&e, "decode", "hf", None).is_empty());
    assert!(explorer::parse_catalogue("hf search | N | a\nhf search | N | b").is_err());
    assert!(explorer::help_command("hf search; quit").is_err());
}
#[test]
fn option_forms_use_advertised_flags_and_reject_separators_and_ev2() {
    let entry = explorer::Entry {
        name: "hf mf rdbl".into(),
        description: String::new(),
        offline: false,
    };
    let help="usage:\n hf mf rdbl [-h] --blk <dec> [-k <hex>]\noptions:\n -h, --help    This help\n -b, --blk <dec>    Block\n -k <hex>    Known key\nexamples/notes:\n hf mf rdbl --blk 1";
    let fields = explorer::options(&entry.name, help).unwrap();
    assert_eq!(fields.len(), 2);
    assert!(fields[0].required);
    let mut enabled = BTreeSet::from(["--blk".into()]);
    let mut values = BTreeMap::from([("--blk".into(), Zeroizing::new("1".into()))]);
    assert_eq!(
        &*explorer::build(&entry, &fields, &enabled, &values)
            .unwrap()
            .text,
        "hf mf rdbl --blk 1"
    );
    for v in ["1;quit", "1\nquit", "\"1\"", "--help", "1 2"] {
        values.insert("--blk".into(), Zeroizing::new(v.into()));
        assert!(explorer::build(&entry, &fields, &enabled, &values).is_err());
    }
    enabled.insert("--unknown".into());
    assert!(explorer::build(&entry, &fields, &enabled, &values).is_err());
    let unsupported = explorer::Entry {
        name: "hf mfdes pc".into(),
        description: String::new(),
        offline: false,
    };
    assert!(explorer::build(&unsupported, &[], &BTreeSet::new(), &BTreeMap::new()).is_err());
    assert!(explorer::options("lf search", help).is_err());
}
#[test]
fn favourites_never_store_arguments_and_old_settings_load() {
    let mut names = vec![];
    explorer::toggle_favourite(&mut names, "hf mf rdbl").unwrap();
    assert!(explorer::toggle_favourite(&mut names, "hf mf rdbl -k AABBCCDDEEFF").is_err());
    assert_eq!(names, vec!["hf mf rdbl"]);
    let old: Settings = serde_json::from_str("{\"cli\":\"proxmark3\"}").unwrap();
    assert!(old.emulator_images.is_empty());
    assert!(!old.rf_may_be_active);
    let mut s = old;
    s.command_favourites = names;
    s.emulator_images.push(image(Geometry::ClassicMini));
    s.rf_may_be_active = true;
    let next: Settings = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    assert!(next.rf_may_be_active);
    next.emulator_images[0].validate().unwrap();
}
#[test]
fn mini_and_2k_geometries_and_key_maps_have_exact_boundaries() {
    for (g, units, sectors) in [
        (Geometry::ClassicMini, 20, 5),
        (Geometry::Classic2K, 128, 32),
    ] {
        assert_eq!(g.units(), units);
        assert_eq!(g.sectors(), sectors);
        assert!(!g.writable(0));
        assert!(!g.writable(units - 1));
        assert!(!g.writable(units));
        assert!(g.writable(1));
        let good = format!("{},A,FFFFFFFFFFFF,B,FFFFFFFFFFFF", sectors - 1);
        assert!(proxmark_studio::classic::KeyMap::parse(&good, g).is_ok());
        assert!(proxmark_studio::classic::KeyMap::parse(
            &format!("{sectors},A,FFFFFFFFFFFF,B,FFFFFFFFFFFF"),
            g
        )
        .is_err());
        let i = image(g);
        assert_eq!(
            backup::import_memory(&i.dump.bytes, "dump", g, "01020304").unwrap(),
            i.dump
        );
        assert!(backup::import_memory(
            &i.dump.bytes[..i.dump.bytes.len() - 1],
            "bin",
            g,
            "01020304"
        )
        .is_err());
    }
}
#[test]
fn selection_is_bounded_sorted_and_never_writes_protected_units() {
    assert_eq!(
        memory_tools::selection("8,4-6,5", Geometry::ClassicMini).unwrap(),
        vec![4, 5, 6, 8]
    );
    for s in ["", "1,", "5-3", "0-20", "-1", "1;2", "99999999999999999999"] {
        assert!(memory_tools::selection(s, Geometry::ClassicMini).is_err());
    }
    let mut w = proxmark_studio::workbench::Workbench::default();
    w.geometry = Geometry::ClassicMini;
    w.uid = "01020304".into();
    *w.key = "FFFFFFFFFFFF".into();
    w.dump = Some(image(w.geometry).dump);
    w.selection = "0,1".into();
    assert!(w.build_selection(true, false).is_err());
    w.selection = "1,2".into();
    assert!(w.build_selection(true, false).is_ok());
}
#[test]
fn local_draft_edits_undo_redo_and_search_preserve_source() {
    let source = image(Geometry::ClassicMini).dump;
    let mut edited =
        memory_tools::edit_unit(&source, 1, "4142434445464748494A4B4C4D4E4F50", false).unwrap();
    assert_ne!(source.bytes, edited.bytes);
    assert_eq!(
        memory_tools::search(&edited, "ABCD", true).unwrap(),
        vec![16]
    );
    assert_eq!(
        memory_tools::search(&edited, "41424344", false).unwrap(),
        vec![16]
    );
    assert!(memory_tools::edit_unit(&source, 0, &"00".repeat(16), false).is_err());
    assert!(memory_tools::edit_unit(&source, 0, &"00".repeat(16), true).is_ok());
    let mut h = memory_tools::DraftHistory::default();
    h.record(&source);
    assert!(h.undo(&mut edited));
    assert_eq!(edited, source);
    assert!(h.redo(&mut edited));
    assert_ne!(edited, source);
    edited.validate().unwrap();
}
#[test]
fn permission_decoder_handles_large_sector_groups_and_invalid_complements() {
    let d = image(Geometry::Classic4K).dump;
    for (block, group) in [
        (128, 0),
        (132, 0),
        (133, 1),
        (137, 1),
        (138, 2),
        (142, 2),
        (143, 3),
    ] {
        assert!(
            memory_tools::permissions(&d, block).unwrap()[0].contains(&format!("group {group}"))
        );
    }
    let mut broken = d.clone();
    broken.bytes[143 * 16 + 6] ^= 1;
    broken.sha256 = broken.digest();
    assert!(memory_tools::permissions(&broken, 128).is_err());
    assert!(memory_tools::permissions(&d, 143)
        .unwrap()
        .iter()
        .any(|s| s.contains("never readable")));
}
#[test]
fn partial_selection_never_replaces_complete_dump() {
    let mut w = proxmark_studio::workbench::Workbench::default();
    w.geometry = Geometry::ClassicMini;
    let original = image(w.geometry).dump;
    w.dump = Some(original.clone());
    w.selection_pending = Some(vec![1, 2]);
    w.receive_selection(&[0; 32], true);
    assert_eq!(w.selected_result.len(), 2);
    assert_eq!(w.dump, Some(original.clone()));
    w.selection_pending = Some(vec![1, 2]);
    w.receive_selection(&[0; 16], false);
    assert!(w.selected_result.is_empty());
    assert_eq!(w.dump, Some(original));
}
#[test]
fn signal_parser_rejects_malformed_input_and_preserves_narrow_peaks() {
    let s = Signal::parse("0\n0\n127\n-127\n0\n0").unwrap();
    assert_eq!(s.envelope(0, 6, 1).unwrap(), vec![(-127, 127)]);
    for t in ["", "0", "0\n128", "0\nNaN", "0,1", "0\n1 2"] {
        assert!(Signal::parse(t).is_err());
    }
    assert!(s.envelope(2, 2, 10).is_err());
    assert!(s.envelope(0, 7, 10).is_err());
}
#[test]
fn emulator_parser_demands_every_block_once_and_exact_geometry() {
    let row = |n| format!("[=] | {n} | {} | data\n", vec!["00"; 16].join(" "));
    let raw = (0..20).map(row).collect::<String>();
    assert_eq!(
        emulator::parse_buffer(&raw, Geometry::ClassicMini)
            .unwrap()
            .len(),
        320
    );
    assert!(emulator::parse_buffer(&(raw.clone() + &row(19)), Geometry::ClassicMini).is_err());
    assert!(emulator::parse_buffer(&raw, Geometry::Classic1K).is_err());
    assert!(emulator::parse_buffer(&(raw + &row(20)), Geometry::ClassicMini).is_err());
}
#[test]
fn images_validate_access_bits_and_never_overwrite_same_name() {
    let mut images = vec![];
    let i = image(Geometry::ClassicMini);
    emulator::save_image(&mut images, i.clone()).unwrap();
    assert!(emulator::save_image(&mut images, i.clone()).is_err());
    let mut bad = i;
    bad.dump.bytes[3 * 16 + 6] ^= 1;
    bad.dump.sha256 = bad.dump.digest();
    assert!(bad.validate().is_err());
}
#[test]
fn mock_catalogue_and_help_are_read_without_device_or_writes() {
    let (_d, mut s, _caps) = fixture();
    s.port.clear();
    let r = run(&s, Job::CommandCatalogue);
    assert_eq!(r.outcome, Outcome::Completed);
    assert!(r
        .evidence
        .command_catalogue
        .iter()
        .any(|e| e.name == "hw ping"));
    let r = run(&s, Job::CommandHelp("hw ping".into()));
    assert_eq!(r.outcome, Outcome::Completed);
    let (name, help) = r.evidence.command_help.unwrap();
    assert_eq!(explorer::options(&name, &help).unwrap()[0].flag, "--len");
}
#[test]
fn mock_emulator_load_verify_and_separately_started_simulation() {
    let (d, s, caps) = fixture();
    let i = image(Geometry::ClassicMini);
    let r = run(&s, request(Action::Simulate(i.clone()), &caps));
    assert_eq!(r.outcome, Outcome::Mismatch);
    assert!(!d.path().join(".mock-pm3-rf-active").exists());
    let r = run(&s, request(Action::Load(i.clone()), &caps));
    assert_eq!(r.outcome, Outcome::Verified);
    assert_eq!(r.evidence.bytes, i.dump.bytes);
    assert!(!d.path().join(".mock-pm3-rf-active").exists());
    let r = run(&s, request(Action::Simulate(i), &caps));
    assert_eq!(r.outcome, Outcome::Completed);
    assert!(d.path().join(".mock-pm3-rf-active").exists());
    let r = run(&s, request(Action::Stop, &caps));
    assert_eq!(r.outcome, Outcome::Completed);
    assert!(!d.path().join(".mock-pm3-rf-active").exists());
}
#[test]
fn mock_emulator_missing_readback_and_ignored_writes_never_verify() {
    let (d, s, caps) = fixture();
    std::fs::write(d.path().join(".mock-pm3-ignore-writes"), b"").unwrap();
    let r = run(
        &s,
        request(Action::Load(image(Geometry::ClassicMini)), &caps),
    );
    assert_eq!(r.outcome, Outcome::Mismatch);
    std::fs::remove_file(d.path().join(".mock-pm3-ignore-writes")).unwrap();
    std::fs::write(d.path().join(".mock-pm3-emulator-short"), b"").unwrap();
    let r = run(
        &s,
        request(Action::Load(image(Geometry::ClassicMini)), &caps),
    );
    assert!(matches!(r.outcome, Outcome::Failed(_)));
}
#[test]
fn emulator_requires_all_capabilities_consent_and_matching_context_before_writing() {
    let (d, s, mut caps) = fixture();
    caps.help.remove("hf mf eview");
    let r = run(
        &s,
        request(Action::Load(image(Geometry::ClassicMini)), &caps),
    );
    assert!(matches!(r.outcome, Outcome::Failed(_)));
    assert!(!d.path().join(".mock-pm3-emulator.bin").exists());
    let caps = run(&s, Job::Probe).capabilities.unwrap();
    for (ack, sim) in [(false, true), (true, false)] {
        let r = run(
            &s,
            Job::Emulator(Request {
                action: Action::Load(image(Geometry::ClassicMini)),
                caps: caps.clone(),
                simulated: sim,
                acknowledged: ack,
            }),
        );
        assert!(matches!(r.outcome, Outcome::Failed(_)));
    }
    assert!(!d.path().join(".mock-pm3-emulator.bin").exists());
}
#[test]
#[ignore = "Requires explicitly supplied real RRG executable; offline help only"]
fn real_rrg_explorer_catalogue_and_emulator_help() {
    let cli = std::env::var("PROXMARK_RRG_TEST_CLIENT").unwrap();
    let d = tempfile::tempdir().unwrap();
    let s = Settings {
        cli,
        working_dir: d.path().display().to_string(),
        timeout_secs: 30,
        ..Default::default()
    };
    let r = run(&s, Job::CommandCatalogue);
    assert_eq!(r.outcome, Outcome::Completed);
    assert!(r.evidence.command_catalogue.len() > 500);
    for name in ["hf mf eview", "hf mf esetblk", "hf mf sim", "hw ping"] {
        let r = run(&s, Job::CommandHelp(name.into()));
        assert_eq!(r.outcome, Outcome::Completed);
        let (_, help) = r.evidence.command_help.unwrap();
        assert!(!explorer::options(name, &help).unwrap().is_empty());
    }
}
