use std::collections::BTreeMap;

pub const COMMANDS: &[&str] = &[
    "hf mf autopwn",
    "hf mf fchk",
    "hf mf cgetblk",
    "hf mf csetblk",
    "lf read",
    "data save",
    "data load",
    "data rawdemod",
    "hw ping",
    "hw status",
    "hw tune",
    "hw break",
    "hf mf eview",
    "hf mf esetblk",
    "hf mf sim",
    "hw version",
    "wiegand encode",
    "wiegand decode",
    "hf mfdes lsfiles",
    "hf mfdes getappnames",
    "hf mfdes getkeyversions",
    "hf mfdes getfileisoids",
    "hf mfdes chkeysettings",
    "lf awid reader",
    "lf awid clone",
    "lf io reader",
    "lf io clone",
    "hf mfdes createapp",
    "hf mfdes deleteapp",
    "hf mfdes createfile",
    "hf mfdes createvaluefile",
    "hf mfdes createrecordfile",
    "hf mfdes deletefile",
    "hf mfdes chfilesettings",
    "hf mfdes changekey",
    "hf mfdes getkeysettings",
    "hf mfdes value",
    "hf mfdes clearrecfile",
    "hf mf rdbl",
    "hf mf wrbl",
    "lf search",
    "lf t55xx detect",
    "hf search",
    "hf 14a info",
    "lf em 410x reader",
    "lf em 410x clone",
    "hf mfu info",
    "hf mfu rdbl",
    "hf mfu wrbl",
    "hf mf info",
    "hf mfdes info",
    "hf mfdes getversion",
    "hf mfdes getaids",
    "hf mfdes getfileids",
    "hf mfdes getfilesettings",
    "hf mfdes auth",
    "hf mfdes read",
    "hf mfdes write",
    "lf hid reader",
    "lf hid clone",
];
#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub help: BTreeMap<String, String>,
}
impl Capabilities {
    pub fn record(&mut self, command: &str, output: &str) {
        let lower = output.to_lowercase();
        if lower.contains("usage:") && lower.contains(command) && !lower.contains("unknown command")
        {
            self.help.insert(command.into(), output.into());
        }
    }
    pub fn has(&self, command: &str) -> bool {
        self.help.contains_key(command)
    }
    pub fn require(&self, command: &str, flags: &[&str]) -> Result<(), String> {
        let h = self.help.get(command).ok_or_else(|| {
            format!("{command} is unconfirmed. Inspect the CLI in Connection first.")
        })?;
        for flag in flags {
            let pattern = format!(r"(?:^|[\s,|\[]){}(?:$|[\s,=<|\]])", regex::escape(flag));
            if !regex::Regex::new(&pattern)
                .expect("escaped flag pattern")
                .is_match(h)
            {
                return Err(format!(
                    "Configured CLI help does not advertise {flag} for {command}"
                ));
            }
        }
        Ok(())
    }
    pub fn validate(&self, command: &str) -> Result<(), String> {
        let base = COMMANDS
            .iter()
            .filter(|c| {
                command.starts_with(**c)
                    && command.as_bytes().get(c.len()).is_none_or(|b| *b == b' ')
            })
            .max_by_key(|c| c.len())
            .ok_or("No guided adapter for command")?;
        // All generated flags must be advertised by this executable, including channel values.
        let tokens: Vec<_> = command.split_whitespace().collect();
        let flags: Vec<_> = tokens
            .iter()
            .copied()
            .filter(|s| s.starts_with('-'))
            .collect();
        self.require(base, &flags)?;
        if command.contains("--schann") {
            let channel = tokens
                .iter()
                .position(|t| *t == "--schann")
                .and_then(|i| tokens.get(i + 1))
                .ok_or("Missing secure channel")?;
            if !self.help[*base].to_lowercase().contains(channel) {
                return Err("Configured CLI does not advertise this secure channel".into());
            }
        }
        Ok(())
    }
}
