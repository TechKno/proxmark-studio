use crate::{
    command::{CommandSpec, Kind},
    credential::Target,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
pub enum Family {
    #[default]
    Awid26,
    Awid34,
    Awid37,
    Awid50,
    IoProx,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Credential {
    pub name: String,
    pub family: Family,
    pub facility: u32,
    pub number: u32,
    pub version: u8,
    pub target: Target,
}
impl Default for Credential {
    fn default() -> Self {
        Self {
            name: String::new(),
            family: Family::Awid26,
            facility: 0,
            number: 0,
            version: 1,
            target: Target::T5577,
        }
    }
}
impl Credential {
    pub fn limits(&self) -> (u32, u32) {
        match self.family {
            Family::Awid26 | Family::IoProx => (255, 65535),
            Family::Awid34 => (255, 16777215),
            Family::Awid37 => (8191, 262143),
            Family::Awid50 => (65535, u32::MAX),
        }
    }
    pub fn write(&self) -> Result<CommandSpec, String> {
        let (fc, cn) = self.limits();
        if self.facility > fc || self.number > cn {
            return Err(format!(
                "Facility/card maximum is {fc}/{cn}; refusing CLI truncation"
            ));
        }
        let flag = match self.target {
            Target::T5577 => "",
            Target::Q5 => " --q5",
            Target::Em4305 => " --em",
        };
        CommandSpec::new(
            if self.family == Family::IoProx {
                format!(
                    "lf io clone --vn {} --fc {} --cn {}{flag}",
                    self.version, self.facility, self.number
                )
            } else {
                format!(
                    "lf awid clone --fmt {} --fc {} --cn {}{flag}",
                    self.bits(),
                    self.facility,
                    self.number
                )
            },
            format!("Programme {:?}", self.family),
            false,
            Kind::Write,
        )
    }
    pub fn bits(&self) -> u8 {
        match self.family {
            Family::Awid26 => 26,
            Family::Awid34 => 34,
            Family::Awid37 => 37,
            Family::Awid50 => 50,
            Family::IoProx => 64,
        }
    }
    pub fn read(&self) -> CommandSpec {
        CommandSpec::simple(
            if self.family == Family::IoProx {
                "lf io reader"
            } else {
                "lf awid reader"
            },
            Kind::Detect,
        )
    }
    pub fn matches(&self, output: &str) -> bool {
        let text = output
            .lines()
            .map(crate::parse::clean)
            .collect::<Vec<_>>()
            .join("\n");
        if self.family == Family::IoProx {
            let re = regex::Regex::new(
                r"(?i)IO Prox - XSF\((\d+)\)([0-9a-f]{2}):(\d+), Raw: [0-9a-f]+\s+\(\s*ok\s*\)",
            )
            .unwrap();
            let all = re.captures_iter(&text).collect::<Vec<_>>();
            all.len() == 1
                && all[0][1].parse::<u8>() == Ok(self.version)
                && u32::from_str_radix(&all[0][2], 16) == Ok(self.facility)
                && all[0][3].parse::<u32>() == Ok(self.number)
        } else {
            let re = regex::Regex::new(
                r"AWID - len:\s*(\d+)\s+FC:\s*(\d+)\s+Card:\s*(\d+)\s+- Wiegand:",
            )
            .unwrap();
            let all = re.captures_iter(&text).collect::<Vec<_>>();
            all.len() == 1
                && all[0][1].parse::<u8>() == Ok(self.bits())
                && all[0][2].parse::<u32>() == Ok(self.facility)
                && all[0][3].parse::<u32>() == Ok(self.number)
        }
    }
}
