use proxmark_studio::{
    command::{CommandSpec, Kind},
    model::Settings,
    runner::{Event, Job, JobResult, Outcome, Worker},
};
use std::time::{Duration, Instant};
fn settings() -> Settings {
    Settings {
        cli: env!("CARGO_BIN_EXE_mock-pm3").into(),
        port: "SIMULATED".into(),
        timeout_secs: 3,
        ..Default::default()
    }
}
fn wait(worker: &Worker) -> (JobResult, Vec<String>) {
    let mut logs = vec![];
    loop {
        match worker
            .rx
            .recv_timeout(Duration::from_secs(12))
            .expect("worker must finish")
        {
            Event::Log(s) => logs.push(s),
            Event::Finished(r) => return (*r, logs),
        }
    }
}
#[test]
fn probes_the_configured_cli() {
    let w = Worker::start(settings(), Job::Probe);
    let (r, _) = wait(&w);
    assert_eq!(r.outcome, Outcome::Completed);
    assert_eq!(
        r.capabilities.unwrap().help.len(),
        proxmark_studio::capabilities::COMMANDS.len()
    );
}
#[test]
fn exact_bytes_verify_and_prefix_does_not() {
    for (expected, outcome) in [
        (vec![0xde, 0xad, 0xbe, 0xef], Outcome::Verified),
        (vec![0xde, 0xad], Outcome::Mismatch),
        (vec![0xde, 0xad, 0xbe, 0xef, 0], Outcome::Mismatch),
    ] {
        let w = Worker::start(
            settings(),
            Job::Verify {
                read: CommandSpec::simple("hf mfu rdbl -b 4", Kind::Read),
                expected,
            },
        );
        let (r, _) = wait(&w);
        assert_eq!(r.outcome, outcome);
    }
}
#[test]
fn cannot_verify_without_a_recognised_read() {
    let w = Worker::start(
        settings(),
        Job::Verify {
            read: CommandSpec::simple("mock nothing", Kind::Read),
            expected: vec![0],
        },
    );
    let (r, _) = wait(&w);
    assert!(matches!(r.outcome, Outcome::Failed(_)));
}
#[test]
fn zero_exit_error_is_not_success() {
    let w = Worker::start(
        settings(),
        Job::Run(CommandSpec::simple("mock error", Kind::Console)),
    );
    let (r, _) = wait(&w);
    assert!(matches!(r.outcome, Outcome::Failed(_)));
}
#[test]
fn sensitive_output_never_reaches_logs() {
    let s = CommandSpec::new(
        "hf mfdes auth -k 00112233445566778899AABBCCDDEEFF".into(),
        "Authenticate",
        true,
        Kind::Authenticate,
    )
    .unwrap();
    let w = Worker::start(settings(), Job::Run(s));
    let (r, logs) = wait(&w);
    assert_eq!(r.outcome, Outcome::Completed);
    assert!(r.evidence.authenticated);
    assert!(!logs.join("\n").contains("0011223344556677"));
}
#[test]
fn cancel_and_timeout_are_bounded() {
    let start = Instant::now();
    let w = Worker::start(
        settings(),
        Job::Run(CommandSpec::simple("mock timeout", Kind::Console)),
    );
    std::thread::sleep(Duration::from_millis(100));
    w.cancel();
    let (r, _) = wait(&w);
    assert_eq!(r.outcome, Outcome::Cancelled);
    assert!(start.elapsed() < Duration::from_secs(3));
    let mut s = settings();
    s.timeout_secs = 2;
    let w = Worker::start(
        s,
        Job::Run(CommandSpec::simple("mock timeout", Kind::Console)),
    );
    assert_eq!(wait(&w).0.outcome, Outcome::TimedOut);
}
#[test]
fn failed_write_does_not_attempt_verification() {
    let w = Worker::start(
        settings(),
        Job::Programme {
            write: CommandSpec::simple("mock error", Kind::Write),
            read: CommandSpec::simple("hf mfu rdbl -b 4", Kind::Read),
            expected: vec![0xde, 0xad, 0xbe, 0xef],
        },
    );
    let (r, logs) = wait(&w);
    assert!(matches!(r.outcome, Outcome::Failed(_)));
    assert!(!logs.join("\n").contains("Reading the same target"));
}
#[test]
fn programming_requires_readback() {
    let w = Worker::start(
        settings(),
        Job::Programme {
            write: CommandSpec::simple("hf mfu wrbl -b 4 -d DEADBEEF", Kind::Write),
            read: CommandSpec::simple("hf mfu rdbl -b 4", Kind::Read),
            expected: vec![0xde, 0xad, 0xbe, 0xef],
        },
    );
    assert_eq!(wait(&w).0.outcome, Outcome::Verified);
}
#[test]
fn missing_executable_and_missing_port_are_errors() {
    let mut s = settings();
    s.cli = "/nonexistent/proxmark3".into();
    let w = Worker::start(s, Job::Run(CommandSpec::simple("hw version", Kind::Device)));
    assert!(matches!(wait(&w).0.outcome, Outcome::Failed(_)));
    let mut s = settings();
    s.port.clear();
    let w = Worker::start(s, Job::Run(CommandSpec::simple("hw version", Kind::Device)));
    assert!(matches!(wait(&w).0.outcome, Outcome::Failed(_)));
}
#[test]
fn output_limit_cannot_pass_as_success() {
    let w = Worker::start(
        settings(),
        Job::Run(CommandSpec::simple("mock large", Kind::Console)),
    );
    assert!(matches!(wait(&w).0.outcome, Outcome::Failed(_)));
}

fn hid_expected() -> proxmark_studio::credential::Credential {
    proxmark_studio::credential::Credential {
        format: proxmark_studio::credential::Format::H10301,
        facility_code: 118,
        card_number: 1603,
    }
}
#[test]
fn hid_read_and_exact_credential_verification() {
    let w = Worker::start(
        settings(),
        Job::ReadCredential {
            format: hid_expected().format,
        },
    );
    let r = wait(&w).0;
    assert_eq!(r.outcome, Outcome::Completed);
    assert_eq!(r.evidence.credential.unwrap().credential, hid_expected());
    let w = Worker::start(
        settings(),
        Job::VerifyCredential {
            expected: hid_expected(),
        },
    );
    assert_eq!(wait(&w).0.outcome, Outcome::Verified);
}
#[test]
fn hid_mismatch_is_not_verified() {
    let mut s = settings();
    s.port = "SIMULATED-MISMATCH".into();
    let w = Worker::start(
        s,
        Job::VerifyCredential {
            expected: hid_expected(),
        },
    );
    assert_eq!(wait(&w).0.outcome, Outcome::Mismatch);
}
#[test]
fn hid_missing_raw_multiple_raw_bad_parity_and_wrong_format_fail() {
    for port in [
        "SIMULATED-NORAW",
        "SIMULATED-MULTI",
        "SIMULATED-BADPARITY",
        "SIMULATED-WRONGFORMAT",
    ] {
        let mut s = settings();
        s.port = port.into();
        let w = Worker::start(
            s,
            Job::VerifyCredential {
                expected: hid_expected(),
            },
        );
        assert!(matches!(wait(&w).0.outcome, Outcome::Failed(_)), "{port}");
    }
}
#[test]
fn hid_alternative_format_diagnostics_do_not_override_selected_format() {
    let mut s = settings();
    s.port = "SIMULATED-ALTERNATE".into();
    let w = Worker::start(
        s,
        Job::VerifyCredential {
            expected: hid_expected(),
        },
    );
    assert_eq!(wait(&w).0.outcome, Outcome::Verified);
}
#[test]
fn hid_failed_write_never_verifies() {
    let mut s = settings();
    s.port = "SIMULATED-FAILWRITE".into();
    let w = Worker::start(
        s,
        Job::ProgrammeCredential {
            write: proxmark_studio::credential::write_command(
                &hid_expected(),
                proxmark_studio::credential::Target::T5577,
            )
            .unwrap(),
            expected: hid_expected(),
        },
    );
    let (r, logs) = wait(&w);
    assert!(matches!(r.outcome, Outcome::Failed(_)));
    assert!(!logs.iter().any(|l| l.contains("fresh HID read")));
}
#[test]
fn hid_programme_persists_simulated_state_then_reads_all_formats() {
    use proxmark_studio::credential::{self, Credential, Format, Target};
    for format in Format::ALL {
        for target in Target::ALL {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(".mock-pm3-enable-state"), "").unwrap();
            let mut s = settings();
            s.working_dir = dir.path().to_string_lossy().into();
            let expected = Credential {
                format,
                facility_code: format.facility_max(),
                card_number: format.card_max(),
            };
            let w = Worker::start(
                s.clone(),
                Job::ProgrammeCredential {
                    write: credential::write_command(&expected, target).unwrap(),
                    expected: expected.clone(),
                },
            );
            let (r, logs) = wait(&w);
            assert_eq!(r.outcome, Outcome::Verified);
            assert!(logs.iter().any(|l| l.contains("fresh HID read")));
            assert_eq!(r.evidence.credential.unwrap().credential, expected);
            let w = Worker::start(s, Job::ReadCredential { format });
            assert_eq!(wait(&w).0.evidence.credential.unwrap().credential, expected);
        }
    }
}

#[test]
fn probe_failure_is_bounded_and_records_its_launch_context() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".mock-pm3-probe-stall"), "").unwrap();
    let mut s = settings();
    s.working_dir = dir.path().to_string_lossy().into();
    s.timeout_secs = 2;
    let started = Instant::now();
    let w = Worker::start(s, Job::Probe);
    let (r, logs) = wait(&w);
    assert_eq!(r.outcome, Outcome::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(r.capabilities.is_none());
    assert!(logs.iter().any(|l| l.starts_with("CLI executable:")));
    assert!(logs.iter().any(|l| l.contains("stdin delivered=true")));
    assert!(!logs.iter().any(|l| l.contains("lf search -h")));
}
#[test]
fn self_executable_is_rejected_before_a_recursive_gui_launch() {
    let mut s = settings();
    s.cli = std::env::current_exe().unwrap().to_string_lossy().into();
    let started = Instant::now();
    let w = Worker::start(s, Job::Probe);
    let (r, logs) = wait(&w);
    assert!(matches!(r.outcome,Outcome::Failed(ref reason) if reason.contains("Studio itself")));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(!logs.iter().any(|l| l.contains("PID")));
}
#[test]
fn invalid_launch_settings_are_specific_failures_not_timeouts() {
    for field in ["empty", "directory", "working_folder"] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = settings();
        match field {
            "empty" => s.cli = "  ".into(),
            "directory" => s.cli = dir.path().to_string_lossy().into(),
            _ => s.working_dir = dir.path().join("missing").to_string_lossy().into(),
        };
        let w = Worker::start(s, Job::Probe);
        let (r, _) = wait(&w);
        assert!(matches!(r.outcome, Outcome::Failed(_)));
        assert!(r.capabilities.is_none());
    }
}
