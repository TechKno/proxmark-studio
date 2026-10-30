use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub sealed_library: Option<crate::sealed::Envelope>,
    pub ui_preferences: crate::appearance::Preferences,
    pub command_favourites: Vec<String>,
    pub emulator_images: Vec<crate::emulator::Image>,
    pub rf_may_be_active: bool,
    pub ui_mode: crate::basic::Mode,
    pub organisation_register: String,
    pub cards: Vec<crate::basic::SavedCard>,
    pub operations: Vec<crate::basic::Record>,
    pub cli: String,
    pub port: String,
    pub working_dir: String,
    pub timeout_secs: u64,
    pub profiles: Vec<Profile>,
    pub custom_profiles: Vec<crate::custom_format::Profile>,
    pub vault_alias: String,
    pub vault_records: Vec<crate::vault::Record>,
    pub key_idle_secs: u64,
    pub console_history: Vec<String>,
    pub lf_profiles: Vec<crate::lf::Credential>,
    pub library_profiles: Vec<crate::format_catalogue::Credential>,
    pub credential_profiles: Vec<crate::credential::CredentialProfile>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            sealed_library: None,
            ui_preferences: Default::default(),
            command_favourites: vec![],
            emulator_images: vec![],
            rf_may_be_active: false,
            ui_mode: crate::basic::Mode::Basic,
            organisation_register: String::new(),
            cards: vec![],
            operations: vec![],
            cli: "proxmark3".into(),
            port: String::new(),
            working_dir: String::new(),
            timeout_secs: 45,
            profiles: vec![],
            credential_profiles: vec![],
            library_profiles: vec![],
            custom_profiles: vec![],
            vault_alias: String::new(),
            vault_records: vec![],
            key_idle_secs: 300,
            console_history: vec![],
            lf_profiles: vec![],
        }
    }
}

// Connection profiles deliberately contain no keys, raw CLI output or tag payloads.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub name: String,
    pub cli: String,
    pub port: String,
    pub working_dir: String,
    pub aid: String,
    pub fid: String,
    pub algorithm: Algorithm,
    pub channel: Channel,
    pub communication: Communication,
    pub key_number: u8,
    #[serde(default)]
    pub backup_file: bool,
    #[serde(default)]
    pub key_alias: String,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub enum Algorithm {
    Des,
    TwoTdea,
    ThreeTdea,
    #[default]
    Aes,
}
impl Algorithm {
    pub const ALL: [Self; 4] = [Self::Des, Self::TwoTdea, Self::ThreeTdea, Self::Aes];
    pub fn cli(self) -> &'static str {
        match self {
            Self::Des => "des",
            Self::TwoTdea => "2tdea",
            Self::ThreeTdea => "3tdea",
            Self::Aes => "aes",
        }
    }
    pub fn key_bytes(self) -> usize {
        match self {
            Self::Des => 8,
            Self::ThreeTdea => 24,
            _ => 16,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub enum Channel {
    D40,
    #[default]
    Ev1,
}
impl Channel {
    pub fn cli(self) -> &'static str {
        match self {
            Self::D40 => "d40",
            Self::Ev1 => "ev1",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub enum Communication {
    #[default]
    Plain,
    Mac,
    Encrypt,
}
impl Communication {
    pub fn cli(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Mac => "mac",
            Self::Encrypt => "encrypt",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Evidence {
    pub recovered: Option<crate::recovery::Keys>,
    pub magic_target: Option<crate::magic::Target>,
    pub memory_dump: Option<crate::backup::Dump>,
    pub signal: Option<crate::acquisition::Capture>,
    pub command_catalogue: Vec<crate::explorer::Entry>,
    pub command_help: Option<(String, String)>,
    pub card: Option<crate::basic::Card>,
    pub destination: Option<crate::basic::Destination>,
    pub format_catalogue: Vec<crate::format_catalogue::Descriptor>,
    pub encoded_wiegand: Option<String>,
    pub decoded_wiegand: Option<String>,
    pub desfire_file: Option<crate::desfire_archive::File>,
    pub verification: Option<String>,
    pub details: Vec<String>,
    pub uid: Option<String>,
    pub version_mismatch: bool,
    pub aids: Vec<String>,
    pub files: Vec<String>,
    pub bytes: Vec<u8>,
    pub hid_raw: Vec<String>,
    pub credential: Option<crate::credential::Decoded>,
    pub settings: Vec<String>,
    pub authenticated: bool,
    pub device_seen: bool,
    pub tag_seen: bool,
    pub reported_error: bool,
}

impl Settings {
    pub fn save_credential_profile(
        &mut self,
        mut profile: crate::credential::CredentialProfile,
    ) -> Result<(), String> {
        profile.name = profile.name.trim().to_owned();
        profile.validate()?;
        if let Some(old) = self
            .credential_profiles
            .iter_mut()
            .find(|p| p.name == profile.name)
        {
            *old = profile;
        } else {
            self.credential_profiles.push(profile);
        }
        Ok(())
    }
}
