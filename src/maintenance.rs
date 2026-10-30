use crate::{command::encode, model::Settings};
use eframe::egui;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    time::{Duration, Instant},
};
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub sha256: String,
    pub flash_bytes: u64,
    pub bootloader: bool,
}
pub fn inspect_image(bytes: &[u8]) -> Result<Image, String> {
    let bad = || {
        "Expected a bounded ELF32 little-endian ARM executable with valid flash load segments"
            .to_string()
    };
    if bytes.len() < 52 || bytes.len() > 4 * 1024 * 1024 || &bytes[..7] != b"\x7fELF\x01\x01\x01" {
        return Err(bad());
    }
    let u16at = |i| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    if u16at(16) != 2 || u16at(18) != 40 || u16at(42) < 32 {
        return Err(bad());
    }
    let off = u32at(28) as usize;
    let size = u16at(42) as usize;
    let count = u16at(44) as usize;
    if count == 0
        || count > 128
        || off
            .checked_add(size * count)
            .is_none_or(|end| end > bytes.len())
    {
        return Err(bad());
    }
    let mut flash_bytes = 0;
    let mut bootloader = false;
    for n in 0..count {
        let p = off + n * size;
        if u32at(p) != 1 {
            continue;
        }
        let fileoff = u32at(p + 4) as usize;
        let addr = u32at(p + 12) as u64;
        let len = u32at(p + 16) as usize;
        let mem = u32at(p + 20) as usize;
        if len > mem || fileoff.checked_add(len).is_none_or(|e| e > bytes.len()) {
            return Err(bad());
        }
        if len == 0 {
            continue;
        }
        if !(0x100000..0x180000).contains(&addr) || addr + len as u64 > 0x180000 {
            return Err(bad());
        }
        flash_bytes += len as u64;
        bootloader |= addr < 0x102000;
    }
    if flash_bytes == 0 {
        return Err(bad());
    }
    Ok(Image {
        sha256: encode(&Sha256::digest(bytes)),
        flash_bytes,
        bootloader,
    })
}
#[derive(Debug, PartialEq)]
pub enum Compatibility {
    Matched(String),
    Mismatch { client: String, firmware: String },
    Unknown,
}
pub fn compatibility(output: &str) -> Compatibility {
    // Modern RRG prints the client version under a [ Client ] heading; older
    // builds put it on the same line as Client:. Compare ARM source fingerprints
    // too when supplied, because two local builds may share the Git revision.
    let revision = regex::Regex::new(r"(?i)(?:/|-g)([0-9a-f]{7,40})(?:[-+\s]|$)").unwrap();
    let bare = regex::Regex::new(r"(?i)^([0-9a-f]{7,40})(?:[-+\s]|$)").unwrap();
    let source = regex::Regex::new(r"(?i)\s([0-9a-f]{9})\s*$").unwrap();
    let label = regex::Regex::new(r"(?i)^(client|os|firmware)[. :]+\s*(.*)$").unwrap();
    let parse = |line: &str| {
        let cap = revision.captures(line).or_else(|| bare.captures(line))?;
        Some((
            cap[1].to_lowercase(),
            source.captures(line).map(|c| c[1].to_lowercase()),
        ))
    };
    let mut client = None;
    let mut firmware = None;
    let mut client_section = false;
    for raw in output.lines() {
        let clean = crate::parse::clean(raw);
        let line = clean.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            client_section = line
                .trim_matches(['[', ']'])
                .trim()
                .eq_ignore_ascii_case("client");
            continue;
        }
        if client_section {
            client = parse(line);
            client_section = false;
        }
        if let Some(cap) = label.captures(line) {
            if let Some(value) = parse(&cap[2]) {
                if cap[1].eq_ignore_ascii_case("client") {
                    client = Some(value);
                } else {
                    firmware = Some(value);
                }
            }
        }
    }
    match (client, firmware) {
        (Some((c, cs)), Some((f, fs))) => {
            let same_revision = c.starts_with(&f) || f.starts_with(&c);
            let same_source = cs.as_ref().zip(fs.as_ref()).is_none_or(|(a, b)| a == b);
            if same_revision && same_source {
                Compatibility::Matched(c)
            } else {
                Compatibility::Mismatch {
                    client: cs.map_or_else(|| c.clone(), |s| format!("{c} (ARM {s})")),
                    firmware: fs.map_or_else(|| f.clone(), |s| format!("{f} (ARM {s})")),
                }
            }
        }
        _ => Compatibility::Unknown,
    }
}
pub fn discover() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            dirs.push(p.to_owned());
        }
    }
    let mut found = vec![];
    for dir in dirs {
        for name in ["proxmark3", "proxmark3.exe", "pm3.exe"] {
            let p = dir.join(name);
            if p.is_file() && !found.contains(&p) {
                found.push(p);
            }
        }
    }
    found
}
pub struct Task {
    rx: Receiver<String>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Task {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}
impl Task {
    pub fn spawn(
        program: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        image: Option<tempfile::NamedTempFile>,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel(256);
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        std::thread::spawn(move || {
            let _image = image;
            let mut cmd = Command::new(program);
            cmd.args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if let Some(cwd) = cwd {
                cmd.current_dir(cwd);
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000);
            }
            crate::process_tree::isolate(&mut cmd);
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(format!("Cannot start tool: {e}\n[task ended]"));
                    return;
                }
            };
            for mut stream in [
                Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>,
                Box::new(child.stderr.take().unwrap()) as Box<dyn Read + Send>,
            ] {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let mut b = [0_u8; 4096];
                    while let Ok(n) = stream.read(&mut b) {
                        if n == 0 {
                            break;
                        }
                        if tx.send(String::from_utf8_lossy(&b[..n]).into()).is_err() {
                            break;
                        }
                    }
                });
            }
            let start = Instant::now();
            loop {
                if c.load(Ordering::SeqCst) || start.elapsed() > Duration::from_secs(1800) {
                    crate::process_tree::terminate(child.id());
                    let _ = child.kill();
                    let _ = child.wait();
                    let _=tx.send("Cancelled or exceeded 30 minutes. Flash state is unknown if interrupted.\n[task ended]".into());
                    break;
                }
                match child.try_wait() {
                    Ok(Some(s)) => {
                        let _ = tx.send(format!("Tool exited: {s}\n[task ended]"));
                        break;
                    }
                    Err(e) => {
                        crate::process_tree::terminate(child.id());
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = tx.send(format!("{e}\n[task ended]"));
                        break;
                    }
                    _ => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        });
        Self { rx, cancel }
    }
}
#[derive(Default)]
pub struct Maintenance {
    pub task: Option<Task>,
    source: String,
    image: String,
    inspected: Option<Image>,
    ack: bool,
    output: String,
    candidates: Vec<PathBuf>,
    pub busy: bool,
}
impl Maintenance {
    pub fn ui(&mut self, ui: &mut egui::Ui, settings: &mut Settings) {
        if let Some(t) = &self.task {
            for s in t.rx.try_iter().take(128) {
                self.output.push_str(&s);
                if s.contains("[task ended]") {
                    self.busy = false;
                }
            }
            if self.output.len() > 128 * 1024 {
                let mut cut = self.output.len() - 65536;
                while !self.output.is_char_boundary(cut) {
                    cut += 1;
                }
                self.output.drain(..cut);
            }
        }
        if !self.busy {
            self.task = None;
        }
        ui.heading("Client and firmware tools");
        ui.label("Client/firmware versions must match. Easy hardware requires a Generic build with the correct flash size. ELF validation checks structure, not board compatibility or provenance.");
        if self.busy {
            if ui.button("Cancel tool").clicked() {
                if let Some(t) = &self.task {
                    t.cancel.store(true, Ordering::SeqCst)
                }
            }
            ui.spinner();
        }
        ui.add_enabled_ui(!self.busy,|ui|{
   if ui.button("Find installed clients").clicked(){self.candidates=discover();}
   for p in &self.candidates{if ui.button(p.display().to_string()).clicked(){settings.cli=p.to_string_lossy().into_owned();}}
   ui.label("Official RRG source folder (new folder for download)");ui.text_edit_singleline(&mut self.source);
   if ui.button("Download official source").clicked(){if self.source.trim().is_empty()||Path::new(&self.source).exists(){self.output="Choose a new, non-empty source folder".into()}else{self.output.clear();self.task=Some(Task::spawn("git".into(),vec!["clone".into(),"--depth".into(),"1".into(),"https://github.com/RfidResearchGroup/proxmark3.git".into(),self.source.clone()],None,None));self.busy=true;}}
   if ui.button("Build Generic client from source").clicked(){if !Path::new(&self.source).join("client/Makefile").is_file(){self.output="Select an RRG source checkout".into()}else if cfg!(windows){self.output="Native Windows source builds require ProxSpace/make and compiler setup. Use its supported build environment, then select proxmark3.exe in Connection.".into()}else{self.output.clear();self.task=Some(Task::spawn("make".into(),vec!["client".into(),"-j4".into(),"PLATFORM=PM3GENERIC".into(),"SKIPQT=1".into(),"SKIPBT=1".into(),"SKIPGD=1".into()],Some(PathBuf::from(&self.source)),None));self.busy=true;}}
   let built=Path::new(&self.source).join(if cfg!(windows){"client/proxmark3.exe"}else{"client/proxmark3"});if built.is_file()&&ui.button("Use built client").clicked(){settings.cli=built.to_string_lossy().into_owned();settings.working_dir=self.source.clone();}
   ui.separator();ui.label("Firmware ELF image");if ui.text_edit_singleline(&mut self.image).changed(){self.inspected=None;self.ack=false;}
   if ui.button("Inspect image").clicked(){match crate::storage::read_bounded(Path::new(&self.image),4*1024*1024).map_err(|e|e.to_string()).and_then(|b|inspect_image(&b)){Ok(i)=>{self.output=format!("SHA-256: {}\nFlash bytes: {}\nIncludes bootloader region: {}",i.sha256,i.flash_bytes,i.bootloader);self.inspected=Some(i);self.ack=false},Err(e)=>{self.inspected=None;self.output=e}}}
   ui.checkbox(&mut self.ack,"I checked this is Generic firmware for the selected device and flash size. Power is stable and I accept recovery may be needed.");
   let enabled=self.ack&&self.inspected.as_ref().is_some_and(|i|!i.bootloader)&&!settings.port.is_empty();
   if ui.add_enabled(enabled,egui::Button::new("Flash reviewed application image")).clicked(){let res=(||{crate::runner::validate_cli_settings(settings)?;let b=crate::storage::read_bounded(Path::new(&self.image),4*1024*1024).map_err(|e|e.to_string())?;let i=inspect_image(&b)?;if Some(&i)!=self.inspected.as_ref(){return Err("Image changed after review".into())}let mut file=tempfile::NamedTempFile::new().map_err(|e|e.to_string())?;file.write_all(&b).map_err(|e|e.to_string())?;Ok(file)})();match res{Ok(file)=>{let args=vec!["-p".into(),settings.port.clone(),"--flash".into(),"--image".into(),file.path().to_string_lossy().into_owned()];self.output.clear();self.task=Some(Task::spawn(settings.cli.clone(),args,(!settings.working_dir.is_empty()).then(||PathBuf::from(&settings.working_dir)),Some(file)));self.busy=true;self.ack=false},Err(e)=>self.output=e}}
   ui.add_enabled(false,egui::Button::new("Bootloader unlock / forced cross-version flash — disabled"));
  });
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.monospace(&self.output);
            });
    }
}
