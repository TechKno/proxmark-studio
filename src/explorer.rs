//! Independent, conservative adapter for the selected client's own help.
use crate::{
    command::{self, CommandSpec, Kind},
    parse,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use zeroize::Zeroizing;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub description: String,
    pub offline: bool,
}
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.split(' ').all(|t| {
            !t.is_empty()
                && t.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}
pub fn parse_catalogue(raw: &str) -> Result<Vec<Entry>, String> {
    if raw.len() > 2 * 1024 * 1024 {
        return Err("Command catalogue exceeds limit".into());
    }
    let mut entries = BTreeMap::new();
    for line in raw.lines() {
        let line = parse::clean(line);
        let fields = line.split('|').map(str::trim).collect::<Vec<_>>();
        if fields.len() != 3 || !valid_name(fields[0]) || !["Y", "N"].contains(&fields[1]) {
            continue;
        }
        let entry = Entry {
            name: fields[0].into(),
            offline: fields[1] == "Y",
            description: fields[2].into(),
        };
        if entry.description.len() > 1000 {
            return Err("Catalogue description exceeds limit".into());
        }
        if let Some(old) = entries.get(&entry.name) {
            if old != &entry {
                return Err("Conflicting duplicate catalogue entry".into());
            }
        } else {
            entries.insert(entry.name.clone(), entry);
        }
        if entries.len() > 5000 {
            return Err("Too many command entries".into());
        }
    }
    if entries.is_empty() {
        return Err("This client did not return a recognised command catalogue".into());
    }
    Ok(entries.into_values().collect())
}
pub fn help_command(name: &str) -> Result<CommandSpec, String> {
    if !valid_name(name) {
        return Err("Invalid command name".into());
    }
    // Help is allowed for unsupported commands; execution is separately rejected.
    Ok(CommandSpec {
        text: Zeroizing::new(format!("{name} -h")),
        label: "Read command help".into(),
        sensitive: false,
        kind: Kind::Help,
    })
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionField {
    pub flag: String,
    pub value_hint: Option<String>,
    pub description: String,
    pub required: bool,
}
pub fn options(name: &str, help: &str) -> Result<Vec<OptionField>, String> {
    if !valid_name(name) || help.len() > 128 * 1024 {
        return Err("Invalid help request".into());
    }
    let clean = help
        .lines()
        .map(parse::clean)
        .collect::<Vec<_>>()
        .join("\n");
    let lower = clean.to_lowercase();
    let usage = lower
        .find("usage:")
        .ok_or("No structured usage found; use the Console for this command")?;
    let opts = lower[usage..]
        .find("options:")
        .map(|i| usage + i)
        .ok_or("No structured option list found")?;
    if !clean[usage..opts].contains(name) {
        return Err("Help does not match the selected command".into());
    }
    let usage_text = &clean[usage..opts];
    let flag_re = regex::Regex::new(r"(?:^|[ ,])(--?[a-zA-Z0-9][a-zA-Z0-9_-]*)").unwrap();
    let hint_re = regex::Regex::new(r"<([^>]+)>").unwrap();
    let split_re = regex::Regex::new(r"\s{2,}").unwrap();
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    for line in clean[opts + 8..].lines() {
        let line = line.trim();
        if line.starts_with("examples") || line.starts_with("notes:") {
            break;
        }
        if !line.starts_with('-') {
            continue;
        }
        let mut parts = split_re.splitn(line, 2);
        let syntax = parts.next().unwrap();
        let description = parts.next().unwrap_or("").to_string();
        let flags = flag_re
            .captures_iter(syntax)
            .map(|c| c[1].to_owned())
            .collect::<Vec<_>>();
        let Some(flag) = flags.iter().find(|f| f.starts_with("--")).or(flags.first()) else {
            continue;
        };
        if flag == "--help" || flag == "-h" {
            continue;
        }
        if !seen.insert(flag.clone()) {
            return Err("Ambiguous duplicate option in help".into());
        }
        let hint = hint_re.captures(syntax).map(|c| c[1].to_owned());
        // Only mark explicitly unbracketed switches as required. Complex syntax is not inferred.
        let mut depth = 0usize;
        let mut required = false;
        for token in usage_text.split_whitespace() {
            let before = depth;
            depth += token.bytes().filter(|b| *b == b'[').count();
            let bare = token.trim_matches(['[', ']']);
            if before == 0 && !token.starts_with('[') && flags.iter().any(|f| f == bare) {
                required = true;
            }
            depth = depth.saturating_sub(token.bytes().filter(|b| *b == b']').count());
        }
        out.push(OptionField {
            flag: flag.clone(),
            value_hint: hint,
            description,
            required,
        });
        if out.len() > 100 {
            return Err("Too many form options".into());
        }
    }
    Ok(out)
}
pub fn build(
    entry: &Entry,
    fields: &[OptionField],
    enabled: &BTreeSet<String>,
    values: &BTreeMap<String, Zeroizing<String>>,
) -> Result<CommandSpec, String> {
    if !valid_name(&entry.name) {
        return Err("Invalid command name".into());
    }
    if enabled.iter().any(|f| !fields.iter().any(|p| &p.flag == f)) {
        return Err("Option was not advertised by this client".into());
    }
    if entry.name == "hf mf sim" {
        return Err("Use Emulator images for verified, tracked Classic RF simulation".into());
    }
    let mut line = entry.name.clone();
    for field in fields {
        if field.required && !enabled.contains(&field.flag) {
            return Err(format!("{} is required", field.flag));
        }
        if !enabled.contains(&field.flag) {
            continue;
        }
        line.push(' ');
        line.push_str(&field.flag);
        if field.value_hint.is_some() {
            let v = values.get(&field.flag).map(|v| v.trim()).unwrap_or("");
            if v.is_empty()
                || v.starts_with('-')
                || v.chars().any(|c| {
                    c.is_whitespace() || c.is_control() || [';', '"', '\'', '`'].contains(&c)
                })
            {
                return Err(format!("{} needs one value without spaces, quotes or command separators. Complex input belongs in Console.",field.flag));
            }
            line.push(' ');
            line.push_str(v);
        }
    }
    command::validate_line(&line)?;
    CommandSpec::console(line)
}
pub fn filtered<'a>(
    entries: &'a [Entry],
    query: &str,
    group: &str,
    favourites: Option<&[String]>,
) -> Vec<&'a Entry> {
    let words = query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    entries
        .iter()
        .filter(|e| {
            (group.is_empty() || e.name.split_whitespace().next() == Some(group))
                && favourites.is_none_or(|v| v.contains(&e.name))
                && {
                    let text = format!("{} {}", e.name, e.description).to_lowercase();
                    words.iter().all(|w| text.contains(w))
                }
        })
        .collect()
}
pub fn toggle_favourite(names: &mut Vec<String>, name: &str) -> Result<(), String> {
    if !valid_name(name) {
        return Err("Only command names can be saved as favourites".into());
    }
    names.retain(|s| valid_name(s));
    if names.iter().any(|s| s == name) {
        names.retain(|s| s != name);
    } else if names.len() < 200 {
        names.push(name.into());
    }
    Ok(())
}
