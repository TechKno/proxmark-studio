use proxmark_studio::{
    capabilities::Capabilities,
    command::{self, DesfireParams, Kind},
    model::{Algorithm, Channel, Communication, Settings},
    parse, storage,
};

fn params() -> DesfireParams<'static> {
    DesfireParams {
        aid: "112233",
        fid: "01",
        key: "00112233445566778899AABBCCDDEEFF",
        key_number: 0,
        algorithm: Algorithm::Aes,
        channel: Channel::Ev1,
        communication: Communication::Plain,
        no_auth: false,
        backup_file: false,
    }
}
#[test]
fn no_auth_is_explicit_and_no_key_is_sent() {
    let p = DesfireParams {
        no_auth: true,
        ..params()
    };
    let s = p.build(Kind::Read, 16, 20, None).unwrap();
    assert_eq!(
        &*s.text,
        "hf mfdes read --aid 112233 --fid 01 --no-auth --type data -o 000010 -l 000014"
    );
    assert!(!s.sensitive);
}
#[test]
fn auth_is_scoped_to_application_and_channel() {
    let s = params().build(Kind::Authenticate, 0, 0, None).unwrap();
    assert!(s.sensitive);
    assert!(s.text.contains("--aid 112233"));
    assert!(s.text.contains("--schann ev1"));
    assert!(!s.text.contains("--save"));
}
#[test]
fn picc_list_has_no_application_flag() {
    let s = params().build(Kind::Applications, 0, 0, None).unwrap();
    assert!(!s.text.contains("--aid"));
}
#[test]
fn rejects_invalid_key_and_incompatible_channel() {
    assert!(DesfireParams {
        key: "00",
        ..params()
    }
    .build(Kind::Read, 0, 4, None)
    .is_err());
    assert!(DesfireParams {
        channel: Channel::D40,
        ..params()
    }
    .build(Kind::Authenticate, 0, 0, None)
    .is_err());
}
#[test]
fn rejects_invalid_identifiers() {
    for aid in ["11223G", "112233;quit", "11 22"] {
        assert!(DesfireParams { aid, ..params() }
            .build(Kind::Files, 0, 0, None)
            .is_err());
    }
    assert!(DesfireParams {
        fid: "20",
        ..params()
    }
    .build(Kind::Read, 0, 0, None)
    .is_err());
}
#[test]
fn enforces_terminal_utf8_byte_limit() {
    assert!(command::validate_line(&"a".repeat(254)).is_ok());
    assert!(command::validate_line(&"a".repeat(1000)).is_ok());
    assert!(command::validate_line(&"a".repeat(4097)).is_err());
    assert!(command::validate_line(&"é".repeat(2049)).is_err());
    assert!(command::validate_line("hf search\nquit").is_err());
}
#[test]
fn writes_commit_only_for_backup() {
    let data = [0xde, 0xad, 0xbe, 0xef];
    let a = params().build(Kind::Write, 0, 4, Some(&data)).unwrap();
    let b = DesfireParams {
        backup_file: true,
        ..params()
    }
    .build(Kind::Write, 0, 4, Some(&data))
    .unwrap();
    assert!(!a.text.contains("--commit"));
    assert!(b.text.ends_with("--commit"));
}
#[test]
fn no_ev2_channel_is_representable() {
    let encoded = serde_json::to_string(&Channel::Ev1).unwrap();
    assert_eq!(encoded, "\"Ev1\"");
    assert!(serde_json::from_str::<Channel>("\"Ev2\"").is_err());
}
#[test]
fn refuses_reserved_mfu_pages_and_wrong_sizes() {
    for p in [0, 1, 2, 3, 130, 255] {
        assert!(command::mfu_write(p, &[0; 4]).is_err());
    }
    assert!(command::mfu_write(4, &[0; 3]).is_err());
    assert!(command::mfu_write(4, &[0; 4]).is_ok());
}
#[test]
fn parses_source_derived_desfire_table() {
    let e = parse::evidence(include_str!("fixtures/desfire-read.txt"), &Kind::Read);
    assert_eq!(
        e.bytes,
        [(0..16).collect::<Vec<u8>>(), vec![0xde, 0xad, 0xbe, 0xef]].concat()
    );
    assert!(!e.reported_error);
}
#[test]
fn parses_source_derived_mfu_table() {
    let e = parse::evidence("[=] 04/0x04 | DE AD BE EF | ....\n", &Kind::Read);
    assert_eq!(e.bytes, [0xde, 0xad, 0xbe, 0xef]);
}
#[test]
fn parses_source_derived_em_id() {
    let e = parse::evidence("[+] EM 410x ID 0102030405\n", &Kind::Read);
    assert_eq!(e.bytes, [1, 2, 3, 4, 5]);
}
#[test]
fn parses_aids_and_file_ids_without_inventing_rows() {
    let e = parse::evidence(
        "[+] --- AID list ( 2 found )\n[+] 112233 comment\n[+] A0B0C0 comment\n",
        &Kind::Applications,
    );
    assert_eq!(e.aids, ["112233", "A0B0C0"]);
    let f = parse::evidence(
        "[=] File ID: 01\n[=] File ID: 1F\n[=] File ID: 0102\n",
        &Kind::Files,
    );
    assert_eq!(f.files, ["01", "1F"]);
}
#[test]
fn rejects_cli_failures_even_if_exit_status_would_be_zero() {
    let e = parse::evidence(
        "[!!] Authentication failed\n[+] Read 4 bytes\n[=] 0/0x00 | DE AD BE EF | ....",
        &Kind::Read,
    );
    assert!(e.reported_error);
}
#[test]
fn key_material_is_redacted() {
    for s in [
        "[+] Session key: ABCD",
        "[+] Key........ DEADBEEF",
        "[=] RndA = ABCDEF",
    ] {
        assert!(!parse::log_line(s).contains("ABCD"));
        assert!(!parse::log_line(s).contains("DEADBEEF"));
    }
}
#[test]
fn capabilities_reject_missing_syntax() {
    let mut c = Capabilities::default();
    assert!(c.validate("hf mfdes read --aid 112233").is_err());
    c.record("hf mfdes read", "usage: hf mfdes read --aid --fid");
    assert!(c.validate("hf mfdes read --aid 112233 --no-auth").is_err());
    assert!(c.validate("hf mfdes read --aid 112233 --fid 01").is_ok());
}
#[test]
fn capabilities_do_not_accept_unknown_command_output() {
    let mut c = Capabilities::default();
    c.record("hf mfdes auth", "unknown command hf mfdes auth usage: help");
    assert!(!c.has("hf mfdes auth"));
}
#[test]
fn settings_roundtrip_and_corruption_are_visible() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("settings.json");
    let s = Settings {
        cli: "a path with spaces/proxmark3".into(),
        ..Default::default()
    };
    storage::save(&p, &s).unwrap();
    assert_eq!(storage::load(&p).unwrap().cli, s.cli);
    std::fs::write(&p, "broken json").unwrap();
    assert!(storage::load(&p).is_err());
}
#[test]
fn profile_schema_cannot_serialize_keys() {
    let s = Settings::default();
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("key\""));
    assert!(s.console_history.is_empty());
}

#[test]
fn capability_flags_must_match_whole_options() {
    let mut c = Capabilities::default();
    c.record(
        "hf mfdes auth",
        "usage: hf mfdes auth --key --kdf --schann ev1",
    );
    assert!(c.require("hf mfdes auth", &["-k"]).is_err());
    assert!(c.require("hf mfdes auth", &["--key"]).is_ok());
}
