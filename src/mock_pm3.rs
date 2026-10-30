//! Explicitly simulated RRG client for integration testing. Never accesses serial ports.
use std::io::{self, BufRead, IsTerminal, Write};
fn main() {
    println!("[=] SIMULATED PROXMARK3 CLIENT • NOT HARDWARE");
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|s| s == "--version") {
        println!("mock-pm3 {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let tty = io::stdin().is_terminal();
    if tty {
        print!("[offline] pm3 --> ");
        io::stdout().flush().unwrap();
    }
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        for cmd in line.split(';').map(str::trim) {
            if cmd == "quit" {
                return;
            }
            if cmd.ends_with(" -h") {
                if std::path::Path::new(".mock-pm3-probe-stall").exists() {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                }
                let base = cmd.trim_end_matches(" -h");
                if base == "hw ping" {
                    println!("Test USB response\nusage:\n hw ping [-h] [-l <dec>]\noptions:\n -h, --help     This help\n -l, --len <dec>     Payload length\nexamples/notes:\n hw ping --len 32");
                } else {
                    println!("usage:\n  {base} [options]\n--aid --fid --no-auth --schann <d40|ev1> -n -t -k -m --type -o -l -d --commit -b --id -w --fc --cn --q5 --em --bin --fmt --vn --blk -a --rawrights --size --backup --maxrecord --cyclic --lower --upper --value --amode --newkeyno --oldalgo --oldkey --newalgo --newkey --newver --kdf -i --ks1 --dstalgo --numkeys --dfhex --isoid -c -v --keynum --ks2 --amode --updaterec --lcredit --isofid -r --issue --oem --pre --mini --1k --2k --4k --ns --no-default -s -f --am --ab --ar --fs --nr --p1 --p2");
                }
            } else if let Some(response) = proxmark_studio::mock_specialist::respond(cmd) {
                println!("{response}");
            } else if let Some(response) = proxmark_studio::mock_tools::respond(cmd) {
                println!("{response}");
            } else if let Some(response) = proxmark_studio::mock_basic::respond(cmd) {
                println!("{response}");
            } else if let Some(response) = proxmark_studio::mock_desfire::respond(cmd) {
                println!("{response}");
            } else if cmd == "wiegand list" {
                for f in proxmark_studio::credential::Format::ALL {
                    println!("[=] {} HID {} {}-bit", f.name(), f.name(), f.bits());
                }
            } else if cmd.starts_with("wiegand encode ") {
                let t = cmd.split_whitespace().collect::<Vec<_>>();
                let option = |n| {
                    t.iter()
                        .position(|v| *v == n)
                        .and_then(|i| t.get(i + 1))
                        .copied()
                        .unwrap_or("")
                };
                let result = proxmark_studio::credential::Format::ALL
                    .into_iter()
                    .find(|f| f.name() == option("-w"))
                    .ok_or("Unknown simulated format".into())
                    .and_then(|f| {
                        proxmark_studio::credential::Credential::from_decimal(
                            f,
                            option("--fc"),
                            option("--cn"),
                        )
                    })
                    .and_then(|c| c.encode());
                match result {
                    Ok(e) => println!("[+] Wiegand: {}", e.hid_raw),
                    Err(e) => println!("[!!] {e}"),
                }
            } else if cmd.starts_with("lf hid clone -r ") {
                let raw = cmd.split_whitespace().nth(4).unwrap_or("");
                match proxmark_studio::credential::normalise_raw(raw) {
                    Ok(raw) => {
                        std::fs::write(".mock-pm3-raw", raw).unwrap();
                        println!("[+] Simulated raw clone complete");
                    }
                    Err(e) => println!("[!!] {e}"),
                }
            } else if cmd == "mock timeout" {
                std::thread::sleep(std::time::Duration::from_secs(10));
            } else if cmd == "mock exit" {
                std::process::exit(7);
            } else if cmd == "mock error" {
                println!("[!!] Authentication failed");
            } else if cmd == "mock large" {
                for _ in 0..300 {
                    println!("{}", "x".repeat(9000));
                }
            } else if let Some(echo) = cmd.strip_prefix("mock echo ") {
                println!("[=] {}", echo);
            } else if cmd.starts_with("lf hid clone --bin ") {
                let binary = cmd.split_whitespace().nth(4).unwrap_or("");
                if binary.len() > 84
                    || binary.is_empty()
                    || !binary.bytes().all(|b| matches!(b, b'0' | b'1'))
                {
                    println!("[!!] Invalid binary");
                } else {
                    let value = u128::from_str_radix(binary, 2).unwrap()
                        | proxmark_studio::credential::hid_header(binary.len());
                    std::fs::write(".mock-pm3-raw", format!("{value:024X}")).unwrap();
                    println!("[+] Simulated binary write complete");
                }
            } else if cmd == "lf hid reader" && std::path::Path::new(".mock-pm3-raw").exists() {
                println!(
                    "[=] raw: {}",
                    std::fs::read_to_string(".mock-pm3-raw").unwrap()
                );
            } else if cmd.starts_with("hf mfu rdbl ")
                || cmd.starts_with("hf mfu wrbl ")
                || cmd.starts_with("hf mf rdbl ")
                || cmd.starts_with("hf mf wrbl ")
            {
                let tokens = cmd.split_whitespace().collect::<Vec<_>>();
                let option = |name| {
                    tokens
                        .iter()
                        .position(|s| *s == name)
                        .and_then(|i| tokens.get(i + 1))
                        .copied()
                        .unwrap_or("")
                };
                let classic = cmd.starts_with("hf mf ");
                let width = if classic { 16 } else { 4 };
                let unit = option(if classic { "--blk" } else { "-b" })
                    .parse::<usize>()
                    .unwrap_or(4);
                let mut data = std::fs::read(".mock-pm3-memory.bin").unwrap_or_else(|_| {
                    let mut b = vec![0; 4096];
                    b[16..20].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
                    b
                });
                if classic && std::path::Path::new(".mock-pm3-classic-auth").exists() {
                    let sector = proxmark_studio::classic::sector(unit as u32).unwrap();
                    let block = proxmark_studio::classic::trailer(sector).unwrap() as usize;
                    let offset = block * 16 + if tokens.contains(&"-b") { 10 } else { 0 };
                    let expected = proxmark_studio::command::encode(&data[offset..offset + 6]);
                    if !option("-k").eq_ignore_ascii_case(&expected) {
                        println!("[!!] Authentication failed");
                        io::stdout().flush().unwrap();
                        continue;
                    }
                }
                if cmd.contains(" wrbl ") {
                    let hex = option("-d");
                    if hex.len() == width * 2 && unit * width + width <= data.len() {
                        for i in 0..width {
                            data[unit * width + i] =
                                u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
                        }
                        if std::path::Path::new(".mock-pm3-enable-state").exists()
                            && !std::path::Path::new(".mock-pm3-ignore-writes").exists()
                        {
                            std::fs::write(".mock-pm3-memory.bin", &data).unwrap();
                        }
                        println!("[+] Simulated data write complete");
                    } else {
                        println!("[!!] Invalid data");
                    }
                } else if unit * width + width <= data.len() {
                    if unit == 2 && !classic && std::path::Path::new(".mock-pm3-locked").exists() {
                        data[unit * width + 2] = 1;
                    }
                    let body =
                        proxmark_studio::command::encode(&data[unit * width..unit * width + width]);
                    let spaced = (0..body.len())
                        .step_by(2)
                        .map(|i| &body[i..i + 2])
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!("[+] {unit:03}/0x{unit:02X} | {spaced} | ....");
                }
            } else if cmd == "hf mf info" {
                println!("[+] UID: 04 11 22 33 44 55 66\n[+] TYPE: MIFARE Classic 1K");
            } else if cmd == "hf mfu info" {
                println!("[+] UID: 04 11 22 33 44 55 66\n[+] TYPE: NTAG 213");
            } else if cmd == "hf 14a info" {
                let uid = if std::path::Path::new(".mock-pm3-changed-uid").exists() {
                    "04 99 22 33 44 55 66"
                } else {
                    "04 11 22 33 44 55 66"
                };
                println!("[+] UID: {uid}");
            } else if cmd.starts_with("lf awid clone ") || cmd.starts_with("lf io clone ") {
                let t = cmd.split_whitespace().collect::<Vec<_>>();
                let option = |n| {
                    t.iter()
                        .position(|s| *s == n)
                        .and_then(|i| t.get(i + 1))
                        .copied()
                        .unwrap_or("0")
                };
                let (fc, cn, fmt, v) = (
                    option("--fc"),
                    option("--cn"),
                    option("--fmt"),
                    option("--vn"),
                );
                let output = if cmd.starts_with("lf io") {
                    format!(
                        "[+] IO Prox - XSF({:02}){:02x}:{:05}, Raw: 1234567890abcdef ( ok )",
                        v.parse::<u8>().unwrap(),
                        fc.parse::<u32>().unwrap(),
                        cn.parse::<u32>().unwrap()
                    )
                } else {
                    format!("[+] AWID - len: {fmt} FC: {fc} Card: {cn} - Wiegand: 1234, Raw: 1234567890abcdef12345678")
                };
                std::fs::write(".mock-pm3-lf", output).unwrap();
                println!("[+] Simulated LF write complete");
            } else if cmd == "lf awid reader" || cmd == "lf io reader" {
                println!(
                    "{}",
                    std::fs::read_to_string(".mock-pm3-lf")
                        .unwrap_or("[!!] No simulated LF credential".into())
                );
            } else if cmd == "lf hid reader" {
                use proxmark_studio::credential::{Credential, Format};
                let port = args
                    .iter()
                    .position(|a| a == "-p")
                    .and_then(|i| args.get(i + 1))
                    .map(String::as_str)
                    .unwrap_or("");
                if port == "SIMULATED-NORAW" {
                    println!("[+] [H10301 ] HID H10301 26-bit FC: 118 CN: 1603 parity ( ok )");
                } else if port == "SIMULATED-MULTI" {
                    println!(
                        "[=] raw: 000000000000002006EC0C86\n[=] raw: 000000000000002004EC3672"
                    );
                } else {
                    let mut value = if std::path::Path::new(".mock-pm3-enable-state").exists() {
                        std::fs::read(".mock-pm3-hid.json")
                            .ok()
                            .and_then(|b| serde_json::from_slice::<Credential>(&b).ok())
                            .unwrap_or(Credential {
                                format: Format::H10301,
                                facility_code: 118,
                                card_number: 1603,
                            })
                    } else {
                        Credential {
                            format: Format::H10301,
                            facility_code: 118,
                            card_number: 1603,
                        }
                    };
                    if port == "SIMULATED-MISMATCH" {
                        value.card_number += 1;
                    }
                    if port == "SIMULATED-WRONGFORMAT" {
                        value.format = Format::H10306;
                    }
                    let encoded = value.encode().unwrap();
                    let raw = if port == "SIMULATED-BADPARITY" {
                        format!(
                            "{:024X}",
                            u128::from_str_radix(&encoded.hid_raw, 16).unwrap() ^ 1
                        )
                    } else {
                        encoded.hid_raw
                    };
                    println!(
                        "[+] [{} ] HID {} {}-bit FC: {} CN: {} parity ( ok )",
                        value.format.name(),
                        value.format.name(),
                        value.format.bits(),
                        value.facility_code,
                        value.card_number
                    );
                    if port == "SIMULATED-ALTERNATE" {
                        println!("[-] Parity tests failed");
                    }
                    println!("[=] raw: {raw}");
                }
            } else if cmd.starts_with("lf hid clone ") {
                use proxmark_studio::credential::{Credential, Format};
                let tokens: Vec<_> = cmd.split_whitespace().collect();
                let option = |name: &str| {
                    tokens
                        .iter()
                        .position(|t| *t == name)
                        .and_then(|i| tokens.get(i + 1))
                        .copied()
                        .unwrap_or("")
                };
                let format = Format::ALL.into_iter().find(|f| f.name() == option("-w"));
                let value = format.ok_or("Unknown format".into()).and_then(|format| {
                    Credential::from_decimal(format, option("--fc"), option("--cn"))
                });
                if args.iter().any(|s| s == "SIMULATED-FAILWRITE") {
                    println!("[-] Cloning ( fail )");
                } else {
                    match value {
                        Ok(value) => {
                            if std::path::Path::new(".mock-pm3-enable-state").exists() {
                                let _ = std::fs::remove_file(".mock-pm3-raw");
                                std::fs::write(
                                    ".mock-pm3-hid.json",
                                    serde_json::to_vec(&value).unwrap(),
                                )
                                .unwrap();
                            }
                            println!("[+] Done! SIMULATED write only");
                        }
                        Err(e) => println!("[!!] {e}"),
                    }
                }
            } else if cmd.contains("hw version") {
                let firmware = if std::path::Path::new(".mock-pm3-version-mismatch").exists() {
                    "1234567"
                } else {
                    "abcdef0"
                };
                println!("[+] device: Proxmark3 Easy (SIMULATED)\nClient: Iceman/master/abcdef0\nOS: Iceman/master/{firmware}");
            } else if cmd.contains("getaids") {
                println!("[+] --- AID list ( 2 found )\n[+] 112233 Example\n[+] A0B0C0 Example");
            } else if cmd.contains("getfileids") {
                println!("[=] ---- File ID list ----\n[=] File ID: 01\n[=] File ID: 02");
            } else if cmd.contains("getfilesettings") {
                println!("[+] File type        : Standard\n[+] File number      : 0x01\n[+] File comm mode   : Plain\n[+] Access rights    : EEEE\n[+] File size        : 4");
            } else if cmd.contains("mfdes auth") {
                println!("[+] Application selected and authenticated succesfully\n[+] Session key: 00112233445566778899AABBCCDDEEFF");
            } else if cmd.contains("mfdes read") {
                println!(
                    "[+] Read 4 bytes from file 0x01 offset 0\n[=]   0/0x00 | DE AD BE EF | ...."
                );
            } else if cmd.contains("mfu rdbl") {
                println!("[+] 04/0x04 | DE AD BE EF | ....");
            } else if cmd.contains("410x reader") {
                println!("[+] EM 410x ID 0102030405");
            } else if cmd.contains("write") || cmd.contains("wrbl") || cmd.contains("clone") {
                println!("[+] Simulated write ended; no physical tag altered");
            } else if cmd.contains("search") || cmd.ends_with("info") || cmd.contains("getversion")
            {
                println!("[+] UID: 04 11 22 33 44 55 66\n[+] SIMULATED tag found");
            } else {
                println!("[=] Simulated console: {cmd}");
            }
            if args.iter().any(|s| s == "-l") {
                println!("[STUDIO_BRIDGE_DONE]");
            }
            if tty {
                print!("[offline] pm3 --> ");
            }
            io::stdout().flush().unwrap();
        }
    }
}
