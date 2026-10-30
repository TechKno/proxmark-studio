//! Each child gets its own process group; cancellation includes script descendants.
pub fn isolate(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000200);
    }
}
pub fn terminate(pid: u32) {
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(pid) {
            if pid > 1 {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("WINDIR") {
            let _ = std::process::Command::new(
                std::path::Path::new(&root).join("System32/taskkill.exe"),
            )
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x08000000)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        }
    }
}
#[cfg(windows)]
use std::os::windows::process::CommandExt;
