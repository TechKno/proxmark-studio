#![allow(clippy::field_reassign_with_default)]
use proxmark_studio::{
    acquisition::{self, Demod},
    backup::{Dump, Geometry},
    capabilities::Capabilities,
    emulator::Image,
    magic::{self, Action, Generation},
    model::Settings,
    recovery::{self, Method},
    runner::{Event, Job, JobResult, Outcome, Worker},
    sealed,
};
use std::time::Duration;
use zeroize::Zeroizing;
fn fixture(card: &str) -> (tempfile::TempDir, Settings, Capabilities) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".mock-pm3-enable-state"), b"").unwrap();
    std::fs::write(dir.path().join(".mock-pm3-card"), card).unwrap();
    let settings = Settings {
        cli: env!("CARGO_BIN_EXE_mock-pm3").into(),
        port: "SIMULATED".into(),
        working_dir: dir.path().display().to_string(),
        timeout_secs: 5,
        ..Default::default()
    };
    let result = run(&settings, Job::Probe).0;
    assert_eq!(result.outcome, Outcome::Completed);
    (dir, settings, result.capabilities.unwrap())
}
fn run(s: &Settings, job: Job) -> (JobResult, Vec<String>) {
    let worker = Worker::start(s.clone(), job);
    let mut logs = vec![];
    loop {
        match worker.rx.recv_timeout(Duration::from_secs(90)).unwrap() {
            Event::Log(l) => logs.push(l),
            Event::Finished(r) => return (*r, logs),
        }
    }
}
fn recover(caps: &Capabilities, method: Method) -> Job {
    Job::Recover {
        caps: caps.clone(),
        request: recovery::Request {
            uid: "01020304".into(),
            geometry: Geometry::Classic1K,
            method,
            seed: Zeroizing::new(String::new()),
            sector: 0,
            key_b: false,
            dictionary: vec![],
            simulated: true,
            acknowledged: true,
        },
    }
}
fn image() -> Image {
    Image {
        name: "Synthetic source".into(),
        dump: Dump::new(
            Geometry::Classic1K,
            "01020304",
            proxmark_studio::mock_specialist::card_bytes(false),
        )
        .unwrap(),
        simulated: true,
    }
}
fn inspect(s: &Settings, caps: &Capabilities, g: Generation) -> magic::Target {
    let r = run(
        s,
        Job::Magic {
            caps: caps.clone(),
            request: magic::Request {
                action: Action::Inspect {
                    generation: g,
                    key: Zeroizing::new("FFFFFFFFFFFF".into()),
                },
                simulated: true,
            },
        },
    )
    .0;
    assert_eq!(r.outcome, Outcome::Completed);
    r.evidence.magic_target.unwrap()
}
fn clone_job(caps: &Capabilities, target: magic::Target, source: Image) -> Job {
    Job::Magic {
        caps: caps.clone(),
        request: magic::Request {
            action: Action::Clone {
                source,
                target,
                key: Zeroizing::new("FFFFFFFFFFFF".into()),
                acknowledged: true,
            },
            simulated: true,
        },
    }
}
#[test]
fn recovery_parser_merges_progress_but_rejects_conflicts_and_invalid_rows() {
    let first = "[+] 000 | 003 | FFFFFFFFFFFF | D | ------------ | 0";
    let last = "[+] 000 | 003 | FFFFFFFFFFFF | D | 112233445566 | N";
    let keys = recovery::parse_keys(
        &format!("{first}\n{last}"),
        "01020304".into(),
        Geometry::Classic1K,
        true,
    )
    .unwrap();
    assert_eq!(keys.counts(), (2, 0));
    assert!(!format!("{keys:?}").contains("FFFFFFFFFFFF"));
    assert!(recovery::parse_keys(
        &format!("{last}\n{}", last.replace("FFFFFFFFFFFF", "000000000000")),
        "01020304".into(),
        Geometry::Classic1K,
        true
    )
    .is_err());
    for row in [
        "[+] 040 | 163 | FFFFFFFFFFFF | 1 | ------------ | 0",
        "[+] 000 | 002 | FFFFFFFFFFFF | 1 | ------------ | 0",
        "[+] 000 | 003 | FFFFFFFFFFFF | ? | ------------ | 0",
    ] {
        assert!(recovery::parse_keys(row, "01020304".into(), Geometry::Classic1K, true).is_err());
    }
}
#[test]
fn dictionary_is_bounded_strict_and_normalised() {
    assert_eq!(
        recovery::dictionary(b"# comment\nffffffffffff\n112233445566").unwrap(),
        b"FFFFFFFFFFFF\n112233445566\n"
    );
    assert!(recovery::dictionary(b"FFFFFFFFFFFF;hw reset").is_err());
    assert!(recovery::dictionary(b"").is_err());
    assert!(recovery::dictionary(&vec![0; 1024 * 1024 + 1]).is_err());
}
#[test]
fn automatic_recovery_confirms_keys_and_logs_no_secrets() {
    let (_d, s, c) = fixture("classic");
    let (r, logs) = run(&s, recover(&c, Method::Automatic));
    assert_eq!(r.outcome, Outcome::Completed);
    let keys = r.evidence.recovered.unwrap();
    assert!(keys.complete());
    assert_eq!(keys.counts(), (32, 32));
    assert!(keys.map().unwrap().contains("0,A,FFFFFFFFFFFF"));
    let all = logs.join("\n");
    assert!(!all.contains("FFFFFFFFFFFF"));
    assert!(!all.contains("112233445566"));
    assert!(!std::path::Path::new(&s.working_dir)
        .join("hf-mf-01020304-key.bin")
        .exists());
}
#[test]
fn partial_and_wrong_recovery_cannot_claim_complete_keys() {
    let (d, s, c) = fixture("classic");
    std::fs::write(d.path().join(".mock-pm3-recovery-partial"), b"").unwrap();
    let k = run(&s, recover(&c, Method::Dictionary))
        .0
        .evidence
        .recovered
        .unwrap();
    assert!(!k.complete());
    assert_eq!(k.counts(), (16, 16));
    std::fs::remove_file(d.path().join(".mock-pm3-recovery-partial")).unwrap();
    std::fs::write(d.path().join(".mock-pm3-recovery-wrong"), b"").unwrap();
    let k = run(&s, recover(&c, Method::Automatic))
        .0
        .evidence
        .recovered
        .unwrap();
    assert!(!k.complete());
    assert_eq!(k.counts(), (32, 0));
}
#[test]
fn recovered_source_capture_inserts_only_confirmed_keys() {
    let (_d, s, c) = fixture("classic");
    let keys = run(&s, recover(&c, Method::Automatic))
        .0
        .evidence
        .recovered
        .unwrap();
    let r = run(
        &s,
        Job::CaptureClassic {
            keys,
            caps: c,
            simulated: true,
        },
    )
    .0;
    assert_eq!(r.outcome, Outcome::Completed);
    assert_eq!(r.evidence.memory_dump.unwrap(), image().dump);
}
#[test]
fn generation_detection_requires_unambiguous_positive_evidence() {
    assert_eq!(
        magic::generation("[+] Magic capabilities... Gen 1a").unwrap(),
        Generation::Gen1a
    );
    assert_eq!(
        magic::generation("[+] Magic capabilities... Gen 2 / CUID").unwrap(),
        Generation::Gen2
    );
    for raw in [
        "possibly Gen 1a",
        "[+] Magic capabilities... Gen 3 / APDU ( possibly )",
        "[+] Magic capabilities... Gen 1a\n[+] Magic capabilities... Gen 4 GDM / USCUID",
    ] {
        assert!(magic::generation(raw).is_err());
    }
}
#[test]
fn source_uid_bcc_geometry_and_trailer_policy_are_checked() {
    let mut i = image();
    magic::validate_source(&i).unwrap();
    i.dump.bytes[4] ^= 1;
    i.dump.sha256 = i.dump.digest();
    assert!(magic::validate_source(&i).is_err());
    let mut i = image();
    i.dump.uid = "11223344".into();
    i.dump.sha256 = i.dump.digest();
    assert!(magic::validate_source(&i).is_err());
    let mut i = image();
    i.dump.bytes[54..57]
        .copy_from_slice(&proxmark_studio::classic::encode_access([0, 0, 0, 7]).unwrap());
    i.dump.sha256 = i.dump.digest();
    assert!(magic::validate_source(&i).is_err());
}
#[test]
fn gen1a_clone_reads_every_block_and_preserves_full_source() {
    let (d, s, c) = fixture("magic1");
    let target = inspect(&s, &c, Generation::Gen1a);
    let r = run(&s, clone_job(&c, target, image())).0;
    assert_eq!(r.outcome, Outcome::Verified);
    assert_eq!(
        std::fs::read(d.path().join(".mock-pm3-classic.bin")).unwrap(),
        image().dump.bytes
    );
}
#[test]
fn gen2_clone_verifies_readable_data_and_new_keys() {
    let (d, s, c) = fixture("magic2");
    let target = inspect(&s, &c, Generation::Gen2);
    let r = run(&s, clone_job(&c, target, image())).0;
    assert_eq!(r.outcome, Outcome::Verified);
    assert_eq!(
        std::fs::read(d.path().join(".mock-pm3-classic.bin")).unwrap(),
        image().dump.bytes
    );
}
#[test]
fn ignored_magic_write_never_verifies() {
    let (d, s, c) = fixture("magic1");
    let target = inspect(&s, &c, Generation::Gen1a);
    std::fs::write(d.path().join(".mock-pm3-ignore-writes"), b"").unwrap();
    assert_eq!(
        run(&s, clone_job(&c, target, image())).0.outcome,
        Outcome::Mismatch
    );
}
#[test]
fn changed_magic_destination_and_wrong_context_stop_before_writes() {
    let (d, s, c) = fixture("magic1");
    let mut target = inspect(&s, &c, Generation::Gen1a);
    target.block0[9] ^= 1;
    assert!(matches!(
        run(&s, clone_job(&c, target, image())).0.outcome,
        Outcome::Failed(_)
    ));
    assert!(!d.path().join(".mock-pm3-classic.bin").exists());
    let target = inspect(&s, &c, Generation::Gen1a);
    let mut i = image();
    i.simulated = false;
    assert!(matches!(
        run(&s, clone_job(&c, target, i)).0.outcome,
        Outcome::Failed(_)
    ));
    assert!(!d.path().join(".mock-pm3-classic.bin").exists());
}
#[test]
fn live_capture_and_demod_share_only_the_captured_samples() {
    let (_d, s, c) = fixture("classic");
    let r = run(
        &s,
        Job::Signal(acquisition::Request {
            samples: 2048,
            demod: Demod::AskManchester,
            caps: c,
            simulated: true,
            saved: None,
        }),
    )
    .0;
    assert_eq!(r.outcome, Outcome::Completed);
    let capture = r.evidence.signal.unwrap();
    assert_eq!(capture.signal.samples.len(), 2048);
    assert_eq!(capture.bits, proxmark_studio::mock_specialist::frame());
    assert!(capture.decoding[0].contains("0102030405"));
    assert!(capture.simulated && capture.live);
}
#[test]
fn missing_capture_file_and_failed_demod_are_distinct() {
    let (d, s, c) = fixture("classic");
    std::fs::write(d.path().join(".mock-pm3-signal-missing"), b"").unwrap();
    let request = || {
        Job::Signal(acquisition::Request {
            samples: 2048,
            demod: Demod::AskManchester,
            caps: c.clone(),
            simulated: true,
            saved: None,
        })
    };
    assert!(matches!(run(&s, request()).0.outcome, Outcome::Failed(_)));
    std::fs::remove_file(d.path().join(".mock-pm3-signal-missing")).unwrap();
    std::fs::write(d.path().join(".mock-pm3-demod-fail"), b"").unwrap();
    let r = run(&s, request()).0;
    assert_eq!(r.outcome, Outcome::Completed);
    let data = r.evidence.signal.unwrap();
    assert!(data.bits.is_empty() && data.analysis_error.is_some());
    assert_eq!(data.signal.samples.len(), 2048);
}
#[test]
fn em410x_frame_decoder_requires_both_parities_and_stop_bit() {
    let good = proxmark_studio::mock_specialist::frame();
    assert_eq!(acquisition::em410x_frames(&good), vec!["0102030405"]);
    for offset in [10, 14, 59, 63] {
        let mut v = good.clone().into_bytes();
        v[offset] = if v[offset] == b'0' { b'1' } else { b'0' };
        assert!(acquisition::em410x_frames(std::str::from_utf8(&v).unwrap()).is_empty());
    }
    assert!(acquisition::em410x_frames(&"1".repeat(400)).is_empty());
}
#[test]
fn capture_rejects_oversized_and_injected_paths() {
    let c = Capabilities::default();
    for count in [0, 511, 60001, u32::MAX] {
        let r = acquisition::Request {
            samples: count,
            demod: Demod::None,
            caps: c.clone(),
            simulated: false,
            saved: None,
        };
        assert!(acquisition::capture_spec(&r, std::path::Path::new("x;hw reset")).is_err());
    }
    assert!(acquisition::bits("[+] 01010101\n[+] not bits").is_ok());
    assert!(acquisition::bits("usage: rawdemod").is_err());
}
#[test]
fn encrypted_files_roundtrip_tamper_fail_and_use_random_salts() {
    let pass = "synthetic-test-passphrase";
    let a = sealed::seal(b"test card data", pass).unwrap();
    let b = sealed::seal(b"test card data", pass).unwrap();
    assert_ne!(a.salt, b.salt);
    assert_ne!(a.nonce, b.nonce);
    assert_eq!(&*sealed::open(&a, pass).unwrap(), b"test card data");
    assert!(sealed::open(&a, "incorrect-test-passphrase").is_err());
    let mut tampered = a.clone();
    tampered.nonce[0] ^= 1;
    assert!(sealed::open(&tampered, pass).is_err());
    tampered = a.clone();
    tampered.schema = 2;
    assert!(sealed::open(&tampered, pass).is_err());
    assert!(sealed::seal(b"data", "short").is_err());
}
#[test]
fn encrypted_images_preserve_demo_marker_and_validate_private_export() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("source.pm3enc");
    let original = image();
    sealed::save(
        &path,
        &serde_json::to_vec(&original).unwrap(),
        "synthetic-test-passphrase",
    )
    .unwrap();
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("Synthetic source"));
    let decoded: Image =
        serde_json::from_slice(&sealed::load(&path, "synthetic-test-passphrase").unwrap()).unwrap();
    assert_eq!(decoded, original);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o077,
            0
        );
    }
}
#[test]
fn old_settings_default_to_system_appearance_and_no_encryption() {
    let settings: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(
        settings.ui_preferences.appearance,
        proxmark_studio::appearance::Appearance::System
    );
    assert!(settings.ui_preferences.show_sidebar);
    assert!(settings.sealed_library.is_none());
}

#[test]
fn recovery_requires_acknowledgement_capabilities_and_matching_uid() {
    let (_d, s, caps) = fixture("classic");
    for case in 0..3 {
        let mut job = recover(&caps, Method::Automatic);
        if let Job::Recover { request, caps } = &mut job {
            match case {
                0 => request.acknowledged = false,
                1 => {
                    caps.help.remove("hf mf autopwn");
                }
                _ => request.uid = "AABBCCDD".into(),
            }
        }
        let r = run(&s, job).0;
        assert!(matches!(r.outcome, Outcome::Failed(_)));
        assert!(r.evidence.recovered.is_none());
    }
}
#[test]
#[ignore = "Requires explicitly supplied real RRG executable; offline help only"]
fn real_rrg_specialist_help_matches_required_flags() {
    let d = tempfile::tempdir().unwrap();
    let s = Settings {
        cli: std::env::var("PROXMARK_RRG_TEST_CLIENT").unwrap(),
        working_dir: d.path().display().to_string(),
        timeout_secs: 30,
        ..Default::default()
    };
    for (command, flags) in [
        ("hf mf autopwn", vec!["--ns", "--1k"]),
        ("hf mf fchk", vec!["--1k"]),
        ("hf mf cgetblk", vec!["--blk"]),
        ("hf mf csetblk", vec!["--blk", "-d"]),
        ("lf read", vec!["-s"]),
        ("data save", vec!["-f"]),
        ("data load", vec!["-f"]),
        (
            "data rawdemod",
            vec!["--am", "--ab", "--fs", "--nr", "--p1", "--p2"],
        ),
    ] {
        let r = run(&s, Job::CommandHelp(command.into())).0;
        assert_eq!(r.outcome, Outcome::Completed);
        let (_, help) = r.evidence.command_help.unwrap();
        for flag in flags {
            assert!(help.contains(flag), "{command} lacks {flag}");
        }
    }
}
