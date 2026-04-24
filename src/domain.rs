use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use chrono::{Datelike, Duration, NaiveDate, Weekday};

pub const DEFAULT_SHIFT_SLOT_ID_MORNING: &str = "morning";
pub const DEFAULT_SHIFT_SLOT_ID_AFTERNOON: &str = "afternoon";
pub const DEFAULT_SHIFT_SLOT_ID_NIGHT: &str = "night";
pub const DEFAULT_SHIFT_SLOT_ID_DAY: &str = "day";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanningError {
    EmptyWorkerId,
    EmptyWorkerLastName,
    EmptyWorkerFirstName,
    EmptyJobRole,
    EmptyShiftSlotId,
    EmptyShiftSlotName,
    EmptyShiftSlotCode,
    EmptyTeamId,
    EmptyTeamName,
    InvalidMonth {
        month: u8,
    },
    InvalidDay {
        year: i32,
        month: u8,
        day: u8,
    },
    InvalidIsoDate {
        value: String,
    },
    InvalidClockTime {
        hour: u8,
        minute: u8,
    },
    InvalidGenerationDays {
        days: u32,
    },
    InvalidRotationCycle,
    DuplicateWorkerId {
        worker_id: String,
    },
    DuplicateShiftSlotId {
        shift_slot_id: String,
    },
    DuplicateShiftSlotCode {
        short_code: String,
    },
    DuplicateTeamId {
        team_id: String,
    },
    DuplicateTeamAnchorShift {
        shift_slot_id: String,
    },
    DuplicateWorkerAcrossTeams {
        worker_id: String,
    },
    RotationHasDuplicateShift {
        shift_slot_id: String,
    },
    UnknownWorker {
        worker_id: String,
    },
    UnknownTeam {
        team_id: String,
    },
    UnknownShiftSlot {
        shift_slot_id: String,
    },
    TeamAnchorShiftNotInRotation {
        team_id: String,
        shift_slot_id: String,
    },
    ActiveTeamsDoNotMatchRotationSlots {
        expected_teams: usize,
        actual_teams: usize,
    },
    TeamMissingLeader {
        team_id: String,
    },
    TeamHasMultipleLeaders {
        team_id: String,
    },
    TeamMissingOperator {
        team_id: String,
    },
    ManualOverrideMissingShift {
        worker_id: String,
        date: PlanningDate,
    },
}

impl fmt::Display for PlanningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyWorkerId => {
                write!(f, "L'identifiant interne du salarie ne peut pas etre vide.")
            }
            Self::EmptyWorkerLastName => write!(f, "Le nom du salarie ne peut pas etre vide."),
            Self::EmptyWorkerFirstName => write!(f, "Le prenom du salarie ne peut pas etre vide."),
            Self::EmptyJobRole => write!(f, "Le poste ne peut pas etre vide."),
            Self::EmptyShiftSlotId => write!(
                f,
                "L'identifiant de la plage horaire ne peut pas etre vide."
            ),
            Self::EmptyShiftSlotName => {
                write!(f, "Le nom de la plage horaire ne peut pas etre vide.")
            }
            Self::EmptyShiftSlotCode => write!(
                f,
                "Le code court de la plage horaire ne peut pas etre vide."
            ),
            Self::EmptyTeamId => write!(f, "L'identifiant de l'equipe ne peut pas etre vide."),
            Self::EmptyTeamName => write!(f, "Le nom de l'equipe ne peut pas etre vide."),
            Self::InvalidMonth { month } => write!(
                f,
                "Le mois {month} est invalide. La valeur attendue est comprise entre 1 et 12."
            ),
            Self::InvalidDay { year, month, day } => write!(
                f,
                "Le jour {day:02}/{month:02}/{year:04} est invalide pour le calendrier demande."
            ),
            Self::InvalidIsoDate { value } => {
                write!(
                    f,
                    "La date '{value}' n'est pas au format attendu AAAA-MM-JJ."
                )
            }
            Self::InvalidClockTime { hour, minute } => write!(
                f,
                "L'heure {hour:02}h{minute:02} est invalide. Les bornes attendues sont 00h00 a 23h59."
            ),
            Self::InvalidGenerationDays { days } => write!(
                f,
                "La duree de generation '{days}' est invalide. Je dois demander au moins un jour."
            ),
            Self::InvalidRotationCycle => write!(
                f,
                "Le cycle de rotation 3x8 est invalide. Je dois definir au moins une plage horaire sans doublon."
            ),
            Self::DuplicateWorkerId { worker_id } => write!(
                f,
                "L'identifiant salarie '{worker_id}' est duplique dans les donnees chargees."
            ),
            Self::DuplicateShiftSlotId { shift_slot_id } => write!(
                f,
                "L'identifiant de plage horaire '{shift_slot_id}' est duplique."
            ),
            Self::DuplicateShiftSlotCode { short_code } => write!(
                f,
                "Le code court de plage horaire '{short_code}' est duplique."
            ),
            Self::DuplicateTeamId { team_id } => {
                write!(f, "L'identifiant d'equipe '{team_id}' est duplique.")
            }
            Self::DuplicateTeamAnchorShift { shift_slot_id } => write!(
                f,
                "La plage '{shift_slot_id}' est deja utilisee comme poste de reference par une autre equipe active."
            ),
            Self::DuplicateWorkerAcrossTeams { worker_id } => write!(
                f,
                "Le salarie '{worker_id}' ne peut pas appartenir a plusieurs equipes actives."
            ),
            Self::RotationHasDuplicateShift { shift_slot_id } => write!(
                f,
                "La plage horaire '{shift_slot_id}' apparait plusieurs fois dans le cycle de rotation."
            ),
            Self::UnknownWorker { worker_id } => {
                write!(f, "Le salarie '{worker_id}' n'existe pas dans la base.")
            }
            Self::UnknownTeam { team_id } => {
                write!(f, "L'equipe '{team_id}' n'existe pas dans la base.")
            }
            Self::UnknownShiftSlot { shift_slot_id } => write!(
                f,
                "La plage horaire '{shift_slot_id}' n'existe pas dans la base."
            ),
            Self::TeamAnchorShiftNotInRotation {
                team_id,
                shift_slot_id,
            } => write!(
                f,
                "L'equipe '{team_id}' pointe vers la plage '{shift_slot_id}', absente du cycle de rotation."
            ),
            Self::ActiveTeamsDoNotMatchRotationSlots {
                expected_teams,
                actual_teams,
            } => write!(
                f,
                "La generation 3x8 attend {expected_teams} equipes actives, mais {actual_teams} equipes actives sont configurees."
            ),
            Self::TeamMissingLeader { team_id } => write!(
                f,
                "L'equipe '{team_id}' doit contenir exactement un chef d'equipe."
            ),
            Self::TeamHasMultipleLeaders { team_id } => write!(
                f,
                "L'equipe '{team_id}' contient plusieurs chefs d'equipe. Un seul est autorise."
            ),
            Self::TeamMissingOperator { team_id } => write!(
                f,
                "L'equipe '{team_id}' doit contenir au moins un operateur."
            ),
            Self::ManualOverrideMissingShift { worker_id, date } => write!(
                f,
                "La correction manuelle du salarie '{worker_id}' sur la date {date} doit pointer vers une plage horaire."
            ),
        }
    }
}

impl Error for PlanningError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClockTime {
    hour: u8,
    minute: u8,
}

impl ClockTime {
    pub fn new(hour: u8, minute: u8) -> Result<Self, PlanningError> {
        if hour > 23 || minute > 59 {
            return Err(PlanningError::InvalidClockTime { hour, minute });
        }

        Ok(Self { hour, minute })
    }

    pub const fn hour(self) -> u8 {
        self.hour
    }

    pub const fn minute(self) -> u8 {
        self.minute
    }

    pub const fn minutes_since_midnight(self) -> u16 {
        (self.hour as u16) * 60 + self.minute as u16
    }
}

impl fmt::Display for ClockTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}h{:02}", self.hour, self.minute)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlanningDate {
    year: i32,
    month: u8,
    day: u8,
}

impl PlanningDate {
    pub fn new(year: i32, month: u8, day: u8) -> Result<Self, PlanningError> {
        let max_day = Self::days_in_month(year, month)?;

        if day == 0 || day > max_day {
            return Err(PlanningError::InvalidDay { year, month, day });
        }

        Ok(Self { year, month, day })
    }

    pub fn parse_iso(value: &str) -> Result<Self, PlanningError> {
        let normalized = value.trim();

        let parsed = NaiveDate::parse_from_str(normalized, "%Y-%m-%d").map_err(|_| {
            PlanningError::InvalidIsoDate {
                value: normalized.to_owned(),
            }
        })?;

        Self::from_naive_date(parsed)
    }

    pub fn from_naive_date(value: NaiveDate) -> Result<Self, PlanningError> {
        Self::new(value.year(), value.month() as u8, value.day() as u8)
    }

    pub fn to_naive_date(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.year, self.month as u32, self.day as u32)
            .expect("PlanningDate garantit une date valide")
    }

    pub fn days_in_month(year: i32, month: u8) -> Result<u8, PlanningError> {
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if Self::is_leap_year(year) {
                    29
                } else {
                    28
                }
            }
            _ => return Err(PlanningError::InvalidMonth { month }),
        };

        Ok(days)
    }

    pub const fn is_leap_year(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    pub fn add_days(self, days: i64) -> Result<Self, PlanningError> {
        let date = self.to_naive_date() + Duration::days(days);
        Self::from_naive_date(date)
    }

    pub fn start_of_week_monday(self) -> Result<Self, PlanningError> {
        let date = self.to_naive_date();
        let days_from_monday = match date.weekday() {
            Weekday::Mon => 0,
            Weekday::Tue => 1,
            Weekday::Wed => 2,
            Weekday::Thu => 3,
            Weekday::Fri => 4,
            Weekday::Sat => 5,
            Weekday::Sun => 6,
        };
        Self::from_naive_date(date - Duration::days(days_from_monday))
    }

    pub fn signed_days_since(self, other: Self) -> i64 {
        (self.to_naive_date() - other.to_naive_date()).num_days()
    }

    pub const fn year(self) -> i32 {
        self.year
    }

    pub const fn month(self) -> u8 {
        self.month
    }

    pub const fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for PlanningDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkerId(String);

impl WorkerId {
    pub fn new(value: impl Into<String>) -> Result<Self, PlanningError> {
        let value = normalize_text(&value.into());

        if value.is_empty() {
            return Err(PlanningError::EmptyWorkerId);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobRole(String);

impl JobRole {
    pub const DEFAULT_LABELS: [&'static str; 4] = [
        "Operateur de production",
        "Operateur de salle blanche",
        "Chef d'equipes",
        "Autre",
    ];

    pub fn new(value: impl Into<String>) -> Result<Self, PlanningError> {
        let value = normalize_text(&value.into());

        if value.is_empty() {
            return Err(PlanningError::EmptyJobRole);
        }

        Ok(Self(value))
    }

    pub fn label(&self) -> &str {
        &self.0
    }

    pub fn default_roles() -> Vec<Self> {
        Self::DEFAULT_LABELS
            .iter()
            .map(|label| Self::new(*label).expect("les postes par defaut sont valides"))
            .collect()
    }

    pub fn from_legacy_storage_key(value: &str) -> Option<Self> {
        let label = match value {
            "operateur_production" => "Operateur de production",
            "operateur_salle_blanche" => "Operateur de salle blanche",
            "chef_d_equipes" => "Chef d'equipes",
            "autre" => "Autre",
            _ => return None,
        };

        Some(Self(label.to_owned()))
    }

    pub fn from_storage_value(value: &str) -> Result<Self, PlanningError> {
        if let Some(role) = Self::from_legacy_storage_key(value) {
            return Ok(role);
        }

        Self::new(value)
    }
}

impl fmt::Display for JobRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShiftVisualStyle {
    NightBlue,
    AfternoonRed,
    MorningYellow,
    DayBeige,
    NeutralGray,
}

impl ShiftVisualStyle {
    pub const ALL: [Self; 5] = [
        Self::NightBlue,
        Self::AfternoonRed,
        Self::MorningYellow,
        Self::DayBeige,
        Self::NeutralGray,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::NightBlue => "Bleu nuit",
            Self::AfternoonRed => "Rouge apres-midi",
            Self::MorningYellow => "Jaune matin",
            Self::DayBeige => "Beige journee",
            Self::NeutralGray => "Gris neutre",
        }
    }

    pub const fn token(self) -> &'static str {
        match self {
            Self::NightBlue => "night-blue",
            Self::AfternoonRed => "afternoon-red",
            Self::MorningYellow => "morning-yellow",
            Self::DayBeige => "day-beige",
            Self::NeutralGray => "neutral-gray",
        }
    }

    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::NightBlue => "night_blue",
            Self::AfternoonRed => "afternoon_red",
            Self::MorningYellow => "morning_yellow",
            Self::DayBeige => "day_beige",
            Self::NeutralGray => "neutral_gray",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "night_blue" => Some(Self::NightBlue),
            "afternoon_red" => Some(Self::AfternoonRed),
            "morning_yellow" => Some(Self::MorningYellow),
            "day_beige" => Some(Self::DayBeige),
            "neutral_gray" => Some(Self::NeutralGray),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShiftSlotId(String);

impl ShiftSlotId {
    pub fn new(value: impl Into<String>) -> Result<Self, PlanningError> {
        let value = normalize_text(&value.into());

        if value.is_empty() {
            return Err(PlanningError::EmptyShiftSlotId);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ShiftSlotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShiftSlot {
    id: ShiftSlotId,
    name: String,
    short_code: String,
    start_time: ClockTime,
    end_time: ClockTime,
    visual_style: ShiftVisualStyle,
    sort_order: i32,
    active: bool,
}

impl ShiftSlot {
    pub fn new(
        id: ShiftSlotId,
        name: impl Into<String>,
        short_code: impl Into<String>,
        start_time: ClockTime,
        end_time: ClockTime,
        visual_style: ShiftVisualStyle,
        sort_order: i32,
        active: bool,
    ) -> Result<Self, PlanningError> {
        let name = normalize_text(&name.into());
        let short_code = normalize_text(&short_code.into()).to_uppercase();

        if name.is_empty() {
            return Err(PlanningError::EmptyShiftSlotName);
        }

        if short_code.is_empty() {
            return Err(PlanningError::EmptyShiftSlotCode);
        }

        Ok(Self {
            id,
            name,
            short_code,
            start_time,
            end_time,
            visual_style,
            sort_order,
            active,
        })
    }

    pub fn default_slots() -> Vec<Self> {
        vec![
            Self::new(
                ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_MORNING).expect("identifiant valide"),
                "Matin",
                "M",
                ClockTime::new(5, 0).expect("heure valide"),
                ClockTime::new(13, 0).expect("heure valide"),
                ShiftVisualStyle::MorningYellow,
                10,
                true,
            )
            .expect("plage valide"),
            Self::new(
                ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_AFTERNOON).expect("identifiant valide"),
                "Apres-midi",
                "A",
                ClockTime::new(13, 0).expect("heure valide"),
                ClockTime::new(21, 0).expect("heure valide"),
                ShiftVisualStyle::AfternoonRed,
                20,
                true,
            )
            .expect("plage valide"),
            Self::new(
                ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_NIGHT).expect("identifiant valide"),
                "Nuit",
                "N",
                ClockTime::new(21, 0).expect("heure valide"),
                ClockTime::new(5, 0).expect("heure valide"),
                ShiftVisualStyle::NightBlue,
                30,
                true,
            )
            .expect("plage valide"),
            Self::new(
                ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_DAY).expect("identifiant valide"),
                "Journee",
                "J",
                ClockTime::new(8, 30).expect("heure valide"),
                ClockTime::new(16, 30).expect("heure valide"),
                ShiftVisualStyle::DayBeige,
                40,
                true,
            )
            .expect("plage valide"),
        ]
    }

    pub fn default_rotation_order() -> Vec<ShiftSlotId> {
        vec![
            ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_AFTERNOON).expect("identifiant valide"),
            ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_MORNING).expect("identifiant valide"),
            ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_NIGHT).expect("identifiant valide"),
        ]
    }

    pub fn id(&self) -> &ShiftSlotId {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn short_code(&self) -> &str {
        &self.short_code
    }

    pub const fn start_time(&self) -> ClockTime {
        self.start_time
    }

    pub const fn end_time(&self) -> ClockTime {
        self.end_time
    }

    pub const fn visual_style(&self) -> ShiftVisualStyle {
        self.visual_style
    }

    pub const fn sort_order(&self) -> i32 {
        self.sort_order
    }

    pub const fn active(&self) -> bool {
        self.active
    }

    pub fn crosses_midnight(&self) -> bool {
        self.end_time.minutes_since_midnight() <= self.start_time.minutes_since_midnight()
    }

    pub fn time_range_label(&self) -> String {
        format!("{} - {}", self.start_time, self.end_time)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TeamMemberRole {
    Leader,
    Operator,
}

impl TeamMemberRole {
    pub const ALL: [Self; 2] = [Self::Leader, Self::Operator];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Leader => "Chef d'equipe",
            Self::Operator => "Operateur",
        }
    }

    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::Leader => "leader",
            Self::Operator => "operator",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "leader" => Some(Self::Leader),
            "operator" => Some(Self::Operator),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TeamId(String);

impl TeamId {
    pub fn new(value: impl Into<String>) -> Result<Self, PlanningError> {
        let value = normalize_text(&value.into());

        if value.is_empty() {
            return Err(PlanningError::EmptyTeamId);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TeamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    id: TeamId,
    name: String,
    anchor_shift_slot_id: ShiftSlotId,
    active: bool,
}

impl Team {
    pub fn new(
        id: TeamId,
        name: impl Into<String>,
        anchor_shift_slot_id: ShiftSlotId,
        active: bool,
    ) -> Result<Self, PlanningError> {
        let name = normalize_text(&name.into());

        if name.is_empty() {
            return Err(PlanningError::EmptyTeamName);
        }

        Ok(Self {
            id,
            name,
            anchor_shift_slot_id,
            active,
        })
    }

    pub fn id(&self) -> &TeamId {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn anchor_shift_slot_id(&self) -> &ShiftSlotId {
        &self.anchor_shift_slot_id
    }

    pub const fn active(&self) -> bool {
        self.active
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamMembership {
    team_id: TeamId,
    worker_id: WorkerId,
    role: TeamMemberRole,
}

impl TeamMembership {
    pub fn new(team_id: TeamId, worker_id: WorkerId, role: TeamMemberRole) -> Self {
        Self {
            team_id,
            worker_id,
            role,
        }
    }

    pub fn team_id(&self) -> &TeamId {
        &self.team_id
    }

    pub fn worker_id(&self) -> &WorkerId {
        &self.worker_id
    }

    pub const fn role(&self) -> TeamMemberRole {
        self.role
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationCycle {
    reference_week_start: PlanningDate,
    ordered_shift_slot_ids: Vec<ShiftSlotId>,
}

impl RotationCycle {
    pub fn new(
        reference_week_start: PlanningDate,
        ordered_shift_slot_ids: Vec<ShiftSlotId>,
    ) -> Result<Self, PlanningError> {
        let ordered_shift_slot_ids = ordered_shift_slot_ids
            .into_iter()
            .collect::<Vec<ShiftSlotId>>();

        if ordered_shift_slot_ids.is_empty() {
            return Err(PlanningError::InvalidRotationCycle);
        }

        let mut seen = BTreeSet::new();

        for shift_slot_id in &ordered_shift_slot_ids {
            if !seen.insert(shift_slot_id.clone()) {
                return Err(PlanningError::RotationHasDuplicateShift {
                    shift_slot_id: shift_slot_id.to_string(),
                });
            }
        }

        Ok(Self {
            reference_week_start: reference_week_start.start_of_week_monday()?,
            ordered_shift_slot_ids,
        })
    }

    pub fn default(reference_week_start: PlanningDate) -> Self {
        Self::new(reference_week_start, ShiftSlot::default_rotation_order())
            .expect("le cycle par defaut est valide")
    }

    pub fn reference_week_start(&self) -> PlanningDate {
        self.reference_week_start
    }

    pub fn ordered_shift_slot_ids(&self) -> &[ShiftSlotId] {
        &self.ordered_shift_slot_ids
    }

    pub fn shift_for(
        &self,
        anchor_shift_slot_id: &ShiftSlotId,
        date: PlanningDate,
    ) -> Result<ShiftSlotId, PlanningError> {
        let anchor_index = self
            .ordered_shift_slot_ids
            .iter()
            .position(|entry| entry == anchor_shift_slot_id)
            .ok_or_else(|| PlanningError::UnknownShiftSlot {
                shift_slot_id: anchor_shift_slot_id.to_string(),
            })? as i64;

        let target_week = date.start_of_week_monday()?;
        let reference_week = self.reference_week_start;
        let weeks_delta = target_week.signed_days_since(reference_week) / 7;
        let cycle_len = self.ordered_shift_slot_ids.len() as i64;
        let normalized_index = (anchor_index + weeks_delta).rem_euclid(cycle_len) as usize;

        Ok(self.ordered_shift_slot_ids[normalized_index].clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worker {
    id: WorkerId,
    last_name: String,
    first_name: String,
    display_name: String,
    job_role: JobRole,
}

impl Worker {
    pub fn new(
        id: WorkerId,
        last_name: impl Into<String>,
        first_name: impl Into<String>,
        job_role: JobRole,
    ) -> Result<Self, PlanningError> {
        let last_name = normalize_text(&last_name.into());
        let first_name = normalize_text(&first_name.into());

        if last_name.is_empty() {
            return Err(PlanningError::EmptyWorkerLastName);
        }

        if first_name.is_empty() {
            return Err(PlanningError::EmptyWorkerFirstName);
        }

        Ok(Self {
            id,
            display_name: format!("{last_name} {first_name}"),
            last_name,
            first_name,
            job_role,
        })
    }

    pub fn id(&self) -> &WorkerId {
        &self.id
    }

    pub fn last_name(&self) -> &str {
        &self.last_name
    }

    pub fn first_name(&self) -> &str {
        &self.first_name
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn job_role(&self) -> &JobRole {
        &self.job_role
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedAssignment {
    worker_id: WorkerId,
    date: PlanningDate,
    shift_slot_id: ShiftSlotId,
}

impl GeneratedAssignment {
    pub fn new(worker_id: WorkerId, date: PlanningDate, shift_slot_id: ShiftSlotId) -> Self {
        Self {
            worker_id,
            date,
            shift_slot_id,
        }
    }

    pub fn worker_id(&self) -> &WorkerId {
        &self.worker_id
    }

    pub const fn date(&self) -> PlanningDate {
        self.date
    }

    pub fn shift_slot_id(&self) -> &ShiftSlotId {
        &self.shift_slot_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualOverrideKind {
    Assignment,
    Off,
}

impl ManualOverrideKind {
    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::Assignment => "assignment",
            Self::Off => "off",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "assignment" => Some(Self::Assignment),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualOverride {
    worker_id: WorkerId,
    date: PlanningDate,
    kind: ManualOverrideKind,
    shift_slot_id: Option<ShiftSlotId>,
}

impl ManualOverride {
    pub fn assignment(worker_id: WorkerId, date: PlanningDate, shift_slot_id: ShiftSlotId) -> Self {
        Self {
            worker_id,
            date,
            kind: ManualOverrideKind::Assignment,
            shift_slot_id: Some(shift_slot_id),
        }
    }

    pub fn off(worker_id: WorkerId, date: PlanningDate) -> Self {
        Self {
            worker_id,
            date,
            kind: ManualOverrideKind::Off,
            shift_slot_id: None,
        }
    }

    pub fn validate(&self) -> Result<(), PlanningError> {
        if self.kind == ManualOverrideKind::Assignment && self.shift_slot_id.is_none() {
            return Err(PlanningError::ManualOverrideMissingShift {
                worker_id: self.worker_id.to_string(),
                date: self.date,
            });
        }

        Ok(())
    }

    pub fn worker_id(&self) -> &WorkerId {
        &self.worker_id
    }

    pub const fn date(&self) -> PlanningDate {
        self.date
    }

    pub const fn kind(&self) -> ManualOverrideKind {
        self.kind
    }

    pub fn shift_slot_id(&self) -> Option<&ShiftSlotId> {
        self.shift_slot_id.as_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOrigin {
    Generated,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanningCell {
    Empty {
        date: PlanningDate,
    },
    Assignment {
        date: PlanningDate,
        shift_slot: ShiftSlot,
        origin: AssignmentOrigin,
    },
    ManualOff {
        date: PlanningDate,
    },
}

impl PlanningCell {
    pub fn date(&self) -> PlanningDate {
        match self {
            Self::Empty { date } | Self::Assignment { date, .. } | Self::ManualOff { date } => {
                *date
            }
        }
    }

    pub fn shift_slot(&self) -> Option<&ShiftSlot> {
        match self {
            Self::Assignment { shift_slot, .. } => Some(shift_slot),
            Self::Empty { .. } | Self::ManualOff { .. } => None,
        }
    }

    pub fn has_assignment(&self) -> bool {
        matches!(self, Self::Assignment { .. })
    }

    pub fn is_manual(&self) -> bool {
        matches!(
            self,
            Self::Assignment {
                origin: AssignmentOrigin::Manual,
                ..
            } | Self::ManualOff { .. }
        )
    }

    pub fn is_manual_off(&self) -> bool {
        matches!(self, Self::ManualOff { .. })
    }

    pub fn short_code(&self) -> String {
        match self {
            Self::Empty { .. } => String::new(),
            Self::Assignment { shift_slot, .. } => shift_slot.short_code().to_owned(),
            Self::ManualOff { .. } => "OFF".to_owned(),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Empty { .. } => String::new(),
            Self::Assignment { shift_slot, .. } => shift_slot.name().to_owned(),
            Self::ManualOff { .. } => "Sans poste".to_owned(),
        }
    }

    pub fn time_range_label(&self) -> String {
        match self {
            Self::Assignment { shift_slot, .. } => shift_slot.time_range_label(),
            Self::Empty { .. } | Self::ManualOff { .. } => String::new(),
        }
    }

    pub fn style_token(&self) -> &'static str {
        match self {
            Self::Empty { .. } => "empty",
            Self::Assignment { shift_slot, .. } => shift_slot.visual_style().token(),
            Self::ManualOff { .. } => "off-gray",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningRow {
    worker_id: WorkerId,
    worker_name: String,
    team_name: String,
    team_role_label: String,
    job_role: JobRole,
    cells: Vec<PlanningCell>,
}

impl PlanningRow {
    pub fn new(
        worker: &Worker,
        team_name: impl Into<String>,
        team_role_label: impl Into<String>,
        cells: Vec<PlanningCell>,
    ) -> Self {
        Self {
            worker_id: worker.id().clone(),
            worker_name: worker.display_name().to_owned(),
            team_name: normalize_text(&team_name.into()),
            team_role_label: normalize_text(&team_role_label.into()),
            job_role: worker.job_role().clone(),
            cells,
        }
    }

    pub fn worker_id(&self) -> &WorkerId {
        &self.worker_id
    }

    pub fn worker_name(&self) -> &str {
        &self.worker_name
    }

    pub fn team_name(&self) -> &str {
        &self.team_name
    }

    pub fn team_role_label(&self) -> &str {
        &self.team_role_label
    }

    pub fn job_role(&self) -> &JobRole {
        &self.job_role
    }

    pub fn cells(&self) -> &[PlanningCell] {
        &self.cells
    }

    pub fn cell_for_offset(&self, offset: u32) -> Option<&PlanningCell> {
        self.cells.get(offset as usize)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangePlanning {
    start_date: PlanningDate,
    total_days: u32,
    rows: Vec<PlanningRow>,
}

impl RangePlanning {
    pub fn new(start_date: PlanningDate, total_days: u32, rows: Vec<PlanningRow>) -> Self {
        Self {
            start_date,
            total_days,
            rows,
        }
    }

    pub fn start_date(&self) -> PlanningDate {
        self.start_date
    }

    pub const fn total_days(&self) -> u32 {
        self.total_days
    }

    pub fn rows(&self) -> &[PlanningRow] {
        &self.rows
    }

    pub fn row_for_worker(&self, worker_id: &WorkerId) -> Option<&PlanningRow> {
        self.rows.iter().find(|row| row.worker_id() == worker_id)
    }

    pub fn date_for_offset(&self, offset: u32) -> Result<PlanningDate, PlanningError> {
        self.start_date.add_days(offset as i64)
    }
}

pub fn build_worker_team_map(
    teams: &[Team],
    memberships: &[TeamMembership],
) -> BTreeMap<WorkerId, (String, String)> {
    let teams_by_id: BTreeMap<TeamId, Team> = teams
        .iter()
        .cloned()
        .map(|team| (team.id().clone(), team))
        .collect();

    memberships
        .iter()
        .filter_map(|membership| {
            teams_by_id.get(membership.team_id()).map(|team| {
                (
                    membership.worker_id().clone(),
                    (team.name().to_owned(), membership.role().label().to_owned()),
                )
            })
        })
        .collect()
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planning_date_handles_leap_year_and_boundaries() {
        assert_eq!(PlanningDate::days_in_month(2024, 2).unwrap(), 29);
        assert_eq!(PlanningDate::days_in_month(2025, 2).unwrap(), 28);
        assert_eq!(
            PlanningDate::new(2025, 12, 31)
                .unwrap()
                .add_days(1)
                .unwrap(),
            PlanningDate::new(2026, 1, 1).unwrap()
        );
        assert_eq!(
            PlanningDate::parse_iso("2026-04-23").unwrap(),
            PlanningDate::new(2026, 4, 23).unwrap()
        );
    }

    #[test]
    fn shift_slot_defaults_match_expected_3x8_labels() {
        let slots = ShiftSlot::default_slots();

        assert_eq!(slots.len(), 4);
        assert_eq!(slots[0].name(), "Matin");
        assert_eq!(slots[1].name(), "Apres-midi");
        assert_eq!(slots[2].name(), "Nuit");
        assert!(slots[2].crosses_midnight());
        assert_eq!(slots[2].time_range_label(), "21h00 - 05h00");
    }

    #[test]
    fn rotation_cycle_rotates_weekly_in_three_eight_order() {
        let cycle = RotationCycle::default(PlanningDate::new(2026, 1, 5).unwrap());
        let anchor_shift = ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_AFTERNOON).unwrap();

        assert_eq!(
            cycle
                .shift_for(&anchor_shift, PlanningDate::new(2026, 1, 5).unwrap())
                .unwrap()
                .as_str(),
            DEFAULT_SHIFT_SLOT_ID_AFTERNOON
        );
        assert_eq!(
            cycle
                .shift_for(&anchor_shift, PlanningDate::new(2026, 1, 12).unwrap())
                .unwrap()
                .as_str(),
            DEFAULT_SHIFT_SLOT_ID_MORNING
        );
        assert_eq!(
            cycle
                .shift_for(&anchor_shift, PlanningDate::new(2026, 1, 19).unwrap())
                .unwrap()
                .as_str(),
            DEFAULT_SHIFT_SLOT_ID_NIGHT
        );
        assert_eq!(
            cycle
                .shift_for(&anchor_shift, PlanningDate::new(2026, 1, 26).unwrap())
                .unwrap()
                .as_str(),
            DEFAULT_SHIFT_SLOT_ID_AFTERNOON
        );
    }

    #[test]
    fn manual_override_requires_shift_when_it_is_an_assignment() {
        let override_without_shift = ManualOverride {
            worker_id: WorkerId::new("worker-01").unwrap(),
            date: PlanningDate::new(2026, 4, 23).unwrap(),
            kind: ManualOverrideKind::Assignment,
            shift_slot_id: None,
        };

        assert!(matches!(
            override_without_shift.validate(),
            Err(PlanningError::ManualOverrideMissingShift { .. })
        ));
        assert!(
            ManualOverride::off(
                WorkerId::new("worker-01").unwrap(),
                PlanningDate::new(2026, 4, 23).unwrap()
            )
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn build_worker_team_map_returns_team_and_role_labels() {
        let team = Team::new(
            TeamId::new("team-a").unwrap(),
            "Equipe A",
            ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_AFTERNOON).unwrap(),
            true,
        )
        .unwrap();
        let membership = TeamMembership::new(
            team.id().clone(),
            WorkerId::new("worker-01").unwrap(),
            TeamMemberRole::Leader,
        );

        let mapping = build_worker_team_map(&[team], &[membership]);

        assert_eq!(
            mapping.get(&WorkerId::new("worker-01").unwrap()),
            Some(&("Equipe A".to_owned(), "Chef d'equipe".to_owned()))
        );
    }
}
