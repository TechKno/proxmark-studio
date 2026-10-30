//! Card placement fixtures for the explicit demo. This module never opens hardware.
pub fn respond(cmd: &str) -> Option<String> {
    let card = std::fs::read_to_string(".mock-pm3-card").ok()?;
    let card = card.trim();
    let hf = matches!(card, "ntag" | "ntag_blank" | "desfire" | "locked");
    let blank = matches!(card, "t5577" | "ignore");
    let uid = if card == "ntag_blank" {
        "04 77 88 99 AA BB CC"
    } else {
        "04 11 22 33 44 55 66"
    };
    let family = if card == "desfire" {
        "MIFARE DESFire EV3"
    } else {
        "NTAG 213"
    };
    if cmd == "lf search" {
        return Some(match card {
            "em" => "[+] EM 410x ID 0102030405\n[+] Valid EM410x ID found".into(),
            "na1" => "[+] EM 410x ID 00002A03E9\n[+] Valid EM410x ID found".into(),
            "hid" => "[+] Valid HID Prox ID found!".into(),
            "t5577" | "ignore" => {
                if let Ok(id) = std::fs::read_to_string(".mock-pm3-em") {
                    format!("[+] EM 410x ID {id}\n[+] Valid EM410x ID found")
                } else {
                    "[-] No known 125/134 kHz tags found!".into()
                }
            }
            _ => "[-] No known 125/134 kHz tags found!".into(),
        });
    }
    if cmd == "hf search" || cmd == "hf 14a info" {
        return Some(if hf {
            format!("[+] UID: {uid}\n[+] TYPE: {family}")
        } else {
            "[!!] No HF card found".into()
        });
    }
    if cmd == "hf mfu info" {
        return Some(if hf && card != "desfire" {
            format!("[+] UID: {uid}\n[+] TYPE: NTAG 213")
        } else {
            "[!!] No NTAG card found".into()
        });
    }
    if cmd == "lf t55xx detect" {
        return Some(if blank {
            "[=] Chip type......... T55x7\n[=] Block0............ 000880E0 (auto detect)\n[=] Password set...... No".into()
        } else {
            "[!!] No password-free T5577 destination found".into()
        });
    }
    if cmd.starts_with("lf em 410x clone ") {
        if !blank {
            return Some("[!!] Wrong destination".into());
        }
        let words = cmd.split_whitespace().collect::<Vec<_>>();
        let id = words
            .windows(2)
            .find(|w| w[0] == "--id")
            .map(|w| w[1])
            .unwrap_or("");
        if crate::command::hex(id, 5, "ID").is_err() {
            return Some("[!!] Invalid ID".into());
        }
        if card != "ignore" {
            std::fs::write(".mock-pm3-em", id).unwrap();
        }
        return Some("[+] Simulated EM410x write complete".into());
    }
    if cmd == "lf em 410x reader" {
        return Some(format!(
            "[+] EM 410x ID {}",
            std::fs::read_to_string(".mock-pm3-em").unwrap_or_else(|_| if card == "em" {
                "0102030405"
            } else if card == "na1" {
                "00002A03E9"
            } else {
                "0000000000"
            }
            .into())
        ));
    }
    if card == "ignore" && cmd.contains(" clone ") {
        return Some("[+] Simulated write ignored for mismatch fixture".into());
    }
    if cmd == "lf hid reader"
        && blank
        && !std::path::Path::new(".mock-pm3-hid.json").exists()
        && !std::path::Path::new(".mock-pm3-raw").exists()
    {
        let c = crate::credential::Credential {
            format: crate::credential::Format::H10301,
            facility_code: 0,
            card_number: 0,
        };
        return Some(format!(
            "[+] [H10301 ] HID H10301 26-bit FC: 0 CN: 0 parity ( ok )\n[=] raw: {}",
            c.encode().unwrap().hid_raw
        ));
    }
    None
}
