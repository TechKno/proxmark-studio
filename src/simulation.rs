use crate::model::Settings;
use std::path::{Path, PathBuf};
pub struct Session {
    pub directory: tempfile::TempDir,
    pub original: Settings,
}
impl Session {
    pub fn settings_to_save(&self, current: &Settings) -> Settings {
        let mut saved = self.original.clone();
        saved.sealed_library = current.sealed_library.clone();
        saved.ui_mode = current.ui_mode;
        saved.ui_preferences = current.ui_preferences.clone();
        saved.command_favourites = current.command_favourites.clone();
        saved.emulator_images = current.emulator_images.clone();
        saved.cards = current.cards.clone();
        saved.operations = current.operations.clone();
        saved.profiles = current.profiles.clone();
        saved.credential_profiles = current.credential_profiles.clone();
        saved.library_profiles = current.library_profiles.clone();
        saved.custom_profiles = current.custom_profiles.clone();
        saved.lf_profiles = current.lf_profiles.clone();
        saved.vault_alias = current.vault_alias.clone();
        saved.vault_records = current.vault_records.clone();
        saved.key_idle_secs = current.key_idle_secs;
        saved.console_history = current.console_history.clone();
        saved
    }
}
pub fn bundled_mock() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| {
            p.parent().map(|p| {
                p.join(if cfg!(windows) {
                    "mock-pm3.exe"
                } else {
                    "mock-pm3"
                })
            })
        })
        .filter(|p| p.is_file())
}
/// The caller owns this directory for the whole session. No real settings are saved.
pub fn configure(current: &Settings, mock: &Path) -> Result<(Settings, Session), String> {
    if !mock.is_file() {
        return Err("The bundled mock client is missing".into());
    }
    let directory = tempfile::Builder::new()
        .prefix("proxmark-studio-simulation-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    std::fs::write(directory.path().join(".mock-pm3-enable-state"), b"")
        .map_err(|e| e.to_string())?;
    std::fs::write(directory.path().join(".mock-pm3-desfire"), b"").map_err(|e| e.to_string())?;
    let mut settings = current.clone();
    settings.cli = std::fs::canonicalize(mock)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into();
    settings.port = "SIMULATED".into();
    settings.rf_may_be_active = false;
    settings.working_dir = directory.path().to_string_lossy().into();
    settings.timeout_secs = 5;
    Ok((
        settings,
        Session {
            directory,
            original: current.clone(),
        },
    ))
}
