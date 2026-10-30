//! Native OS credential store. Profiles contain aliases only, never secrets.
use zeroize::Zeroizing;
const SERVICE: &str = "org.proxmarkstudio.desktop.desfire";
pub fn validate_alias(alias: &str) -> Result<(), String> {
    if alias.trim().is_empty() || alias.len() > 80 || alias.chars().any(char::is_control) {
        return Err("Key alias must be 1–80 bytes without control characters".into());
    }
    Ok(())
}
fn entry(alias: &str) -> Result<keyring::Entry, String> {
    validate_alias(alias)?;
    keyring::Entry::new(SERVICE, alias).map_err(|e| e.to_string())
}
pub fn store(alias: &str, key: &str, bytes: usize) -> Result<(), String> {
    let key = Zeroizing::new(crate::command::hex(key, bytes, "Key")?);
    entry(alias)?.set_password(&key).map_err(|e| e.to_string())
}
pub fn load(alias: &str, bytes: usize) -> Result<Zeroizing<String>, String> {
    let key = Zeroizing::new(entry(alias)?.get_password().map_err(|e| e.to_string())?);
    Ok(Zeroizing::new(crate::command::hex(
        &key,
        bytes,
        "Stored key",
    )?))
}
pub fn delete(alias: &str) -> Result<(), String> {
    entry(alias)?.delete_credential().map_err(|e| e.to_string())
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub alias: String,
    pub bytes: usize,
    pub version: u8,
    pub note: String,
}
impl Record {
    pub fn validate(&self) -> Result<(), String> {
        validate_alias(&self.alias)?;
        if ![6, 8, 16, 24].contains(&self.bytes)
            || self.note.len() > 240
            || self.note.chars().any(char::is_control)
        {
            return Err("Invalid vault record metadata".into());
        }
        Ok(())
    }
}
pub fn upsert(records: &mut Vec<Record>, record: Record) -> Result<(), String> {
    record.validate()?;
    if let Some(old) = records.iter_mut().find(|r| r.alias == record.alias) {
        *old = record
    } else {
        records.push(record)
    }
    Ok(())
}
pub fn expired(idle: std::time::Duration, timeout_secs: u64) -> bool {
    timeout_secs != 0 && idle >= std::time::Duration::from_secs(timeout_secs.min(3600))
}
