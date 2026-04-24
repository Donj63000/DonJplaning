use std::error::Error;
use std::fmt;

use crate::domain::PlanningError;

#[derive(Debug)]
pub enum AppError {
    Planning(PlanningError),
    Database(rusqlite::Error),
    Io(std::io::Error),
    DirectoriesUnavailable,
    UnsupportedDatabaseSchema,
    InvalidJobRole(String),
    InvalidShiftStyle(String),
    InvalidShiftSlot(String),
    InconsistentDatabase(String),
    InvalidDateInput(String),
    InvalidNumericInput(String),
    MissingWorkerSelection,
    MissingTeamSelection,
    MissingShiftSlotSelection,
    MissingJobRoleSelection,
    MissingTeamMemberRoleSelection,
    MissingPlanningCellSelection,
    WorkerHasPlanningLinks {
        worker_id: String,
    },
    DuplicateWorkerIdentity {
        last_name: String,
        first_name: String,
    },
    DuplicateShiftSlotCode {
        short_code: String,
    },
    DuplicateTeamName {
        team_name: String,
    },
    WorkerAlreadyAssignedToTeam {
        worker_id: String,
        team_name: String,
    },
    TeamAlreadyHasLeader {
        team_name: String,
    },
    UiEventLoop(slint::EventLoopError),
    UiPlatform(slint::PlatformError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Planning(error) => error.fmt(f),
            Self::Database(error) => write!(f, "Erreur SQLite: {error}"),
            Self::Io(error) => write!(f, "Erreur d'entree/sortie: {error}"),
            Self::DirectoriesUnavailable => write!(
                f,
                "Impossible de determiner le dossier local de l'application sur cette machine."
            ),
            Self::UnsupportedDatabaseSchema => write!(
                f,
                "La base locale n'est pas compatible avec cette version du logiciel."
            ),
            Self::InvalidJobRole(value) => write!(f, "Le poste '{value}' n'est pas valide."),
            Self::InvalidShiftStyle(value) => {
                write!(f, "Le style visuel '{value}' n'est pas reconnu.")
            }
            Self::InvalidShiftSlot(value) => {
                write!(f, "La plage horaire '{value}' n'est pas reconnue.")
            }
            Self::InconsistentDatabase(value) => {
                write!(f, "La base locale contient une incoherence: {value}")
            }
            Self::InvalidDateInput(value) => write!(
                f,
                "La date '{value}' n'est pas valide. Le format attendu est AAAA-MM-JJ."
            ),
            Self::InvalidNumericInput(value) => {
                write!(f, "La valeur numerique '{value}' n'est pas valide.")
            }
            Self::MissingWorkerSelection => write!(f, "Je dois d'abord selectionner un salarie."),
            Self::MissingTeamSelection => write!(f, "Je dois d'abord selectionner une equipe."),
            Self::MissingShiftSlotSelection => {
                write!(f, "Je dois d'abord selectionner une plage horaire.")
            }
            Self::MissingJobRoleSelection => write!(f, "Je dois selectionner un poste."),
            Self::MissingTeamMemberRoleSelection => {
                write!(f, "Je dois selectionner le role du membre d'equipe.")
            }
            Self::MissingPlanningCellSelection => {
                write!(f, "Je dois d'abord selectionner une cellule du planning.")
            }
            Self::WorkerHasPlanningLinks { worker_id } => write!(
                f,
                "Je ne peux pas supprimer le salarie '{worker_id}' tant qu'il est encore utilise dans une equipe ou dans le planning."
            ),
            Self::DuplicateWorkerIdentity {
                last_name,
                first_name,
            } => write!(f, "Une fiche existe deja pour {last_name} {first_name}."),
            Self::DuplicateShiftSlotCode { short_code } => write!(
                f,
                "Le code court '{short_code}' est deja utilise par une autre plage horaire."
            ),
            Self::DuplicateTeamName { team_name } => {
                write!(f, "Une equipe nommee '{team_name}' existe deja.")
            }
            Self::WorkerAlreadyAssignedToTeam {
                worker_id,
                team_name,
            } => write!(
                f,
                "Le salarie '{worker_id}' est deja rattache a l'equipe '{team_name}'."
            ),
            Self::TeamAlreadyHasLeader { team_name } => write!(
                f,
                "L'equipe '{team_name}' possede deja un chef d'equipe. Je dois le retirer avant d'en ajouter un autre."
            ),
            Self::UiEventLoop(error) => write!(f, "Erreur de boucle d'evenements UI: {error}"),
            Self::UiPlatform(error) => write!(f, "Erreur de plateforme UI: {error}"),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Planning(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::UiEventLoop(error) => Some(error),
            Self::UiPlatform(error) => Some(error),
            Self::DirectoriesUnavailable
            | Self::UnsupportedDatabaseSchema
            | Self::InvalidJobRole(_)
            | Self::InvalidShiftStyle(_)
            | Self::InvalidShiftSlot(_)
            | Self::InconsistentDatabase(_)
            | Self::InvalidDateInput(_)
            | Self::InvalidNumericInput(_)
            | Self::MissingWorkerSelection
            | Self::MissingTeamSelection
            | Self::MissingShiftSlotSelection
            | Self::MissingJobRoleSelection
            | Self::MissingTeamMemberRoleSelection
            | Self::MissingPlanningCellSelection
            | Self::WorkerHasPlanningLinks { .. }
            | Self::DuplicateWorkerIdentity { .. }
            | Self::DuplicateShiftSlotCode { .. }
            | Self::DuplicateTeamName { .. }
            | Self::WorkerAlreadyAssignedToTeam { .. }
            | Self::TeamAlreadyHasLeader { .. } => None,
        }
    }
}

impl From<PlanningError> for AppError {
    fn from(value: PlanningError) -> Self {
        Self::Planning(value)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<slint::EventLoopError> for AppError {
    fn from(value: slint::EventLoopError) -> Self {
        Self::UiEventLoop(value)
    }
}

impl From<slint::PlatformError> for AppError {
    fn from(value: slint::PlatformError) -> Self {
        Self::UiPlatform(value)
    }
}
