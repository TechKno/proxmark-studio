use proxmark_studio::{
    command::Kind,
    maintenance::{self, Compatibility},
    parse,
};
const SYNTHETIC: &str = include_str!("fixtures/rrg-hardware-version.txt");
#[test]
fn synthetic_usb_version_output_confirms_device_and_matching_sources() {
    let e = parse::evidence(SYNTHETIC, &Kind::Device);
    assert!(e.device_seen);
    assert!(!e.version_mismatch);
    assert_eq!(
        maintenance::compatibility(SYNTHETIC),
        Compatibility::Matched("abcdef1".into())
    );
}
#[test]
fn same_revision_with_a_different_arm_source_is_a_mismatch() {
    let changed = SYNTHETIC.replace(
        "OS......... Iceman/master/abcdef1-suspect 2020-01-02 17:29:18 111222333",
        "OS......... Iceman/master/abcdef1-suspect 2020-01-02 17:29:18 123456789",
    );
    assert_ne!(changed, SYNTHETIC);
    assert!(matches!(
        maintenance::compatibility(&changed),
        Compatibility::Mismatch { .. }
    ));
    assert!(parse::evidence(&changed, &Kind::Device).version_mismatch);
}
#[test]
fn modern_missing_or_different_client_version_never_becomes_matched() {
    let different = SYNTHETIC.replace(
        "Iceman/master/abcdef1-suspect 2020-01-01",
        "Iceman/master/abcdef0-suspect 2020-01-01",
    );
    assert!(matches!(
        maintenance::compatibility(&different),
        Compatibility::Mismatch { .. }
    ));
    assert_eq!(
        maintenance::compatibility(
            "[ Client ]\nCompiler: gcc 20261027\nOS.... Iceman/master/abcdef1\n"
        ),
        Compatibility::Unknown
    );
}
#[test]
fn tagged_release_versions_compare_the_git_suffix() {
    assert!(matches!(maintenance::compatibility("Client: Iceman/master/v4.23346-116-gcea810820\nOS..... Iceman/master/v4.23346-116-gcea810820\n"),Compatibility::Matched(_)));
}
#[test]
fn help_without_a_hardware_handshake_does_not_confirm_a_device() {
    assert!(!parse::evidence("Communicating with PM3 over USB-CDC", &Kind::Help).device_seen);
    assert!(
        !parse::evidence(
            "[ Proxmark3 ]\n[ Client ]\nIceman/master/abcdef1\n",
            &Kind::Device
        )
        .device_seen
    );
}

#[test]
fn normal_hardware_status_error_verbosity_is_not_an_operation_failure() {
    let e = parse::evidence("[#] Debug log level..... 1 ( error )\n[#] Transfer Speed PM3 -> Client... 968448 bytes/s\n", &Kind::Console);
    assert!(!e.reported_error);
}
#[test]
fn real_error_after_status_verbosity_still_fails() {
    let e = parse::evidence(
        "[#] Debug log level..... 1 ( error )\n[!!] ERROR: USB read failed\n",
        &Kind::Console,
    );
    assert!(e.reported_error);
    assert!(
        parse::evidence("[!!] Debug log level..... 1 ( error )", &Kind::Console).reported_error
    );
}
