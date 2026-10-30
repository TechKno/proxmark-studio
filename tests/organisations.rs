use proxmark_studio::{
    basic::{Card, Content},
    em410x::ReaderFormat,
    netaccess::{self, Profile, Repository, Store},
};
fn profile() -> Profile {
    Profile {
        organisation: "Example Organisation".into(),
        site: "North".into(),
        facility: 42,
        first_number: 100,
        last_number: 102,
        confirmed: true,
        ..Default::default()
    }
}
fn store() -> Store {
    Store {
        profile: profile(),
        ..Default::default()
    }
}

#[test]
fn profile_requires_names_confirmation_supported_format_and_valid_ranges() {
    assert!(Profile::default().validate().is_err());
    let mut p = profile();
    p.confirmed = false;
    assert!(p.validate().is_err());
    p.confirmed = true;
    p.facility = 256;
    assert!(p.validate().is_err());
    p.format = ReaderFormat::Wiegand34;
    assert!(p.validate().is_ok());
    p.facility = 65536;
    assert!(p.validate().is_err());
    p.facility = 42;
    p.format = ReaderFormat::Customer8Id32;
    assert!(p.validate().is_err());
    p = profile();
    p.first_number = 0;
    assert!(p.validate().is_err());
    p.first_number = 103;
    assert!(p.validate().is_err());
}
#[test]
fn custom_facilities_encode_and_decode_independent_wiegand_vectors() {
    let p = profile();
    let card = netaccess::new_card(&p, 100, false).unwrap();
    assert_eq!(card.em_identity().unwrap().0, "00002A0064");
    assert_eq!(netaccess::identity(&p, &card, false).unwrap().0, 100);
    let p = Profile {
        format: ReaderFormat::Wiegand34,
        facility: 0x1234,
        ..profile()
    };
    let card = netaccess::new_card(&p, 101, false).unwrap();
    assert_eq!(card.em_identity().unwrap().0, "0012340065");
    assert!(netaccess::identity(&profile(), &card, false).is_err());
    let high = Card {
        content: Content::Em410x("AB12340065".into()),
        captured_at: 0,
        simulated: false,
    };
    assert_eq!(
        netaccess::identity(&p, &high, false).unwrap(),
        (101, "AB12340065".into())
    );
    assert!(netaccess::new_card(&p, 103, false).is_err());
}
#[test]
fn bounded_allocation_retains_deleted_and_observed_numbers() {
    let mut s = store();
    let user = s.add_user("Example", None).unwrap();
    assert_eq!(s.reserve(user).unwrap(), 100);
    s.delete_fob(100, user).unwrap();
    let card = netaccess::new_card(&s.profile, 102, false).unwrap();
    s.observe_scan(&card).unwrap();
    assert_eq!(s.reserve(user).unwrap(), 101);
    assert!(s.reserve(user).is_err());
}
#[test]
fn profiles_cannot_reinterpret_existing_credentials_or_observations() {
    let mut s = store();
    let mut p = s.profile.clone();
    p.facility = 43;
    s.update_profile(p).unwrap();
    let user = s.add_user("Example", Some(1)).unwrap();
    s.reserve(user).unwrap();
    let mut p = s.profile.clone();
    p.facility = 44;
    assert!(s.update_profile(p).is_err());
    let mut p = s.profile.clone();
    p.format = ReaderFormat::Wiegand34;
    assert!(s.update_profile(p).is_err());
    let mut p = s.profile.clone();
    p.organisation = "Renamed organisation".into();
    p.first_number = 200;
    p.last_number = 300;
    s.update_profile(p).unwrap();
    assert_eq!(s.reserve(user).unwrap(), 200);
    s.validate().unwrap();
}
#[test]
fn create_never_overwrites_and_registers_are_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    let mut first = Repository::create(&a, store()).unwrap();
    first
        .update(|s| {
            let u = s.add_user("First", None)?;
            s.reserve(u)
        })
        .unwrap();
    let second = Repository::create(&b, store()).unwrap();
    assert!(second.data.users.is_empty());
    assert!(Repository::create(&a, store()).is_err());
    drop(first);
    let before = std::fs::read(&a).unwrap();
    assert!(Repository::create(&a, store()).is_err());
    assert_eq!(std::fs::read(&a).unwrap(), before);
    assert!(Repository::open(&a, true).is_err());
}
#[test]
fn csv_import_is_atomic_and_preserves_existing_records() {
    let mut s = store();
    assert_eq!(
        s.import_csv(include_str!("../examples/synthetic-users.csv"))
            .unwrap(),
        1
    );
    assert_eq!(s.users.len(), 2);
    assert_eq!(s.card(100).unwrap().observed_ids, vec!["00002A0064"]);
    let before = serde_json::to_string(&s).unwrap();
    let input =
        "User Number,User Name,Card Number,Full EM410x ID,Notes\n3,New,101,,\n4,Duplicate,100,,\n";
    assert!(s.import_csv(input).is_err());
    assert_eq!(serde_json::to_string(&s).unwrap(), before);
    assert!(s
        .import_csv(
            "User Number,User Name,Card Number,Full EM410x ID,Notes\n1,Wrong person,102,,\n"
        )
        .is_err());
    assert!(s
        .import_csv(
            "User Number,User Name,Card Number,Full EM410x ID,Notes\n3,New,101,00002B0065,\n"
        )
        .is_err());
    assert_eq!(
        s.export_csv().lines().nth(1).unwrap().split(',').nth(2),
        Some("42")
    );
}
#[test]
fn legacy_schema_two_keeps_unset_controller_numbers_and_history() {
    let json = r#"{"schema":2,"site":"Legacy Site","facility":42,"encoding":"EM410x/Wiegand26","users":[{"number":335,"name":"Example Holder","na1_number":null}],"cards":[{"number":1001,"user_no":335,"state":"Imported","observed_ids":["12AB2A03E9"],"needs_enrolment":true}],"changes":[{"at":1,"detail":"Previous transfer"}]}"#;
    let mut s: Store = serde_json::from_str(json).unwrap();
    assert!(s.upgrade().unwrap());
    assert_eq!(s.profile.organisation, "Legacy Site");
    assert_eq!(s.profile.facility, 42);
    assert_eq!(s.user(335).unwrap().controller_number, None);
    assert_eq!(s.changes[0].detail, "Previous transfer");
    assert_eq!(s.card(1001).unwrap().observed_ids, vec!["12AB2A03E9"]);
    let saved = serde_json::to_string(&s).unwrap();
    assert!(!saved.contains("na1_number"));
    assert!(!saved.contains("encoding"));
    assert!(!s.upgrade().unwrap());
}
