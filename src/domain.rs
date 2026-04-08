use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanningError {
    EmptyWorkerId,
    EmptyWorkerName,
    InvalidMonth {
        month: u8,
    },
    InvalidDay {
        year: i32,
        month: u8,
        day: u8,
    },
    InvalidClockTime {
        hour: u8,
        minute: u8,
    },
    DuplicateWorkerId {
        worker_id: String,
    },
    UnknownWorker {
        worker_id: String,
    },
    AssignmentOutsideTargetMonth {
        expected_year: i32,
        expected_month: u8,
        date: PlanningDate,
    },
    WorkerAlreadyAssignedOnDate {
        worker_id: String,
        date: PlanningDate,
    },
}

impl fmt::Display for PlanningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyWorkerId => write!(f, "L'identifiant de l'ouvrier ne peut pas etre vide."),
            Self::EmptyWorkerName => write!(f, "Le nom de l'ouvrier ne peut pas etre vide."),
            Self::InvalidMonth { month } => {
                write!(
                    f,
                    "Le mois {month} est invalide. La valeur attendue est comprise entre 1 et 12."
                )
            }
            Self::InvalidDay { year, month, day } => write!(
                f,
                "Le jour {day:02}/{month:02}/{year:04} est invalide pour le mois demande."
            ),
            Self::InvalidClockTime { hour, minute } => write!(
                f,
                "L'heure {hour:02}h{minute:02} est invalide. Les bornes attendues sont 00h00 a 23h59."
            ),
            Self::DuplicateWorkerId { worker_id } => write!(
                f,
                "L'identifiant d'ouvrier '{worker_id}' est duplique dans la base utilisateur."
            ),
            Self::UnknownWorker { worker_id } => write!(
                f,
                "L'ouvrier '{worker_id}' n'existe pas dans la base utilisateur du planning."
            ),
            Self::AssignmentOutsideTargetMonth {
                expected_year,
                expected_month,
                date,
            } => write!(
                f,
                "L'affectation du {date} n'appartient pas au mois cible {expected_month:02}/{expected_year:04}."
            ),
            Self::WorkerAlreadyAssignedOnDate { worker_id, date } => write!(
                f,
                "L'ouvrier '{worker_id}' possede deja une affectation sur la date {date}."
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
}

impl fmt::Display for ClockTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}h{:02}", self.hour, self.minute)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkerId(String);

impl WorkerId {
    pub fn new(value: impl Into<String>) -> Result<Self, PlanningError> {
        let value = value.into();
        let normalized = value.trim();

        if normalized.is_empty() {
            return Err(PlanningError::EmptyWorkerId);
        }

        Ok(Self(normalized.to_owned()))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobRole {
    OperateurProduction,
    OperateurSalleBlanche,
    ChefDEquipes,
    Autre,
}

impl JobRole {
    pub const ALL: [Self; 4] = [
        Self::OperateurProduction,
        Self::OperateurSalleBlanche,
        Self::ChefDEquipes,
        Self::Autre,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::OperateurProduction => "Operateur de production",
            Self::OperateurSalleBlanche => "Operateur de salle blanche",
            Self::ChefDEquipes => "Chef d'equipes",
            Self::Autre => "Autre",
        }
    }

    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::OperateurProduction => "operateur_production",
            Self::OperateurSalleBlanche => "operateur_salle_blanche",
            Self::ChefDEquipes => "chef_d_equipes",
            Self::Autre => "autre",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "operateur_production" => Some(Self::OperateurProduction),
            "operateur_salle_blanche" => Some(Self::OperateurSalleBlanche),
            "chef_d_equipes" => Some(Self::ChefDEquipes),
            "autre" => Some(Self::Autre),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShiftStyleKey {
    NightBlue,
    AfternoonRed,
    MorningYellow,
    DayBeige,
}

impl ShiftStyleKey {
    pub const fn token(self) -> &'static str {
        match self {
            Self::NightBlue => "night-blue",
            Self::AfternoonRed => "afternoon-red",
            Self::MorningYellow => "morning-yellow",
            Self::DayBeige => "day-beige",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShiftKind {
    Night,
    Afternoon,
    Morning,
    Day,
}

impl ShiftKind {
    pub const ALL: [Self; 4] = [Self::Night, Self::Afternoon, Self::Morning, Self::Day];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Night => "Nuit",
            Self::Afternoon => "Apres-midi",
            Self::Morning => "Matin",
            Self::Day => "Journee",
        }
    }

    pub const fn start_time(self) -> ClockTime {
        match self {
            Self::Night => ClockTime {
                hour: 21,
                minute: 0,
            },
            Self::Afternoon => ClockTime {
                hour: 13,
                minute: 0,
            },
            Self::Morning => ClockTime { hour: 5, minute: 0 },
            Self::Day => ClockTime {
                hour: 8,
                minute: 30,
            },
        }
    }

    pub const fn end_time(self) -> ClockTime {
        match self {
            Self::Night => ClockTime { hour: 5, minute: 0 },
            Self::Afternoon => ClockTime {
                hour: 21,
                minute: 0,
            },
            Self::Morning => ClockTime {
                hour: 13,
                minute: 0,
            },
            Self::Day => ClockTime {
                hour: 16,
                minute: 30,
            },
        }
    }

    pub const fn style_key(self) -> ShiftStyleKey {
        match self {
            Self::Night => ShiftStyleKey::NightBlue,
            Self::Afternoon => ShiftStyleKey::AfternoonRed,
            Self::Morning => ShiftStyleKey::MorningYellow,
            Self::Day => ShiftStyleKey::DayBeige,
        }
    }

    pub const fn crosses_midnight(self) -> bool {
        matches!(self, Self::Night)
    }

    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::Night => "night",
            Self::Afternoon => "afternoon",
            Self::Morning => "morning",
            Self::Day => "day",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "night" => Some(Self::Night),
            "afternoon" => Some(Self::Afternoon),
            "morning" => Some(Self::Morning),
            "day" => Some(Self::Day),
            _ => None,
        }
    }

    pub const fn short_code(self) -> &'static str {
        match self {
            Self::Night => "N",
            Self::Afternoon => "A",
            Self::Morning => "M",
            Self::Day => "J",
        }
    }

    pub const fn time_range_label(self) -> &'static str {
        match self {
            Self::Night => "21h00 - 05h00",
            Self::Afternoon => "13h00 - 21h00",
            Self::Morning => "05h00 - 13h00",
            Self::Day => "08h30 - 16h30",
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worker {
    id: WorkerId,
    display_name: String,
    job_role: JobRole,
}

impl Worker {
    pub fn new(
        id: WorkerId,
        display_name: impl Into<String>,
        job_role: JobRole,
    ) -> Result<Self, PlanningError> {
        let display_name = display_name.into();
        let normalized = display_name.trim();

        if normalized.is_empty() {
            return Err(PlanningError::EmptyWorkerName);
        }

        Ok(Self {
            id,
            display_name: normalized.to_owned(),
            job_role,
        })
    }

    pub fn id(&self) -> &WorkerId {
        &self.id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub const fn job_role(&self) -> JobRole {
        self.job_role
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    worker_id: WorkerId,
    date: PlanningDate,
    shift_kind: ShiftKind,
}

impl Assignment {
    pub fn new(worker_id: WorkerId, date: PlanningDate, shift_kind: ShiftKind) -> Self {
        Self {
            worker_id,
            date,
            shift_kind,
        }
    }

    pub fn worker_id(&self) -> &WorkerId {
        &self.worker_id
    }

    pub const fn date(&self) -> PlanningDate {
        self.date
    }

    pub const fn shift_kind(&self) -> ShiftKind {
        self.shift_kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningCell {
    date: PlanningDate,
    shift_kind: ShiftKind,
    job_role: JobRole,
}

impl PlanningCell {
    pub fn new(date: PlanningDate, shift_kind: ShiftKind, job_role: JobRole) -> Self {
        Self {
            date,
            shift_kind,
            job_role,
        }
    }

    pub const fn date(&self) -> PlanningDate {
        self.date
    }

    pub const fn shift_kind(&self) -> ShiftKind {
        self.shift_kind
    }

    pub const fn job_role(&self) -> JobRole {
        self.job_role
    }

    pub const fn shift_label(&self) -> &'static str {
        self.shift_kind.label()
    }

    pub const fn style_key(&self) -> ShiftStyleKey {
        self.shift_kind.style_key()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningRow {
    worker_id: WorkerId,
    worker_name: String,
    job_role: JobRole,
    pub(crate) cells: Vec<Option<PlanningCell>>,
}

impl PlanningRow {
    pub(crate) fn new(worker: &Worker, total_days: u8) -> Self {
        Self {
            worker_id: worker.id().clone(),
            worker_name: worker.display_name().to_owned(),
            job_role: worker.job_role(),
            cells: vec![None; total_days as usize],
        }
    }

    pub(crate) fn set_cell(&mut self, day: u8, cell: PlanningCell) {
        let index = (day - 1) as usize;
        self.cells[index] = Some(cell);
    }

    pub fn worker_id(&self) -> &WorkerId {
        &self.worker_id
    }

    pub fn worker_name(&self) -> &str {
        &self.worker_name
    }

    pub const fn job_role(&self) -> JobRole {
        self.job_role
    }

    pub fn cells(&self) -> &[Option<PlanningCell>] {
        &self.cells
    }

    pub fn cell_for_day(&self, day: u8) -> Option<&PlanningCell> {
        if day == 0 {
            return None;
        }

        self.cells
            .get((day - 1) as usize)
            .and_then(std::option::Option::as_ref)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthlyPlanning {
    year: i32,
    month: u8,
    total_days: u8,
    rows: Vec<PlanningRow>,
}

impl MonthlyPlanning {
    pub(crate) fn new(year: i32, month: u8, total_days: u8, rows: Vec<PlanningRow>) -> Self {
        Self {
            year,
            month,
            total_days,
            rows,
        }
    }

    pub const fn year(&self) -> i32 {
        self.year
    }

    pub const fn month(&self) -> u8 {
        self.month
    }

    pub const fn total_days(&self) -> u8 {
        self.total_days
    }

    pub fn rows(&self) -> &[PlanningRow] {
        &self.rows
    }

    pub fn row_for_worker(&self, worker_id: &WorkerId) -> Option<&PlanningRow> {
        self.rows.iter().find(|row| row.worker_id() == worker_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_role_labels_are_stable() {
        assert_eq!(
            JobRole::OperateurProduction.label(),
            "Operateur de production"
        );
        assert_eq!(
            JobRole::OperateurSalleBlanche.label(),
            "Operateur de salle blanche"
        );
        assert_eq!(JobRole::ChefDEquipes.label(), "Chef d'equipes");
        assert_eq!(JobRole::Autre.label(), "Autre");
        assert_eq!(
            JobRole::from_storage_key(JobRole::ChefDEquipes.storage_key()),
            Some(JobRole::ChefDEquipes)
        );
    }

    #[test]
    fn shift_kind_exposes_expected_schedule_and_style() {
        assert_eq!(ShiftKind::Night.label(), "Nuit");
        assert_eq!(
            ShiftKind::Night.start_time(),
            ClockTime::new(21, 0).unwrap()
        );
        assert_eq!(ShiftKind::Night.end_time(), ClockTime::new(5, 0).unwrap());
        assert_eq!(ShiftKind::Night.style_key(), ShiftStyleKey::NightBlue);
        assert!(ShiftKind::Night.crosses_midnight());

        assert_eq!(
            ShiftKind::Afternoon.style_key(),
            ShiftStyleKey::AfternoonRed
        );
        assert_eq!(
            ShiftKind::Morning.start_time(),
            ClockTime::new(5, 0).unwrap()
        );
        assert_eq!(ShiftKind::Day.end_time(), ClockTime::new(16, 30).unwrap());
        assert_eq!(ShiftStyleKey::DayBeige.token(), "day-beige");
        assert_eq!(ShiftKind::Night.short_code(), "N");
        assert_eq!(ShiftKind::Day.time_range_label(), "08h30 - 16h30");
        assert_eq!(
            ShiftKind::from_storage_key(ShiftKind::Morning.storage_key()),
            Some(ShiftKind::Morning)
        );
    }

    #[test]
    fn planning_date_validates_leap_year_boundaries() {
        assert_eq!(PlanningDate::days_in_month(2024, 2).unwrap(), 29);
        assert_eq!(PlanningDate::days_in_month(2025, 2).unwrap(), 28);
        assert!(PlanningDate::new(2024, 2, 29).is_ok());
        assert!(matches!(
            PlanningDate::new(2025, 2, 29),
            Err(PlanningError::InvalidDay {
                year: 2025,
                month: 2,
                day: 29
            })
        ));
    }

    #[test]
    fn worker_requires_non_empty_identifiers_and_name() {
        assert!(matches!(
            WorkerId::new("   "),
            Err(PlanningError::EmptyWorkerId)
        ));

        let worker_id = WorkerId::new("worker-01").unwrap();
        assert!(matches!(
            Worker::new(worker_id, "   ", JobRole::Autre),
            Err(PlanningError::EmptyWorkerName)
        ));
    }

    #[test]
    fn clock_time_rejects_invalid_values() {
        assert!(matches!(
            ClockTime::new(24, 0),
            Err(PlanningError::InvalidClockTime {
                hour: 24,
                minute: 0
            })
        ));
        assert!(matches!(
            ClockTime::new(10, 60),
            Err(PlanningError::InvalidClockTime {
                hour: 10,
                minute: 60
            })
        ));
    }
}
