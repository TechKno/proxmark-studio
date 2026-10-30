//! Stateful fixtures for tools integration. Never opens a device.
use crate::{backup::Geometry, command};
pub fn respond(cmd: &str) -> Option<String> {
    let tokens = cmd.split_whitespace().collect::<Vec<_>>();
    let option = |name: &str| {
        tokens
            .iter()
            .position(|s| *s == name)
            .and_then(|i| tokens.get(i + 1))
            .copied()
            .unwrap_or("")
    };
    if cmd == "XX_internal_command_dump_XX" {
        return Some("command |offline|description\nhw version |N|Show client and device versions\nhw ping |N|Test USB response\nhw status |N|Show reader status\nhw tune |N|Measure antennas\nhw break |N|Request RF stop\nhf search |N|Find an HF card\nlf search |N|Find an LF card\nhf mf eview |N|View emulator memory\nhf mf sim |N|Simulate a Classic image\nhf mfdes pc |N|Unsupported proximity check\nwiegand decode |Y|Decode a raw credential".into());
    }
    match cmd {
        "hw ping" | "hw ping --len 32" => {
            return Some("[+] SIMULATED ping response received".into())
        }
        "hw status" => return Some("[=] SIMULATED device status: ready".into()),
        "hw tune" => return Some(
            "[=] SIMULATED antenna measurements\n[+] LF antenna: 25.00 V\n[+] HF antenna: 28.00 V"
                .into(),
        ),
        "hw break" => {
            let _ = std::fs::remove_file(".mock-pm3-rf-active");
            return Some("[=] SIMULATED stop request sent (no hardware acknowledgement)".into());
        }
        _ => {}
    }
    let path = std::path::Path::new(".mock-pm3-emulator.bin");
    if cmd.starts_with("hf mf esetblk ") {
        let block = option("--blk").parse::<usize>().ok()?;
        if block >= 256 {
            return Some("[!!] Invalid emulator block".into());
        }
        let b = match command::hex(option("-d"), 16, "Emulator data")
            .and_then(|s| command::payload(&s))
        {
            Ok(b) => b,
            Err(e) => return Some(format!("[!!] {e}")),
        };
        let mut data = std::fs::read(path).unwrap_or_else(|_| vec![0; 4096]);
        data.resize(4096, 0);
        if !std::path::Path::new(".mock-pm3-ignore-writes").exists() {
            data[block * 16..block * 16 + 16].copy_from_slice(&b);
            std::fs::write(path, data).unwrap();
        }
        return Some("[+] SIMULATED emulator block stored".into());
    }
    if cmd.starts_with("hf mf eview ") {
        let g = if tokens.contains(&"--mini") {
            Geometry::ClassicMini
        } else if tokens.contains(&"--2k") {
            Geometry::Classic2K
        } else if tokens.contains(&"--4k") {
            Geometry::Classic4K
        } else {
            Geometry::Classic1K
        };
        let data = std::fs::read(path).unwrap_or_else(|_| vec![0; 4096]);
        let count = if std::path::Path::new(".mock-pm3-emulator-short").exists() {
            g.units() - 1
        } else {
            g.units()
        };
        let mut out = String::from("[=] sec | blk | data | ascii\n");
        for n in 0..count as usize {
            if n * 16 + 16 > data.len() {
                break;
            }
            let bytes = data[n * 16..n * 16 + 16]
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!("[=]     | {n} | {bytes} | ................\n"));
        }
        return Some(out);
    }
    if cmd.starts_with("hf mf sim ") {
        std::fs::write(".mock-pm3-rf-active", b"SIMULATED").unwrap();
        return Some("[=] SIMULATED RF start requested; press device button to stop".into());
    }
    None
}
