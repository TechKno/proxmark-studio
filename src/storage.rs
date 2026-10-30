use crate::model::Settings;
use directories::ProjectDirs;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub fn config_path() -> Result<PathBuf, String> {
    ProjectDirs::from("org", "ProxmarkStudio", "Proxmark Studio")
        .map(|p| p.config_dir().join("settings.json"))
        .ok_or("Cannot locate your configuration folder".into())
}
pub fn load(path: &Path) -> Result<Settings, String> {
    match fs::read(path) {
        Ok(v) => serde_json::from_slice(&v).map_err(|e| format!("Settings could not be read: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(format!("Settings could not be opened: {e}")),
    }
}
pub fn save(path: &Path, value: &Settings) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid settings path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    // Tempfile uses private files and atomically replaces the destination on Windows/macOS.
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > limit {
        return Err("File exceeds the supported size".into());
    }
    let mut bytes = vec![];
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("File grew beyond the supported size".into());
    }
    Ok(bytes)
}
