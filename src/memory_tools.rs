//! Local draft operations. These functions never access a card.
use crate::{
    backup::{Dump, Geometry},
    command,
};
use std::collections::BTreeSet;

pub fn selection(input: &str, geometry: Geometry) -> Result<Vec<u32>, String> {
    if input.len() > 4096 {
        return Err("Selection is too long".into());
    }
    let mut out = BTreeSet::new();
    for token in input.split(',').map(str::trim) {
        if token.is_empty() {
            return Err("Enter blocks/pages such as 4-8,12,16".into());
        }
        let (a, b) = token
            .split_once('-')
            .map_or((token, token), |(a, b)| (a.trim(), b.trim()));
        let number = |s: &str| -> Result<u32, String> {
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err("Use decimal unit numbers".into());
            }
            s.parse().map_err(|_| "Invalid unit number".into())
        };
        let a = number(a)?;
        let b = number(b)?;
        if a > b || b >= geometry.units() {
            return Err("Selection exceeds the chosen geometry or has a reversed range".into());
        }
        out.extend(a..=b);
    }
    Ok(out.into_iter().collect())
}
pub fn search(dump: &Dump, input: &str, ascii: bool) -> Result<Vec<usize>, String> {
    dump.validate()?;
    let needle = if ascii {
        input.as_bytes().to_vec()
    } else {
        command::payload(input)?
    };
    if needle.is_empty() || needle.len() > dump.bytes.len() {
        return Err("Search needs a non-empty value no larger than the dump".into());
    }
    Ok(dump
        .bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(i, b)| (b == needle).then_some(i))
        .collect())
}
#[derive(Default)]
pub struct DraftHistory {
    undo: Vec<Dump>,
    redo: Vec<Dump>,
}
impl DraftHistory {
    pub fn reset(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
    pub fn record(&mut self, old: &Dump) {
        self.undo.push(old.clone());
        if self.undo.len() > 20 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    pub fn undo(&mut self, current: &mut Dump) -> bool {
        if let Some(d) = self.undo.pop() {
            self.redo.push(current.clone());
            *current = d;
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self, current: &mut Dump) -> bool {
        if let Some(d) = self.redo.pop() {
            self.undo.push(current.clone());
            *current = d;
            true
        } else {
            false
        }
    }
}
pub fn edit_unit(dump: &Dump, unit: u32, text: &str, protected_ack: bool) -> Result<Dump, String> {
    dump.validate()?;
    if unit >= dump.geometry.units() {
        return Err("Unit is outside this dump".into());
    }
    if !dump.geometry.writable(unit) && !protected_ack {
        return Err("This is protected metadata. Acknowledge a local draft edit first; hardware write guards remain in place.".into());
    }
    let text = command::hex(text, dump.geometry.width(), "Unit data")?;
    let bytes = command::payload(&text)?;
    let mut out = dump.clone();
    let start = unit as usize * dump.geometry.width();
    out.bytes[start..start + bytes.len()].copy_from_slice(&bytes);
    out.sha256 = out.digest();
    out.validate()?;
    Ok(out)
}
pub fn permissions(dump: &Dump, block: u32) -> Result<Vec<String>, String> {
    dump.validate()?;
    if !dump.geometry.classic() || block >= dump.geometry.units() {
        return Err("Select a Classic block".into());
    }
    let sector = crate::classic::sector(block)?;
    let trailer = crate::classic::trailer(sector)?;
    let start = trailer as usize * 16;
    let conditions = crate::classic::access_bits(&dump.bytes[start + 6..start + 9])?;
    let group = if block == trailer {
        3
    } else if block < 128 {
        block % 4
    } else {
        (block - 128) % 16 / 5
    } as usize;
    let c = conditions[group];
    let mut lines = vec![format!(
        "Sector {sector}, trailer block {trailer}, access group {group}, condition {c:03b}"
    )];
    if group == 3 {
        let (write_keys, read_bits, write_bits, read_b) = match c {
            0 => ("A", "A", "never", "A"),
            1 => ("A", "A", "A", "A"),
            2 => ("never", "A", "never", "A"),
            3 => ("B", "A or B", "B", "never"),
            4 => ("B", "A or B", "never", "never"),
            5 => ("never", "A or B", "B", "never"),
            _ => ("never", "A or B", "never", "never"),
        };
        lines.push(format!("Write keys: {write_keys}; read access bytes: {read_bits}; write access bytes: {write_bits}; read key B: {read_b}. Key A is never readable."));
    } else {
        let (read, write, increment, decrement) = match c {
            0 => ("A or B", "A or B", "A or B", "A or B"),
            1 => ("A or B", "never", "never", "A or B"),
            2 => ("A or B", "never", "never", "never"),
            3 => ("B", "B", "never", "never"),
            4 => ("A or B", "B", "never", "never"),
            5 => ("B", "never", "never", "never"),
            6 => ("A or B", "B", "B", "A or B"),
            _ => ("never", "never", "never", "never"),
        };
        lines.push(format!("Read: {read}; write: {write}; increment: {increment}; decrement/transfer/restore: {decrement}."));
    }
    if conditions[3] <= 2 {
        lines.push(
            "Key B is readable under this trailer policy and cannot authenticate as a key.".into(),
        );
    }
    lines.push("These permissions describe the saved bytes. They do not establish current card state or key recovery.".into());
    Ok(lines)
}
