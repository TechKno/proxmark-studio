//! A PTY keeps CLI state and interactive prompts alive. No OS shell is involved.
use crate::{model::Settings, runner::validate_cli_settings};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, PtySize};
use std::{
    io::{Read, Write},
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
};
use zeroize::{Zeroize, Zeroizing};
pub const MAX_INPUT: usize = 1000;
pub fn validate_input(s: &str) -> Result<(), String> {
    crate::command::validate_line(s)?;
    if s.is_empty() || s.len() > MAX_INPUT || s.chars().any(char::is_control) {
        return Err("Enter a single line of 1–1000 bytes without control characters".into());
    }
    Ok(())
}
pub enum Input {
    Line(Zeroizing<String>),
    Interrupt,
    Escape,
    Stop,
}
enum SessionInput {
    User(Input),
    #[cfg(windows)]
    CursorPosition,
}

// portable-pty requests the parent cursor position when starting ConPTY.
// Studio owns a fresh terminal, whose initial cursor is at row 1, column 1.
// Match across read boundaries, and answer only this startup handshake.
#[cfg(any(windows, test))]
#[derive(Default)]
struct StartupCursorQuery {
    matched: usize,
    answered: bool,
}
#[cfg(any(windows, test))]
impl StartupCursorQuery {
    fn observe(&mut self, bytes: &[u8]) -> bool {
        if self.answered {
            return false;
        }
        const QUERY: &[u8] = b"\x1b[6n";
        for &byte in bytes {
            if byte == QUERY[self.matched] {
                self.matched += 1;
                if self.matched == QUERY.len() {
                    self.answered = true;
                    return true;
                }
            } else {
                self.matched = usize::from(byte == QUERY[0]);
            }
        }
        false
    }
}

pub struct Session {
    pid: Option<u32>,
    pub output: Receiver<String>,
    pub exit: Receiver<Result<u32, String>>,
    tx: SyncSender<SessionInput>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}
impl Session {
    pub fn start(settings: &Settings, offline: bool) -> Result<Self, String> {
        validate_cli_settings(settings)?;
        if !offline && settings.port.is_empty() {
            return Err("Select a serial port".into());
        }
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 36,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(&settings.cli);
        cmd.arg("--incognito");
        if !offline {
            cmd.args(["-p", &settings.port]);
        }
        if !settings.working_dir.is_empty() {
            cmd.cwd(&settings.working_dir);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let pid = child.process_id();
        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let (otx, output) = mpsc::sync_channel(128);
        let (tx, rx) = mpsc::sync_channel(32);
        #[cfg(windows)]
        let query_tx = tx.clone();
        thread::spawn(move || {
            let mut buf = [0_u8; 4096];
            #[cfg(windows)]
            let mut cursor_query = StartupCursorQuery::default();
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                #[cfg(windows)]
                if cursor_query.observe(&buf[..n])
                    && query_tx.try_send(SessionInput::CursorPosition).is_err()
                {
                    break;
                }
                if otx
                    .send(String::from_utf8_lossy(&buf[..n]).into_owned())
                    .is_err()
                {
                    break;
                }
            }
            buf.zeroize();
        });
        let (exit_tx, exit) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let _master = pair.master;
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let _ = exit_tx.send(Ok(status.exit_code()));
                        return;
                    }
                    Err(e) => {
                        let _ = exit_tx.send(Err(e.to_string()));
                        break;
                    }
                    _ => {}
                }
                let input = match rx.recv_timeout(std::time::Duration::from_millis(25)) {
                    Ok(v) => v,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let res = match input {
                    SessionInput::User(Input::Line(mut s)) => {
                        // ConPTY interprets Enter as CR; LF alone is not submitted.
                        s.push(if cfg!(windows) { '\r' } else { '\n' });
                        writer.write_all(s.as_bytes())
                    }
                    SessionInput::User(Input::Interrupt) => writer.write_all(&[3]),
                    SessionInput::User(Input::Escape) => writer.write_all(&[27]),
                    SessionInput::User(Input::Stop) => break,
                    #[cfg(windows)]
                    SessionInput::CursorPosition => writer.write_all(b"\x1b[1;1R"),
                };
                if res.and_then(|()| writer.flush()).is_err() {
                    break;
                }
            }
            let _ = child.kill();
            let status = child
                .wait()
                .map(|s| s.exit_code())
                .map_err(|e| e.to_string());
            let _ = exit_tx.send(status);
        });
        Ok(Self {
            pid,
            output,
            exit,
            tx,
            killer,
        })
    }
    pub fn send(&self, input: Input) -> Result<(), String> {
        self.tx
            .try_send(SessionInput::User(input))
            .map_err(|e| e.to_string())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            crate::process_tree::terminate(pid);
        }
        let _ = self.tx.try_send(SessionInput::User(Input::Stop));
        let _ = self.killer.kill();
    }
}
#[derive(Default)]
pub struct History {
    pub lines: Vec<String>,
    pub index: usize,
}
impl History {
    pub fn add(&mut self, line: &str) {
        if let Ok(s) = crate::command::CommandSpec::console(line.into()) {
            if !s.sensitive && self.lines.last().is_none_or(|s| s != line) {
                self.lines.push(line.into());
                if self.lines.len() > 100 {
                    self.lines.remove(0);
                }
                self.index = self.lines.len();
            }
        }
    }
    pub fn previous(&mut self) -> Option<&str> {
        if self.lines.is_empty() {
            return None;
        }
        self.index = self.index.saturating_sub(1);
        self.lines.get(self.index).map(String::as_str)
    }
    pub fn next_line(&mut self) -> Option<&str> {
        self.index = (self.index + 1).min(self.lines.len());
        self.lines.get(self.index).map(String::as_str)
    }
}

/// Persist command names/help only. Arguments, prompt responses and secrets stay out.
pub fn persistable(line: &str) -> bool {
    crate::capabilities::COMMANDS
        .iter()
        .any(|c| line == *c || line.strip_suffix(" -h") == Some(*c))
}

#[cfg(test)]
mod tests {
    use super::StartupCursorQuery;

    #[test]
    fn startup_cursor_handshake_survives_every_read_boundary_and_answers_once() {
        for split in 1..4 {
            let mut query = StartupCursorQuery::default();
            assert!(!query.observe(&b"\x1b[6n"[..split]));
            assert!(query.observe(&b"\x1b[6n"[split..]));
            assert!(!query.observe(b"\x1b[6n"));
        }
    }

    #[test]
    fn unrelated_terminal_output_does_not_trigger_the_startup_reply() {
        let mut query = StartupCursorQuery::default();
        assert!(!query.observe(b"[6n\x1b[?6n\x1b[6x\x1b"));
        assert!(query.observe(b"\x1b[6n"));
    }
}
