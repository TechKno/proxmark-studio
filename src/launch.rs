use std::{ffi::OsString, path::PathBuf};
/// Parse only Studio arguments, never forward them to the Proxmark client.
pub fn config_argument(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Option<PathBuf>, String> {
    let mut args = args.into_iter();
    let mut config = None;
    while let Some(arg) = args.next() {
        if arg != "--config" {
            return Err(format!(
                "Unknown Studio argument: {}. Usage: proxmark-studio [--config FILE]",
                arg.to_string_lossy()
            ));
        }
        if config.is_some() {
            return Err("Specify --config only once".into());
        }
        let value = args.next().ok_or("--config needs a settings file path")?;
        if value.is_empty() || value.to_string_lossy().starts_with("--") {
            return Err("--config needs a settings file path".into());
        }
        config = Some(PathBuf::from(value));
    }
    Ok(config)
}
