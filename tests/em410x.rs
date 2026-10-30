use proxmark_studio::{
    basic::{self, Card, Content, Destination, SavedCard},
    credential::Target,
    em410x::{self, ReaderFormat as F},
    runner::Job,
};
#[test]
fn documented_supplier_vector_has_distinct_26_and_34_bit_interpretations() {
    let a = em410x::decode("1B02D4318F", F::Wiegand26).unwrap();
    assert_eq!((a.facility, a.number), (212, 12687));
    assert_eq!(a.decimal24, 13906319);
    assert_eq!(a.decimal32, 47460751);
    let b = em410x::decode(&a.id, F::Wiegand34).unwrap();
    assert_eq!((b.facility, b.number), (724, 12687));
    assert_eq!(
        em410x::decode(&a.id, F::Customer8Id32).unwrap().facility,
        27
    );
}
#[test]
fn documented_customer_byte_vector_does_not_confuse_customer_with_wiegand_facility() {
    let d = em410x::decode("4B00423C15", F::Customer8Id32).unwrap();
    assert_eq!(
        (d.facility, d.number, d.decimal40),
        (75, 4340757, 322126887957)
    );
    assert_eq!(em410x::decode(&d.id, F::Wiegand26).unwrap().facility, 66);
}
#[test]
fn known_facility_filters_without_overwriting_data_or_selecting_a_format() {
    let decoded = em410x::matching_formats("AB12FA4321", 250).unwrap();
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].format, F::Wiegand26);
    assert_eq!(decoded[0].number, 17185);
    assert_eq!(decoded[0].id, "AB12FA4321");
    assert!(em410x::matching_formats("AB12FA4321", 251)
        .unwrap()
        .is_empty());
    // The same facility may match multiple layouts; never infer a unique width.
    assert_eq!(
        em410x::matching_formats("AB00FA4321", 250).unwrap().len(),
        2
    );
}
#[test]
fn all_zero_and_maximum_bits_use_unsigned_fields_without_truncation() {
    for f in F::ALL {
        let d = em410x::decode("0000000000", f).unwrap();
        assert_eq!((d.facility, d.number), (0, 0));
    }
    for (f, facility, number) in [
        (F::Wiegand26, 255, 65535),
        (F::Wiegand34, 65535, 65535),
        (F::Customer8Id32, 255, 4294967295),
    ] {
        let d = em410x::decode("FFFFFFFFFF", f).unwrap();
        assert_eq!((d.facility, d.number), (facility, number));
        assert_eq!(d.decimal40, 1099511627775);
    }
}
#[test]
fn malformed_and_extended_identifiers_and_invalid_hints_are_rejected() {
    for id in [
        "",
        "12345678",
        "123456789012",
        "GG12345678",
        "0102030405;quit",
    ] {
        assert!(em410x::decode(id, F::Wiegand26).is_err());
    }
    assert_eq!(em410x::known_facility(" 250 ").unwrap(), Some(250));
    assert_eq!(em410x::known_facility("").unwrap(), None);
    for hint in ["-1", "0xFA", "250.0", "65536", "999999999999999999"] {
        assert!(em410x::known_facility(hint).is_err());
    }
}
fn source() -> Card {
    Card {
        content: Content::Em410x("AB12FA4321".into()),
        captured_at: 1,
        simulated: true,
    }
}
#[test]
fn old_library_records_gain_a_labelled_preview_without_changing_schema() {
    let c = source();
    let encoded = serde_json::to_string(&c).unwrap();
    assert!(encoded.contains("\"Em410x\":\"AB12FA4321\""));
    let decoded: Card = serde_json::from_str(&encoded).unwrap();
    assert!(decoded.summary().contains("Facility 250 · card 17185"));
    assert!(decoded.summary().contains("unconfirmed"));
    assert_eq!(decoded, c);
}
#[test]
fn selected_interpretation_persists_and_clone_still_uses_all_original_id_bits() {
    let mut c = source();
    c.select_em_format(Some(F::Wiegand26)).unwrap();
    assert!(c.summary().contains("user-selected"));
    let saved = SavedCard {
        name: "Synthetic decoder fixture".into(),
        card: c.clone(),
    };
    let round: SavedCard = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
    assert_eq!(round, saved);
    basic::validate_library(&[round]).unwrap();
    let destination = Destination::T55 {
        target: Target::T5577,
        block0: "000880E0".into(),
    };
    let Job::Programme {
        write, expected, ..
    } = basic::write_job(&c, &destination).unwrap()
    else {
        panic!("must preserve EM410x protocol")
    };
    assert_eq!(write.text.as_str(), "lf em 410x clone --id AB12FA4321");
    assert_eq!(expected, vec![0xab, 0x12, 0xfa, 0x43, 0x21]);
    c.select_em_format(None).unwrap();
    assert_eq!(c, source());
}
#[test]
fn failed_read_does_not_replace_the_selected_snapshot() {
    let mut c = source();
    c.select_em_format(Some(F::Wiegand34)).unwrap();
    let mut flow = basic::Workflow::default();
    flow.set_source(c.clone());
    flow.pending = Some(basic::Pending::Read);
    flow.receive(
        &proxmark_studio::runner::Outcome::TimedOut,
        &Default::default(),
    );
    assert_eq!(flow.source, Some(c));
}
