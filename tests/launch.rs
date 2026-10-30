use proxmark_studio::{
    credential::{Credential, CredentialProfile, Target},
    launch,
    model::Settings,
    simulation,
};
use std::ffi::OsString;
fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
#[test]
fn accepts_an_explicit_config_with_spaces() {
    assert_eq!(
        launch::config_argument(args(&["--config", "/a path/settings.json"]))
            .unwrap()
            .unwrap(),
        std::path::PathBuf::from("/a path/settings.json")
    );
}
#[test]
fn default_config_and_bad_launch_arguments() {
    assert!(launch::config_argument(Vec::<OsString>::new())
        .unwrap()
        .is_none());
    for values in [
        vec!["--config"],
        vec!["--config", ""],
        vec!["--config", "--unknown"],
        vec!["--config", "a", "--config", "b"],
        vec!["--incognito"],
    ] {
        assert!(launch::config_argument(args(&values)).is_err());
    }
}
#[test]
fn simulation_uses_an_isolated_stateful_folder() {
    let real = Settings {
        cli: "real-proxmark3".into(),
        port: "COM9".into(),
        working_dir: "real-folder".into(),
        ..Default::default()
    };
    let (mock, session) =
        simulation::configure(&real, std::path::Path::new(env!("CARGO_BIN_EXE_mock-pm3"))).unwrap();
    assert_eq!(mock.port, "SIMULATED");
    assert_eq!(mock.timeout_secs, 5);
    assert!(session
        .directory
        .path()
        .join(".mock-pm3-enable-state")
        .exists());
    let path = session.directory.path().to_path_buf();
    drop(session);
    assert!(!path.exists());
}
#[test]
fn saving_credential_profiles_does_not_persist_simulation_connection() {
    let real = Settings {
        cli: "real-proxmark3".into(),
        port: "COM9".into(),
        working_dir: "real-folder".into(),
        ..Default::default()
    };
    let (mut mock, session) =
        simulation::configure(&real, std::path::Path::new(env!("CARGO_BIN_EXE_mock-pm3"))).unwrap();
    mock.save_credential_profile(CredentialProfile {
        name: "Example".into(),
        credential: Credential::default(),
        target: Target::T5577,
    })
    .unwrap();
    let saved = session.settings_to_save(&mock);
    assert_eq!(saved.cli, real.cli);
    assert_eq!(saved.port, real.port);
    assert_eq!(saved.working_dir, real.working_dir);
    assert_eq!(saved.credential_profiles.len(), 1);
}
#[test]
fn missing_simulation_executable_is_rejected() {
    assert!(simulation::configure(
        &Settings::default(),
        std::path::Path::new("/missing-mock-pm3")
    )
    .is_err());
}
