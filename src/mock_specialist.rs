//! Synthetic Classic/magic/signal fixtures. No serial or radio access.
use crate::{backup::Geometry, command};
use std::path::Path;
pub fn card_bytes(blank: bool) -> Vec<u8> {
    let mut b = vec![0; 1024];
    let uid = if blank {
        [0xA1, 0xB2, 0xC3, 0xD4]
    } else {
        [1, 2, 3, 4]
    };
    b[..4].copy_from_slice(&uid);
    b[4] = uid.iter().fold(0, |a, b| a ^ b);
    b[5] = 8;
    b[6] = 4;
    for s in 0..16 {
        let n = (s * 4 + 3) * 16;
        b[n..n + 6].fill(255);
        b[n + 6..n + 10].copy_from_slice(&[255, 7, 128, 105]);
        b[n + 10..n + 16].copy_from_slice(if blank {
            &[255; 6]
        } else {
            &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]
        });
    }
    if !blank {
        b[16..32].copy_from_slice(b"STUDIO LAB 00001");
    }
    b
}
pub fn frame() -> String {
    let id = 0x0102030405u64;
    let mut out = "1".repeat(9);
    let mut cols = [0; 4];
    for row in 0..10 {
        let nibble = (id >> (36 - row * 4)) & 15;
        let mut parity = 0;
        for (col, v) in cols.iter_mut().enumerate() {
            let bit = ((nibble >> (3 - col)) & 1) as u8;
            parity ^= bit;
            *v ^= bit;
            out.push(char::from(b'0' + bit));
        }
        out.push(char::from(b'0' + parity));
    }
    for c in cols {
        out.push(char::from(b'0' + c));
    }
    out.push('0');
    out
}
pub fn respond(cmd: &str) -> Option<String> {
    let t = cmd.split_whitespace().collect::<Vec<_>>();
    let opt = |n: &str| {
        t.iter()
            .position(|v| *v == n)
            .and_then(|i| t.get(i + 1))
            .copied()
            .unwrap_or("")
    };
    if cmd.starts_with("lf read ") {
        let n = opt("-s").parse::<usize>().ok()?;
        let bits = frame();
        let samples = (0..n)
            .map(|i| {
                if bits.as_bytes()[(i / 16) % 64] == b'1' {
                    90
                } else {
                    -90
                }
            })
            .collect::<Vec<_>>();
        std::fs::write(
            ".mock-pm3-signal",
            samples.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .ok()?;
        return Some(format!("[+] SIMULATED Got {n} samples"));
    }
    if cmd.starts_with("data save -f ") {
        let p = cmd.strip_prefix("data save -f ")?.trim_matches('"');
        let data = std::fs::read(".mock-pm3-signal").ok()?;
        if !Path::new(".mock-pm3-signal-missing").exists() {
            std::fs::write(format!("{p}.pm3"), data).ok()?;
        }
        return Some("[+] SIMULATED saved signal".into());
    }
    if cmd.starts_with("data load -f ") {
        let p = cmd.strip_prefix("data load -f ")?.trim_matches('"');
        return Some(
            if Path::new(p).exists() {
                "[+] SIMULATED loaded graph"
            } else {
                "[!!] Can't open signal"
            }
            .into(),
        );
    }
    if cmd.starts_with("data rawdemod ") {
        return Some(if Path::new(".mock-pm3-demod-fail").exists() {
            "[!!] Demodulation failed".into()
        } else {
            format!("[+] SIMULATED demodulation\n[+] {}", frame())
        });
    }
    let card = std::fs::read_to_string(".mock-pm3-card").unwrap_or_default();
    if !matches!(card.as_str(), "classic" | "magic1" | "magic2") {
        return None;
    }
    let path = Path::new(".mock-pm3-classic.bin");
    let mut data = std::fs::read(path).unwrap_or_else(|_| card_bytes(card != "classic"));
    if data.len() != 1024 {
        return Some("[!!] Invalid fixture memory".into());
    }
    if cmd == "hf 14a info" || cmd == "hf search" || cmd == "hf mf info" {
        return Some(format!(
            "[+] UID: {}\n[+] MIFARE Classic 1K\n{}",
            command::encode(&data[..4]),
            match card.as_str() {
                "magic1" => "[+] Magic capabilities... Gen 1a",
                "magic2" => "[+] Magic capabilities... Gen 2 / CUID",
                _ => "",
            }
        ));
    }
    if cmd.starts_with("hf mf autopwn ") || cmd.starts_with("hf mf fchk ") {
        let mut out = String::from("[+] Sec | Blk | key A |res| key B |res\n");
        for s in 0..16 {
            let n = (s * 4 + 3) * 16;
            let a = if Path::new(".mock-pm3-recovery-wrong").exists() {
                "000000000000".into()
            } else {
                command::encode(&data[n..n + 6])
            };
            let partial = Path::new(".mock-pm3-recovery-partial").exists();
            out.push_str(&format!(
                "[+] {s:03} | {:03} | {a} | 1 | {} | {}\n",
                s * 4 + 3,
                if partial {
                    "------------".into()
                } else {
                    command::encode(&data[n + 10..n + 16])
                },
                if partial { "0" } else { "1" }
            ));
        }
        return Some(out);
    }
    let read = cmd.starts_with("hf mf rdbl ") || cmd.starts_with("hf mf cgetblk ");
    let write = cmd.starts_with("hf mf wrbl ") || cmd.starts_with("hf mf csetblk ");
    if !read && !write {
        return None;
    }
    let n = opt("--blk").parse::<usize>().ok()?;
    if n >= Geometry::Classic1K.units() as usize {
        return Some("[!!] Invalid block".into());
    }
    let backdoor = cmd.contains(" cgetblk ") || cmd.contains(" csetblk ");
    if backdoor && card != "magic1" {
        return Some("[!!] Backdoor not supported".into());
    }
    let tr = (n / 4 * 4 + 3) * 16;
    if !backdoor {
        let key = opt("-k");
        let b = t.contains(&"-b");
        if key != command::encode(&data[tr + if b { 10 } else { 0 }..tr + if b { 16 } else { 6 }])
            || b && data[tr + 6..tr + 9] == [255, 7, 128]
        {
            return Some("[!!] Authentication failed".into());
        }
    }
    if write {
        let bytes = command::payload(opt("-d")).ok()?;
        if bytes.len() != 16 {
            return Some("[!!] Invalid data".into());
        }
        if !Path::new(".mock-pm3-ignore-writes").exists() {
            data[n * 16..n * 16 + 16].copy_from_slice(&bytes);
            std::fs::write(path, &data).ok()?;
        }
        return Some("[+] SIMULATED write completed".into());
    }
    let mut block = data[n * 16..n * 16 + 16].to_vec();
    if !backdoor && n % 4 == 3 {
        block[..6].fill(0);
        if data[tr + 6..tr + 9] != [255, 7, 128] {
            block[10..].fill(0);
        }
    }
    Some(format!(
        "[+] {n:03} | {} | synthetic",
        block
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    ))
}
