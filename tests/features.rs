use proxmark_studio::{
    backup::{self, Dump, Geometry},
    command::{self, DesfireParams, Kind},
    credential::{self, Credential, Format, Target},
    custom_format::Layout,
    desfire_admin::{Action, Admin},
    maintenance::{self, Compatibility},
    model::{Algorithm, Channel, Communication},
    terminal::{self, History},
};
fn params() -> DesfireParams<'static> {
    DesfireParams {
        aid: "112233",
        fid: "01",
        key: "00000000000000000000000000000000",
        key_number: 0,
        algorithm: Algorithm::Aes,
        channel: Channel::Ev1,
        communication: Communication::Encrypt,
        no_auth: false,
        backup_file: false,
    }
}
#[test]
fn custom_h10301_matches_independent_upstream_vectors() {
    let layout = Layout::default();
    for (fc, cn) in [(0, 0), (118, 1603), (255, 65535)] {
        let e = layout.encode(fc, cn).unwrap();
        assert_eq!(
            e,
            Credential {
                format: Format::H10301,
                facility_code: fc,
                card_number: cn
            }
            .encode()
            .unwrap()
        );
        assert_eq!(layout.decode(&e.binary).unwrap(), (fc, cn, true));
    }
}
#[test]
fn custom_layout_rejects_overlap_bad_ranges_and_truncation() {
    let mut l = Layout {
        card_start: 2,
        ..Default::default()
    };
    assert!(l.validate().is_err());
    l = Layout::default();
    l.even_start = 0;
    assert!(l.validate().is_err());
    l = Layout::default();
    assert!(l.encode(256, 0).is_err());
    l.bits = usize::MAX;
    assert!(l.validate().is_err());
}
#[test]
fn custom_every_bit_error_is_detected() {
    let l = Layout::default();
    let e = l.encode(118, 1603).unwrap();
    for i in 0..26 {
        let mut b = e.binary.as_bytes().to_vec();
        b[i] = if b[i] == b'0' { b'1' } else { b'0' };
        assert!(!l.decode(std::str::from_utf8(&b).unwrap()).unwrap().2);
    }
}
#[test]
fn auto_candidates_preserve_37bit_ambiguity() {
    let c = Credential {
        format: Format::H10304,
        facility_code: 123,
        card_number: 456,
    };
    let e = c.encode().unwrap();
    let all = credential::candidates(&e.hid_raw, false);
    assert!(all.iter().any(|d| d.credential.format == Format::H10304));
    assert!(all.iter().any(|d| d.credential.format == Format::H10302));
}
#[test]
fn geometry_excludes_every_protected_region() {
    for g in Geometry::ALL {
        assert!(!g.writable(0));
        assert!(!g.writable(g.units()));
        if g.classic() {
            for b in 0..g.units() {
                let trailer = if b < 128 { b % 4 == 3 } else { b % 16 == 15 };
                if trailer {
                    assert!(!g.writable(b));
                }
            }
        } else {
            for p in 0..4 {
                assert!(!g.writable(p));
            }
            assert!(g.writable(g.data_end()));
            assert!(!g.writable(g.data_end() + 1));
        }
    }
}
#[test]
fn lock_checks_fail_closed() {
    assert!(backup::check_locks(Geometry::Ntag213, &[0; 4], Some(&[0; 4])).is_ok());
    assert!(backup::check_locks(Geometry::Ntag213, &[0, 0, 1, 0], Some(&[0; 4])).is_err());
    assert!(backup::check_locks(Geometry::Ntag213, &[0; 4], None).is_err());
    assert!(backup::check_locks(Geometry::Ntag215, &[0; 4], Some(&[1, 0, 0, 0])).is_err());
    assert!(backup::check_locks(Geometry::Ultralight, &[0; 4], None).is_ok());
}
#[test]
fn dump_detects_tampering_of_memory_geometry_and_identity() {
    let d = Dump::new(Geometry::Ntag213, "04112233445566", vec![0; 180]).unwrap();
    for which in 0..3 {
        let mut bad = d.clone();
        match which {
            0 => bad.bytes[5] = 1,
            1 => bad.uid = "04112233445567".into(),
            _ => bad.geometry = Geometry::Ntag215,
        };
        assert!(bad.validate().is_err());
    }
    assert!(Dump::new(Geometry::Ntag213, "04112233445566", vec![0; 179]).is_err());
}
#[test]
fn dump_roundtrip_and_diff_report_byte_offsets() {
    let a = Dump::new(Geometry::Classic1K, "11223344", vec![0; 1024]).unwrap();
    let mut bytes = a.bytes.clone();
    bytes[100] = 1;
    bytes[1023] = 2;
    let b = Dump::new(a.geometry, &a.uid, bytes).unwrap();
    assert_eq!(a.differences(&b).unwrap(), [100, 1023]);
    let c: Dump = serde_json::from_slice(&serde_json::to_vec(&b).unwrap()).unwrap();
    c.validate().unwrap();
}
#[test]
fn uid_parser_ignores_table_payloads_and_refuses_bad_lengths() {
    assert_eq!(
        backup::uid("[+] UID: 04 11 22 33 44 55 66\n[+] TYPE: NTAG 213"),
        Some("04112233445566".into())
    );
    assert!(backup::uid("[+] UID: 11 22 33").is_none());
    assert!(backup::uid("0 | 04 11 22 33 |").is_none());
}
#[test]
fn classic_commands_refuse_security_blocks_and_keys_in_labels() {
    assert!(Geometry::Classic4K
        .write(143, &[0; 16], "FFFFFFFFFFFF", false)
        .is_err());
    assert!(Geometry::Classic1K
        .write(0, &[0; 16], "FFFFFFFFFFFF", false)
        .is_err());
    let w = Geometry::Classic4K
        .write(144, &[0; 16], "FFFFFFFFFFFF", true)
        .unwrap();
    assert!(w.sensitive);
    assert!(!w.label.contains("FFFFFF"));
    assert!(w.text.contains(" -b "));
}
#[test]
fn desfire_admin_has_no_ev2_lrp_or_ev3_actions() {
    for action in Action::ALL {
        let a = Admin {
            action,
            newkey: zeroize::Zeroizing::new("11".repeat(16)),
            oldkey: zeroize::Zeroizing::new("00".repeat(16)),
            data: "DEADBEEF".into(),
            ..Default::default()
        };
        let s = a.build(&params()).unwrap();
        assert!(!s.text.contains("ev2"));
        assert!(!s.text.contains("lrp"));
        assert!(!s.text.contains("ev3"));
        assert!(s.text.contains("--schann ev1"));
        assert!(s.sensitive);
        assert!(!s.label.contains("000000"));
    }
}
#[test]
fn desfire_admin_checks_limits_and_explicit_kdf() {
    let mut a = Admin {
        action: Action::CreateValue,
        lower: 5,
        value: 3,
        ..Default::default()
    };
    assert!(a.build(&params()).is_err());
    a = Admin {
        action: Action::CreateApp,
        numkeys: 15,
        ..Default::default()
    };
    assert!(a.build(&params()).is_err());
    a = Admin {
        kdf: "AN10922".into(),
        ..Default::default()
    };
    assert!(a.build(&params()).is_err());
    a.kdf_input = "01".repeat(31);
    assert!(a.build(&params()).unwrap().text.contains("--kdf AN10922"));
    a.kdf_input = "01".repeat(32);
    assert!(a.build(&params()).is_err());
}
#[test]
fn long_sensitive_commands_are_supported_without_secret_labels() {
    let a = Admin {
        action: Action::ChangeKey,
        newkey: zeroize::Zeroizing::new("11".repeat(24)),
        oldkey: zeroize::Zeroizing::new("22".repeat(24)),
        algorithm: Algorithm::ThreeTdea,
        ..Default::default()
    };
    let mut p = params();
    p.algorithm = Algorithm::ThreeTdea;
    p.key = "000000000000000000000000000000000000000000000000";
    let s = a.build(&p).unwrap();
    assert!(s.text.len() > 254);
    assert!(s.sensitive);
    assert!(!s.label.contains("111111"));
}
#[test]
fn terminal_input_and_history_do_not_persist_secrets() {
    assert!(terminal::validate_input(&"a".repeat(1000)).is_ok());
    assert!(terminal::validate_input("one\ntwo").is_err());
    assert!(terminal::validate_input(&"é".repeat(501)).is_err());
    let mut h = History::default();
    h.add("hw version");
    h.add("hf mfdes auth -k 1234");
    h.add("hf search");
    assert_eq!(h.lines, vec!["hw version", "hf search"]);
    assert_eq!(h.previous(), Some("hf search"));
    assert_eq!(h.previous(), Some("hw version"));
}
#[test]
fn lf_bounds_prevent_silent_cli_truncation() {
    use proxmark_studio::lf::{Credential as Lf, Family};
    for family in [
        Family::Awid26,
        Family::Awid34,
        Family::Awid37,
        Family::Awid50,
        Family::IoProx,
    ] {
        let mut c = Lf {
            family,
            ..Default::default()
        };
        let (fc, cn) = c.limits();
        c.facility = fc;
        c.number = cn;
        assert!(c.write().is_ok());
        c.facility = fc + 1;
        assert!(c.write().is_err());
    }
}
#[test]
fn lf_readback_requires_family_fields_and_crc() {
    use proxmark_studio::lf::{Credential as Lf, Family};
    let c = Lf {
        family: Family::IoProx,
        facility: 118,
        number: 1603,
        version: 1,
        ..Default::default()
    };
    assert!(c.matches("[+] IO Prox - XSF(01)76:01603, Raw: 0011223344556677 ( ok )"));
    assert!(!c.matches("[+] IO Prox - XSF(01)76:01603, Raw: 0011223344556677 ( fail )"));
    assert!(!c.matches("[+] IO Prox - XSF(02)76:01603, Raw: 0011223344556677 ( ok )"));
}
#[test]
fn capability_version_match_mismatch_and_unknown_are_distinct() {
    assert!(matches!(
        maintenance::compatibility(
            "Client.... Iceman/master/abcdef0\nOS........ Iceman/master/abcdef0\n"
        ),
        Compatibility::Matched(_)
    ));
    assert!(matches!(
        maintenance::compatibility("Client: Iceman/master/abcdef0\nOS: Iceman/master/1234567\n"),
        Compatibility::Mismatch { .. }
    ));
    assert_eq!(
        maintenance::compatibility("device connected"),
        Compatibility::Unknown
    );
}
fn elf() -> Vec<u8> {
    let mut b = vec![0; 120];
    b[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    b[16..18].copy_from_slice(&2_u16.to_le_bytes());
    b[18..20].copy_from_slice(&40_u16.to_le_bytes());
    b[28..32].copy_from_slice(&52_u32.to_le_bytes());
    b[42..44].copy_from_slice(&32_u16.to_le_bytes());
    b[44..46].copy_from_slice(&1_u16.to_le_bytes());
    for (o, v) in [(52, 1_u32), (56, 100), (64, 0x102000), (68, 20), (72, 20)] {
        b[o..o + 4].copy_from_slice(&v.to_le_bytes())
    }
    b
}
#[test]
fn firmware_parser_rejects_wrong_architecture_and_truncation() {
    let b = elf();
    assert!(!maintenance::inspect_image(&b).unwrap().bootloader);
    for n in 0..b.len() {
        if n < 120 {
            assert!(maintenance::inspect_image(&b[..n]).is_err());
        }
    }
    let mut bad = b.clone();
    bad[18] = 62;
    assert!(maintenance::inspect_image(&bad).is_err());
    let mut bad = b;
    bad[64..68].copy_from_slice(&0x100000_u32.to_le_bytes());
    assert!(maintenance::inspect_image(&bad).unwrap().bootloader);
}
#[test]
fn firmware_hash_binds_review_to_exact_bytes() {
    let a = elf();
    let mut b = a.clone();
    b[119] = 1;
    assert_ne!(
        maintenance::inspect_image(&a).unwrap().sha256,
        maintenance::inspect_image(&b).unwrap().sha256
    );
}
#[test]
fn vault_rejects_aliases_and_invalid_keys_without_os_access() {
    assert!(proxmark_studio::vault::validate_alias("").is_err());
    assert!(proxmark_studio::vault::validate_alias("key\nname").is_err());
    assert!(proxmark_studio::vault::store("test", "not a key", 16).is_err());
}
#[test]
fn raw_custom_adapter_uses_binary_and_target_flag() {
    let (s, raw) = Layout::default().write(118, 1603, Target::Q5).unwrap();
    assert!(s.text.starts_with("lf hid clone --bin "));
    assert!(s.text.ends_with(" --q5"));
    assert_eq!(raw, "000000000000002006EC0C86");
}
#[test]
fn desfire_larger_payload_has_explicit_bound() {
    let s = params()
        .build(Kind::Write, 0, 128, Some(&[0; 128]))
        .unwrap();
    assert!(s.text.len() > 254);
    assert!(command::payload(&"AB".repeat(1024)).is_ok());
    assert!(command::payload(&"AB".repeat(1025)).is_err());
}
#[test]
fn external_rrg_json_import_refuses_partial_dumps() {
    let mut blocks = serde_json::Map::new();
    for n in 0..45 {
        blocks.insert(n.to_string(), "00000000".into());
    }
    let mut v =
        serde_json::json!({"FileType":"mfu","Card":{"UID":"04112233445566"},"blocks":blocks});
    let d = backup::import_json(&serde_json::to_vec(&v).unwrap(), Geometry::Ntag213).unwrap();
    assert_eq!(d.bytes.len(), 180);
    v["blocks"].as_object_mut().unwrap().remove("44");
    assert!(backup::import_json(&serde_json::to_vec(&v).unwrap(), Geometry::Ntag213).is_err());
}
#[test]
fn persisted_history_only_contains_command_names_or_help() {
    assert!(terminal::persistable("hf search"));
    assert!(terminal::persistable("hf mfdes auth -h"));
    assert!(!terminal::persistable("hf mfdes auth -k 0000000000000000"));
    assert!(!terminal::persistable("1234567890ABCDEF"));
}
#[test]
fn unsupported_secure_operations_are_rejected_in_both_console_transports() {
    for c in [
        "hf mfdes auth --schann ev2",
        "hf mfdes read --schann=lrp",
        "hf mfdes createmfcmapping -h",
        "hf mfdes createmacfile",
    ] {
        assert!(command::CommandSpec::console(c.into()).is_err());
        assert!(terminal::validate_input(c).is_err());
    }
    assert!(command::CommandSpec::console("hf mfdes getversion".into()).is_ok());
}
#[test]
fn terminal_rendering_applies_cursor_and_erase_sequences() {
    let mut p = vt100::Parser::new(10, 80, 0);
    p.process(b"old text\r\x1b[2Knew prompt: ");
    assert_eq!(p.screen().contents(), "new prompt: ");
}
#[test]
fn desfire_value_and_rights_results_exclude_key_material() {
    use proxmark_studio::parse;
    let e=parse::evidence("[+] Value: -5 (0xfffffffb)\n[+] [1...] AMK Configuration changeable : YES\n[+] Key cnt.... 3\n[+] Session key: 00112233445566778899AABBCCDDEEFF\n",&Kind::Read);
    assert_eq!(e.details.len(), 3);
    assert!(e.details[0].contains("-5"));
    assert!(!format!("{:?}", e.details).contains("001122"));
}
