use proxmark_studio::{
    command::Kind,
    credential::{self, Credential, CredentialProfile, Format, Target},
    model::Settings,
    parse, storage,
};
use serde::Deserialize;
#[derive(Deserialize)]
struct Vector {
    format: Format,
    facility_code: u32,
    card_number: u64,
    hid_raw: String,
}
fn vectors() -> Vec<Vector> {
    serde_json::from_str(include_str!("fixtures/credential-vectors.json")).unwrap()
}
#[test]
fn encodes_against_independent_upstream_c_vectors() {
    for v in vectors() {
        let c = Credential {
            format: v.format,
            facility_code: v.facility_code,
            card_number: v.card_number,
        };
        assert_eq!(c.encode().unwrap().hid_raw, v.hid_raw, "{:?}", c);
    }
}
#[test]
fn decodes_independent_upstream_c_vectors() {
    for v in vectors() {
        let d = credential::decode_hid(v.format, &v.hid_raw).unwrap();
        assert_eq!(d.credential.facility_code, v.facility_code);
        assert_eq!(d.credential.card_number, v.card_number);
        assert!(d.parity_valid);
        assert_eq!(d.encoded.hid_raw, v.hid_raw);
    }
}
#[test]
fn source_example_decodes_to_118_1603() {
    let d = credential::decode_hid(Format::H10301, "2006ec0c86").unwrap();
    assert_eq!(
        (d.credential.facility_code, d.credential.card_number),
        (118, 1603)
    );
    assert!(d.parity_valid);
    assert_eq!(d.encoded.binary, "10111011000000110010000110");
}
#[test]
fn every_single_bit_error_is_detected() {
    for f in Format::ALL {
        let c = Credential {
            format: f,
            facility_code: f.facility_max().min(118),
            card_number: 1603,
        };
        let data = c.encode().unwrap();
        for i in 0..f.bits() {
            let mut bits = data.binary.as_bytes().to_vec();
            bits[i] = if bits[i] == b'0' { b'1' } else { b'0' };
            let d = credential::decode_binary(f, &String::from_utf8(bits).unwrap()).unwrap();
            assert!(!d.parity_valid, "{} bit {i}", f.name());
        }
    }
}
#[test]
fn rejects_format_and_transport_mismatch() {
    let data = Credential {
        format: Format::H10301,
        facility_code: 118,
        card_number: 1603,
    }
    .encode()
    .unwrap();
    assert!(credential::decode_hid(Format::H10306, &data.hid_raw).is_err());
    assert!(credential::decode_hid(Format::H10304, &data.hid_raw).is_err());
    assert!(credential::decode_hid(Format::H10301, "9E00000000002006EC0C86").is_err());
}
#[test]
fn rejects_malformed_binary_and_raw() {
    for raw in ["", "GG", "2006EC0C86;quit", "FFFFFFFFFFFFFFFFFFFFFFFFF"] {
        assert!(credential::decode_hid(Format::H10301, raw).is_err());
    }
    for binary in [
        "",
        "10101",
        "0000000000000000000000000X",
        "111111111111111111111111111",
    ] {
        assert!(credential::decode_binary(Format::H10301, binary).is_err());
    }
}
#[test]
fn normalises_raw_prefix_case_and_leading_zeroes() {
    assert_eq!(
        credential::normalise_raw(" 0x 20 06 ec 0c 86 ").unwrap(),
        "000000000000002006EC0C86"
    );
}
#[test]
fn validates_format_specific_decimal_ranges() {
    for f in Format::ALL {
        assert!(Credential::from_decimal(f, "0", "0").is_ok());
        assert!(Credential::from_decimal(
            f,
            &f.facility_max().to_string(),
            &f.card_max().to_string()
        )
        .is_ok());
        assert!(Credential::from_decimal(f, &(f.facility_max() + 1).to_string(), "0").is_err());
        assert!(Credential::from_decimal(f, "0", &(f.card_max() + 1).to_string()).is_err());
    }
    for s in ["-1", "+1", "1.5", "0xff", "1;quit", "18446744073709551616"] {
        assert!(Credential::from_decimal(Format::H10301, s, "1").is_err());
        assert!(Credential::from_decimal(Format::H10301, "1", s).is_err());
    }
}
#[test]
fn write_commands_use_selected_format_fields_and_target() {
    let c = Credential {
        format: Format::H10304,
        facility_code: 65535,
        card_number: 524287,
    };
    for (t, suffix) in [
        (Target::T5577, ""),
        (Target::Q5, " --q5"),
        (Target::Em4305, " --em"),
    ] {
        let s = credential::write_command(&c, t).unwrap();
        assert_eq!(
            s.text.as_str(),
            format!("lf hid clone -w H10304 --fc 65535 --cn 524287{suffix}")
        );
    }
    assert_eq!(credential::read_command().kind, Kind::CredentialRead);
}
#[test]
fn raw_parser_requires_fresh_exact_raw_line() {
    let e=parse::evidence("[+] [H10301 ] HID H10301 26-bit FC: 118 CN: 1603 parity ( ok )\n[=] raw: 000000000000002006ec0c86\n[=] raw: 000000000000002006ec0c86\n",&Kind::CredentialRead);
    assert_eq!(e.hid_raw, ["000000000000002006EC0C86"]);
    assert!(
        parse::evidence("[+] FC: 118 CN: 1603 parity ( ok )", &Kind::CredentialRead)
            .hid_raw
            .is_empty()
    );
    assert!(parse::evidence("[=] raw: corrupt", &Kind::CredentialRead).reported_error);
}
#[test]
fn alternate_format_parity_diagnostic_does_not_hide_good_raw() {
    let e = parse::evidence(
        "[-] Parity tests failed\n[=] raw: 000000000000002006EC0C86",
        &Kind::CredentialRead,
    );
    assert!(!e.reported_error);
    assert_eq!(e.hid_raw.len(), 1);
    assert!(parse::evidence("[-] Parity tests failed", &Kind::Read).reported_error);
}
#[test]
fn recognises_rrg_clone_failure_even_with_zero_exit() {
    assert!(parse::evidence("[-] Cloning ( fail )", &Kind::Write).reported_error);
}
#[test]
fn credential_profiles_roundtrip_and_replace_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let mut s = Settings::default();
    let p = CredentialProfile {
        name: " Lab credential ".into(),
        credential: Credential {
            format: Format::H10306,
            facility_code: 65535,
            card_number: 12345,
        },
        target: Target::Q5,
    };
    s.save_credential_profile(p).unwrap();
    storage::save(&path, &s).unwrap();
    let mut loaded = storage::load(&path).unwrap();
    assert_eq!(loaded.credential_profiles[0].name, "Lab credential");
    assert_eq!(loaded.credential_profiles[0].target, Target::Q5);
    let mut replacement = loaded.credential_profiles[0].clone();
    replacement.credential.card_number = 54321;
    loaded.save_credential_profile(replacement).unwrap();
    assert_eq!(loaded.credential_profiles.len(), 1);
    assert_eq!(loaded.credential_profiles[0].credential.card_number, 54321);
}
#[test]
fn invalid_profile_cannot_mutate_saved_profiles() {
    let mut s = Settings::default();
    let p = CredentialProfile {
        name: "Bad".into(),
        credential: Credential {
            format: Format::H10301,
            facility_code: 256,
            card_number: 1,
        },
        target: Target::T5577,
    };
    assert!(s.save_credential_profile(p).is_err());
    assert!(s.credential_profiles.is_empty());
}
#[test]
fn previous_version_settings_load_without_credential_profiles() {
    let s: Settings = serde_json::from_str(r#"{"cli":"proxmark3","profiles":[]}"#).unwrap();
    assert!(s.credential_profiles.is_empty());
}
