//! Configurable organisation profiles and private credential registers.
//! This is a local credential register, not a connection to the access controller.
use crate::{
    basic::{Card, Content},
    em410x::ReaderFormat,
    runner::Outcome,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub organisation: String,
    pub site: String,
    pub format: ReaderFormat,
    pub facility: u32,
    pub first_number: u16,
    pub last_number: u16,
    pub confirmed: bool,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            organisation: String::new(),
            site: String::new(),
            format: ReaderFormat::Wiegand26,
            facility: 0,
            first_number: 1,
            last_number: u16::MAX,
            confirmed: false,
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        for name in [&self.organisation, &self.site] {
            if name.trim().is_empty() || name.len() > 200 || name.chars().any(char::is_control) {
                return Err("Enter an organisation and site name of 1–200 characters".into());
            }
        }
        let max = match self.format {
            ReaderFormat::Wiegand26 => 255,
            ReaderFormat::Wiegand34 => 65535,
            _ => {
                return Err(
                    "Organisation mode supports EM410x with Wiegand 26 or 34 reader output".into(),
                )
            }
        };
        if self.facility > max {
            return Err(format!(
                "Facility must be between 0 and {max} for this format"
            ));
        }
        if self.first_number == 0 || self.first_number > self.last_number {
            return Err("Choose a card-number range within 1–65535".into());
        }
        if !self.confirmed {
            return Err("Confirm the reader format and facility code before saving".into());
        }
        Ok(())
    }
    pub fn contains(&self, number: u16) -> bool {
        number >= self.first_number && number <= self.last_number
    }
    pub fn summary(&self) -> String {
        format!(
            "EM410x · {} · facility {} · new numbers {}–{}",
            self.format.name(),
            self.facility,
            self.first_number,
            self.last_number
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    /// Stable local key. Imported users initially use their original Controller number.
    pub number: u32,
    pub name: String,
    #[serde(default, alias = "na1_number")]
    pub controller_number: Option<u32>,
}
impl User {
    pub fn reference(&self) -> String {
        self.controller_number
            .map(|n| format!("Controller user {n}"))
            .unwrap_or_else(|| "Controller number not set".into())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub at: u64,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Imported,
    Reserved,
    Scanned,
    Writing,
    Verified,
    Uncertain,
}
impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Imported => "Imported record",
            Self::Reserved => "Number reserved",
            Self::Scanned => "Scanned and linked",
            Self::Writing => "Write in progress",
            Self::Verified => "Fob written and verified",
            Self::Uncertain => "Write not verified; number retained",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CardRecord {
    pub number: u16,
    pub user_no: u32,
    pub state: State,
    #[serde(default)]
    pub observed_ids: Vec<String>,
    #[serde(default)]
    pub planned_id: Option<String>,
    #[serde(default)]
    pub needs_enrolment: bool,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub authorized: Option<bool>,
    #[serde(default)]
    pub valid_from: String,
    #[serde(default)]
    pub expires_on: String,
    #[serde(default)]
    pub issue_number: u32,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub clone_write: bool,
    #[serde(default)]
    pub replaces: Option<u16>,
}
impl CardRecord {
    pub fn imported(number: u16, user_no: u32) -> Self {
        Self {
            number,
            user_no,
            state: State::Imported,
            observed_ids: vec![],
            planned_id: None,
            needs_enrolment: false,
            enabled: None,
            authorized: None,
            valid_from: String::new(),
            expires_on: String::new(),
            issue_number: 0,
            deleted: false,
            notes: String::new(),
            clone_write: false,
            replaces: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub schema: u32,
    pub profile: Profile,
    // Read-only compatibility fields. Never written into schema 3 registers.
    #[serde(skip_serializing)]
    pub site: String,
    #[serde(skip_serializing)]
    pub facility: u32,
    #[serde(skip_serializing)]
    pub encoding: String,
    pub simulated: bool,
    pub source: String,
    pub users: Vec<User>,
    pub cards: Vec<CardRecord>,
    pub seen_numbers: Vec<u16>,
    pub changes: Vec<Change>,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            schema: 3,
            profile: Profile::default(),
            site: String::new(),
            facility: 0,
            encoding: String::new(),
            simulated: false,
            source: String::new(),
            users: vec![],
            cards: vec![],
            seen_numbers: vec![],
            changes: vec![],
        }
    }
}
pub fn new_card(profile: &Profile, number: u16, simulated: bool) -> Result<Card, String> {
    profile.validate()?;
    if !profile.contains(number) {
        return Err("Choose a card number within the organisation's allocation range".into());
    }
    Ok(Card {
        content: Content::Em410xMapped {
            id: format!(
                "{:010X}",
                (u64::from(profile.facility) << 16) | u64::from(number)
            ),
            format: profile.format,
        },
        captured_at: crate::basic::now(),
        simulated,
    })
}
pub fn identity(profile: &Profile, card: &Card, simulated: bool) -> Result<(u16, String), String> {
    profile.validate()?;
    if card.simulated != simulated {
        return Err("Demo and physical credentials cannot be mixed".into());
    }
    let (id, _) = card
        .em_identity()
        .ok_or("access system requires an EM410x fob, not HID or an HF card")?;
    let d = crate::em410x::decode(id, profile.format)?;
    if d.facility != profile.facility {
        return Err(format!(
            "Facility {} does not match this organisation's facility {}",
            d.facility, profile.facility
        ));
    }
    if d.number == 0 {
        return Err("access system card number zero is not supported".into());
    }
    Ok((d.number as u16, d.id))
}
impl Store {
    /// Upgrade old registers before validation; older releases refuse schema 3.
    pub fn upgrade(&mut self) -> Result<bool, String> {
        match self.schema {
            1 | 2 => {
                if self.schema == 1 {
                    for user in &mut self.users {
                        user.controller_number = Some(user.number);
                    }
                }
                if self.encoding != "EM410x/Wiegand26" {
                    return Err("Unsupported legacy register encoding".into());
                }
                self.profile = Profile {
                    organisation: self.site.clone(),
                    site: self.site.clone(),
                    facility: self.facility,
                    confirmed: true,
                    ..Profile::default()
                };
                self.schema = 3;
                self.validate()?;
                Ok(true)
            }
            3 => {
                self.validate()?;
                Ok(false)
            }
            _ => Err(
                "Unsupported access system register version; use a matching Studio release".into(),
            ),
        }
    }
    pub fn update_profile(&mut self, profile: Profile) -> Result<(), String> {
        profile.validate()?;
        if (profile.format != self.profile.format || profile.facility != self.profile.facility)
            && (!self.cards.is_empty() || !self.seen_numbers.is_empty())
        {
            return Err("Create a separate organisation register to change encoding or facility after fobs have been recorded".into());
        }
        if profile != self.profile {
            self.record("Updated organisation profile.".into())?;
            self.profile = profile;
        }
        Ok(())
    }
    fn check_name(name: &str) -> Result<String, String> {
        if name.trim().is_empty() || name.len() > 200 || name.chars().any(char::is_control) {
            return Err("Enter a name of 1–200 characters without control characters".into());
        }
        Ok(name.trim().into())
    }
    fn check_user_number(&self, key: Option<u32>, number: Option<u32>) -> Result<(), String> {
        if number == Some(0)
            || number.is_some_and(|n| {
                self.users
                    .iter()
                    .any(|u| Some(u.number) != key && u.controller_number == Some(n))
            })
        {
            return Err("That Controller user number is zero or already in use".into());
        }
        Ok(())
    }
    fn record(&mut self, detail: String) -> Result<(), String> {
        if self.changes.len() >= 100000 {
            return Err(
                "Change history is full; export and archive the register before further edits"
                    .into(),
            );
        }
        self.changes.push(Change {
            at: crate::basic::now(),
            detail,
        });
        Ok(())
    }
    pub fn add_user(&mut self, name: &str, controller_number: Option<u32>) -> Result<u32, String> {
        let name = Self::check_name(name)?;
        self.check_user_number(None, controller_number)?;
        if self.users.len() >= 10000 {
            return Err("User limit reached".into());
        }
        let number = self
            .users
            .iter()
            .map(|u| u.number)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("User identifiers exhausted")?;
        self.record(format!(
            "Added {name} (local user {number}; Controller number {}).",
            controller_number
                .map(|n| n.to_string())
                .unwrap_or_else(|| "not set".into())
        ))?;
        self.users.push(User {
            number,
            name,
            controller_number,
        });
        self.users.sort_by_key(|u| u.name.to_lowercase());
        Ok(number)
    }
    pub fn edit_user(
        &mut self,
        number: u32,
        name: &str,
        controller_number: Option<u32>,
    ) -> Result<(), String> {
        let name = Self::check_name(name)?;
        self.check_user_number(Some(number), controller_number)?;
        let old = self.user(number).ok_or("User no longer exists")?.clone();
        if old.name == name && old.controller_number == controller_number {
            return Ok(());
        }
        if self
            .cards
            .iter()
            .any(|c| c.user_no == number && c.state == State::Writing)
        {
            return Err("Finish the current fob write before editing its user".into());
        }
        self.record(format!(
            "Edited {} ({}) to {} ({}).",
            old.name,
            old.reference(),
            name,
            controller_number
                .map(|n| format!("Controller user {n}"))
                .unwrap_or_else(|| "Controller number not set".into())
        ))?;
        let user = self.users.iter_mut().find(|u| u.number == number).unwrap();
        user.name = name;
        user.controller_number = controller_number;
        self.users.sort_by_key(|u| u.name.to_lowercase());
        Ok(())
    }
    pub fn reassign(
        &mut self,
        number: u16,
        expected_owner: u32,
        new_owner: u32,
    ) -> Result<(), String> {
        let target = self
            .user(new_owner)
            .ok_or("Choose an existing recipient")?
            .name
            .clone();
        let card = self.card(number).ok_or("Card no longer exists")?;
        if card.user_no != expected_owner {
            return Err("The owner changed; reopen the reassignment review".into());
        }
        if card.deleted {
            return Err("This fob was deleted from the active register".into());
        }
        if card.state == State::Writing {
            return Err("Finish the current fob write before reassigning it".into());
        }
        if new_owner == expected_owner {
            return Err("Choose a different recipient".into());
        }
        let previous = self
            .user(expected_owner)
            .ok_or("Previous owner is missing")?
            .name
            .clone();
        self.record(format!(
            "Card {number}: {previous} → {target}. access system update required."
        ))?;
        let card = self.cards.iter_mut().find(|c| c.number == number).unwrap();
        card.user_no = new_owner;
        card.needs_enrolment = true;
        Ok(())
    }
    pub fn edit_fob(&mut self, number: u16, notes: &str) -> Result<(), String> {
        if notes.len() > 2000
            || notes
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err("Fob notes must be at most 2000 characters".into());
        }
        let card = self.card(number).ok_or("Fob no longer exists")?;
        if card.deleted || card.state == State::Writing {
            return Err("Deleted fobs or fobs being written cannot be edited".into());
        }
        if card.notes == notes {
            return Ok(());
        }
        self.record(format!("Edited notes for card {number}."))?;
        self.cards
            .iter_mut()
            .find(|c| c.number == number)
            .unwrap()
            .notes = notes.into();
        Ok(())
    }
    pub fn delete_fob(&mut self, number: u16, expected_owner: u32) -> Result<(), String> {
        let card = self.card(number).ok_or("Fob no longer exists")?;
        if card.deleted || card.user_no != expected_owner || card.state == State::Writing {
            return Err("Fob changed or is being written; reopen its details".into());
        }
        self.record(format!("Deleted card {number} from the active register. Number retained; access system revocation required."))?;
        let card = self.cards.iter_mut().find(|c| c.number == number).unwrap();
        card.deleted = true;
        card.needs_enrolment = true;
        Ok(())
    }
    pub fn begin_clone(&mut self, number: u16, user_no: u32, id: &str) -> Result<(), String> {
        let card = self.card(number).ok_or("Fob no longer exists")?;
        if card.deleted
            || card.user_no != user_no
            || card.state == State::Writing
            || !card.observed_ids.iter().any(|observed| observed == id)
            || (matches!(card.state, State::Reserved | State::Uncertain) && !card.clone_write)
        {
            return Err("Read the assigned original fob before cloning it".into());
        }
        let decoded = crate::em410x::decode(id, self.profile.format)?;
        if decoded.facility != self.profile.facility || decoded.number != u64::from(number) {
            return Err("Original fob does not match the selected card".into());
        }
        self.record(format!(
            "Started cloning card {number}; destination verification pending."
        ))?;
        let card = self.cards.iter_mut().find(|c| c.number == number).unwrap();
        card.planned_id = Some(id.into());
        card.state = State::Writing;
        card.clone_write = true;
        Ok(())
    }
    pub fn change_number(
        &mut self,
        old_number: u16,
        expected_owner: u32,
        new_number: u16,
    ) -> Result<(), String> {
        let old = self.card(old_number).ok_or("Fob no longer exists")?;
        if old.deleted
            || old.user_no != expected_owner
            || old.state == State::Writing
            || old.state == State::Reserved
            || (old.state == State::Uncertain && !old.clone_write)
        {
            return Err("Choose an existing assigned fob that is not awaiting a write".into());
        }
        if !self.profile.contains(new_number)
            || self.card(new_number).is_some()
            || self.seen_numbers.contains(&new_number)
        {
            return Err("Choose an unused card number from 1 to 65535; deleted and observed numbers remain reserved".into());
        }
        let mut replacement = CardRecord::imported(new_number, expected_owner);
        replacement.notes = old.notes.clone();
        replacement.state = State::Reserved;
        replacement.needs_enrolment = true;
        replacement.planned_id = Some(
            new_card(&self.profile, new_number, self.simulated)?
                .em_identity()
                .unwrap()
                .0
                .into(),
        );
        replacement.replaces = Some(old_number);
        self.record(format!(
            "Reserved card {new_number} to replace {old_number}; rewrite and verification pending."
        ))?;
        self.cards.push(replacement);
        Ok(())
    }
    pub fn user(&self, number: u32) -> Option<&User> {
        self.users.iter().find(|u| u.number == number)
    }
    pub fn card(&self, number: u16) -> Option<&CardRecord> {
        self.cards.iter().find(|c| c.number == number)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 3 {
            return Err("Unsupported organisation register version".into());
        }
        self.profile.validate()?;
        if self.users.len() > 10000 || self.cards.len() > 65535 || self.source.len() > 4096 {
            return Err("Register exceeds supported limits".into());
        }
        let mut users = HashSet::new();
        let mut controller_numbers = HashSet::new();
        for u in &self.users {
            if !users.insert(u.number)
                || u.name.trim().is_empty()
                || u.name.len() > 200
                || u.name.chars().any(char::is_control)
            {
                return Err("Duplicate user number or invalid user name".into());
            }
            if u.controller_number
                .is_some_and(|n| n == 0 || !controller_numbers.insert(n))
            {
                return Err("Duplicate or zero Controller user number".into());
            }
        }
        if self.changes.len() > 100000 || self.changes.iter().any(|c| c.detail.len() > 1500) {
            return Err("Change history exceeds supported limits".into());
        }
        if self.seen_numbers.len() > 65535 || self.seen_numbers.contains(&0) {
            return Err("Invalid observed card numbers".into());
        }
        let mut numbers = HashSet::new();
        for c in &self.cards {
            if c.number == 0 || !numbers.insert(c.number) || !users.contains(&c.user_no) {
                return Err("Duplicate card number or missing assigned user".into());
            }
            if c.replaces
                .is_some_and(|n| n == c.number || self.card(n).is_none())
            {
                return Err("Replacement references an invalid previous fob".into());
            }
            if c.observed_ids.len() > 64
                || c.valid_from.len() > 100
                || c.expires_on.len() > 100
                || c.notes.len() > 2000
            {
                return Err("Card record exceeds supported limits".into());
            }
            for id in c.observed_ids.iter().chain(c.planned_id.iter()) {
                let d = crate::em410x::decode(id, self.profile.format)?;
                if d.facility != self.profile.facility || d.number != u64::from(c.number) {
                    return Err("Full fob ID does not match its assigned Controller number".into());
                }
            }
            if matches!(
                c.state,
                State::Reserved | State::Writing | State::Verified | State::Uncertain
            ) && c.planned_id.is_none()
            {
                return Err("An issued credential must retain its planned full ID".into());
            }
        }
        Ok(())
    }
    pub fn reserve(&mut self, user_no: u32) -> Result<u16, String> {
        self.user(user_no)
            .ok_or("Choose an existing Controller user")?;
        let used: HashSet<_> = self
            .cards
            .iter()
            .map(|c| c.number)
            .chain(self.seen_numbers.iter().copied())
            .collect();
        let first = u32::from(self.profile.first_number);
        let last = u32::from(self.profile.last_number);
        let start = used
            .iter()
            .copied()
            .filter(|n| self.profile.contains(*n))
            .max()
            .map(|n| u32::from(n) + 1)
            .unwrap_or(first);
        let number = (start..=last)
            .chain(first..start.min(last + 1))
            .map(|n| n as u16)
            .find(|n| !used.contains(n))
            .ok_or("All numbers in this organisation's allocation range are reserved")?;
        let mut c = CardRecord::imported(number, user_no);
        c.state = State::Reserved;
        c.planned_id = Some(
            new_card(&self.profile, number, self.simulated)?
                .em_identity()
                .unwrap()
                .0
                .into(),
        );
        c.needs_enrolment = true;
        self.cards.push(c);
        Ok(number)
    }
    pub fn observe_scan(&mut self, card: &Card) -> Result<Option<u32>, String> {
        let (number, _) = identity(&self.profile, card, self.simulated)?;
        if !self.seen_numbers.contains(&number) {
            self.seen_numbers.push(number);
        }
        if self.card(number).is_some() {
            self.link_scan(card, None).map(Some)
        } else {
            Ok(None)
        }
    }
    pub fn link_scan(&mut self, card: &Card, chosen_user: Option<u32>) -> Result<u32, String> {
        let (number, id) = identity(&self.profile, card, self.simulated)?;
        if let Some(c) = self.cards.iter_mut().find(|c| c.number == number) {
            if c.deleted {
                return Err(format!("Card {number} is deleted from the active register; its number remains reserved. Revoke it in access system separately."));
            }
            if chosen_user.is_some_and(|u| u != c.user_no) {
                return Err(
                    "This number is already assigned to another user; it was not reassigned".into(),
                );
            }
            if !c.observed_ids.contains(&id) {
                if c.observed_ids.len() >= 64 {
                    return Err("Too many observed full IDs for this number".into());
                }
                c.observed_ids.push(id);
            }
            return Ok(c.user_no);
        }
        let user_no = chosen_user
            .ok_or("No user matches this fob. Select its user and explicitly link it")?;
        self.user(user_no)
            .ok_or("Choose an existing Controller user")?;
        let mut c = CardRecord::imported(number, user_no);
        c.state = State::Scanned;
        c.observed_ids.push(id);
        c.needs_enrolment = true;
        self.cards.push(c);
        Ok(user_no)
    }
    pub fn begin_write(&mut self, number: u16, user_no: u32, id: &str) -> Result<(), String> {
        if let Some(old_number) = self.card(number).and_then(|c| c.replaces) {
            let old = self
                .card(old_number)
                .ok_or("Original fob record is missing")?;
            if old.deleted || old.user_no != user_no || old.state == State::Writing {
                return Err("Original fob assignment changed; cancel this replacement and review its holder".into());
            }
        }
        let c = self
            .cards
            .iter_mut()
            .find(|c| c.number == number)
            .ok_or("Number has not been reserved")?;
        if c.user_no != user_no
            || c.deleted
            || c.planned_id.as_deref() != Some(id)
            || !matches!(c.state, State::Reserved | State::Uncertain)
        {
            return Err(
                "The reserved user, full ID or write state changed; prepare the destination again"
                    .into(),
            );
        }
        c.state = State::Writing;
        Ok(())
    }
    pub fn finish_write(&mut self, number: u16, outcome: &Outcome) -> Result<(), String> {
        let c = self
            .cards
            .iter_mut()
            .find(|c| c.number == number)
            .ok_or("Reservation is missing")?;
        if c.state != State::Writing {
            return Err("No recorded write is awaiting verification".into());
        }
        c.state = if *outcome == Outcome::Verified {
            State::Verified
        } else {
            State::Uncertain
        };
        if c.state == State::Verified {
            let id = c.planned_id.clone().ok_or("Planned ID is missing")?;
            if !c.observed_ids.contains(&id) {
                c.observed_ids.push(id);
            }
        }
        let replacement = if c.state == State::Verified && !c.clone_write {
            c.replaces
        } else {
            None
        };
        if let Some(old_number) = replacement {
            self.record(format!("Card {old_number} replaced by {number} after verified write. Old number retained; update access system separately."))?;
            let old = self
                .cards
                .iter_mut()
                .find(|c| c.number == old_number)
                .ok_or("Original fob record is missing")?;
            old.deleted = true;
            old.needs_enrolment = true;
        }
        Ok(())
    }
    pub fn export_csv(&self) -> String {
        let mut out =
            "User No,User Name,Facility,Card Number,Full EM410x ID,Status,Needs access system update,Local User ID,Deleted,Notes\r\n"
                .to_owned();
        for c in &self.cards {
            let name = self.user(c.user_no).map(|u| u.name.as_str()).unwrap_or("");
            let name = if name.trim_start().starts_with(['=', '+', '-', '@']) {
                format!("'{name}")
            } else {
                name.into()
            };
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{}\r\n",
                self.user(c.user_no)
                    .and_then(|u| u.controller_number)
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                crate::provision::csv_cell(&name),
                self.profile.facility,
                c.number,
                c.planned_id
                    .as_deref()
                    .or_else(|| c.observed_ids.first().map(String::as_str))
                    .unwrap_or(""),
                crate::provision::csv_cell(c.state.label()),
                c.needs_enrolment,
                c.user_no,
                c.deleted,
                crate::provision::csv_cell(&c.notes)
            ));
        }
        out
    }
    pub fn import_csv(&mut self, input: &str) -> Result<usize, String> {
        let rows = crate::provision::parse_csv(input)?;
        let expected = [
            "user number",
            "user name",
            "card number",
            "full em410x id",
            "notes",
        ];
        let headers = rows.first().ok_or("CSV is empty")?;
        if headers
            .iter()
            .map(|s| s.trim().to_lowercase())
            .collect::<Vec<_>>()
            != expected
        {
            return Err("Use the CSV template headers: User Number,User Name,Card Number,Full EM410x ID,Notes".into());
        }
        let mut next = self.clone();
        let mut count = 0;
        for (index, row) in rows.iter().enumerate().skip(1) {
            let result = (|| -> Result<(), String> {
                if row.len() != 5 {
                    return Err("Expected five CSV columns".into());
                }
                let external = if row[0].trim().is_empty() {
                    None
                } else {
                    Some(
                        row[0]
                            .trim()
                            .parse::<u32>()
                            .map_err(|_| "Invalid user number")?,
                    )
                };
                let name = Self::check_name(&row[1])?;
                let matches = next
                    .users
                    .iter()
                    .filter(|u| {
                        if let Some(n) = external {
                            u.controller_number == Some(n)
                        } else {
                            u.controller_number.is_none() && u.name == name
                        }
                    })
                    .map(|u| u.number)
                    .collect::<Vec<_>>();
                let user = match matches.as_slice() {
                    [] => next.add_user(&name, external)?,
                    [key] => {
                        if next.user(*key).unwrap().name != name {
                            return Err("User number already belongs to a different name".into());
                        }
                        *key
                    }
                    _ => return Err("Ambiguous user name; provide a controller user number".into()),
                };
                if row[2].trim().is_empty() {
                    if !row[3].trim().is_empty() || !row[4].is_empty() {
                        return Err("Fob ID/notes require a card number".into());
                    }
                    return Ok(());
                }
                let number = row[2]
                    .trim()
                    .parse::<u16>()
                    .map_err(|_| "Card number must be 1–65535")?;
                if number == 0 || next.card(number).is_some() || next.seen_numbers.contains(&number)
                {
                    return Err(
                        "Card number is zero or already assigned, reserved or observed".into(),
                    );
                }
                let mut record = CardRecord::imported(number, user);
                record.notes = row[4].clone();
                if !row[3].trim().is_empty() {
                    let card = Card {
                        content: Content::Em410x(row[3].trim().into()),
                        captured_at: crate::basic::now(),
                        simulated: next.simulated,
                    };
                    let (decoded, id) = identity(&next.profile, &card, next.simulated)?;
                    if decoded != number {
                        return Err("Full ID does not match card number".into());
                    }
                    record.observed_ids.push(id);
                }
                next.cards.push(record);
                count += 1;
                Ok(())
            })();
            result.map_err(|e| format!("CSV row {}: {e}", index + 1))?;
        }
        next.record(format!(
            "Imported {} CSV rows containing {count} fobs.",
            rows.len() - 1
        ))?;
        next.validate()?;
        *self = next;
        Ok(count)
    }
}
pub fn save_roster(path: &Path, data: &Store) -> Result<(), String> {
    data.validate()?;
    let bytes = serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Register exceeds the 32 MiB file limit".into());
    }
    crate::storage::write_private(path, &bytes)
}
pub struct Repository {
    pub data: Store,
    pub path: PathBuf,
    _lock: File,
}
impl Repository {
    pub fn create(path: &Path, data: Store) -> Result<Self, String> {
        use std::io::Write;
        data.validate()?;
        let parent = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|_| "Register is already open in another window")?;
        let bytes = serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("Register exceeds the file limit".into());
        }
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist_noclobber(path).map_err(|e| {
            format!("Choose a new register file; existing files are never overwritten: {e}")
        })?;
        Ok(Self {
            data,
            path: path.into(),
            _lock: lock,
        })
    }
    pub fn open(path: &Path, simulated: bool) -> Result<Self, String> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .map_err(|e| format!("Cannot open the access system register lock: {e}"))?;
        lock.try_lock().map_err(|_| {
            "The access system register is already open in another Studio window".to_owned()
        })?;
        let bytes = crate::storage::read_bounded(path, 32 * 1024 * 1024)?;
        let mut data: Store = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid access system register: {e}"))?;
        let upgraded = data.upgrade()?;
        if data.simulated != simulated {
            return Err("Demo and physical registers cannot be mixed".into());
        }
        if upgraded {
            use std::io::Write;
            let mut backup = tempfile::Builder::new()
                .prefix("organisation-before-0.9.0-")
                .suffix(".json")
                .tempfile_in(path.parent().unwrap_or(Path::new(".")))
                .map_err(|e| e.to_string())?;
            backup.write_all(&bytes).map_err(|e| e.to_string())?;
            backup.as_file().sync_all().map_err(|e| e.to_string())?;
            // Never overwrite a previous backup. A unique snapshot is retained before migration.
            backup.keep().map_err(|e| e.to_string())?;
            save_roster(path, &data)?;
        }
        let mut repo = Self {
            data,
            path: path.into(),
            _lock: lock,
        };
        if repo.data.cards.iter().any(|c| c.state == State::Writing) {
            repo.update(|s| {
                for c in &mut s.cards {
                    if c.state == State::Writing {
                        c.state = State::Uncertain;
                    }
                }
                Ok(())
            })?;
        }
        Ok(repo)
    }
    pub fn update<T>(
        &mut self,
        f: impl FnOnce(&mut Store) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut next = self.data.clone();
        let value = f(&mut next)?;
        save_roster(&self.path, &next)?;
        self.data = next;
        Ok(value)
    }
}
