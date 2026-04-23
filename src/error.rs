use std::error::Error;
use std::fmt;

use crate::domain::PlanningError;

#[derive(Debug)]
pub enum AppError {
    Planning(PlanningError),
    Database(rusqlite::Error),
    Io(std::io::Error),
    DirectoriesUnavailable,
    InvalidJobRole(String),
    InvalidShiftKind(String),
    InvalidMonthInput(i32),
    InvalidDayInput(i32),
    MissingWorkerSelection,
    MissingAssignmentSelection,
    MissingShiftSelection,
    MissingJobRoleSelection,
    WorkerHasAssignments {
        worker_id: String,
    },
    DuplicateWorkerIdentity {
        last_name: String,
        first_name: String,
    },
    UnsupportedDatabaseSchema,
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
            Self::InvalidJobRole(value) => {
                write!(f, "Le poste '{value}' n'est pas valide.")
            }
            Self::InvalidShiftKind(value) => {
                write!(f, "L'horaire '{value}' n'est pas reconnu par le logiciel.")
            }
            Self::InvalidMonthInput(value) => {
                write!(
                    f,
                    "Le mois '{value}' est invalide. La valeur attendue est comprise entre 1 et 12."
                )
            }
            Self::InvalidDayInput(value) => {
                write!(
                    f,
                    "Le jour '{value}' est invalide pour le mois selectionne."
                )
            }
            Self::MissingWorkerSelection => {
                write!(f, "Je dois selectionner une fiche avant de continuer.")
            }
            Self::MissingAssignmentSelection => {
                write!(
                    f,
                    "Je dois selectionner une affectation existante avant de continuer."
                )
            }
            Self::MissingShiftSelection => {
                write!(f, "Je dois selectionner un horaire.")
            }
            Self::MissingJobRoleSelection => {
                write!(f, "Je dois selectionner un poste.")
            }
            Self::WorkerHasAssignments { worker_id } => write!(
                f,
                "Je ne peux pas supprimer la fiche '{worker_id}' tant qu'elle possede des affectations."
            ),
            Self::DuplicateWorkerIdentity {
                last_name,
                first_name,
            } => write!(f, "Une fiche existe deja pour {last_name} {first_name}."),
            Self::UnsupportedDatabaseSchema => write!(
                f,
                "La base locale n'est pas compatible avec cette version du logiciel."
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
            | Self::InvalidJobRole(_)
            | Self::InvalidShiftKind(_)
            | Self::InvalidMonthInput(_)
            | Self::InvalidDayInput(_)
            | Self::MissingWorkerSelection
            | Self::MissingAssignmentSelection
            | Self::MissingShiftSelection
            | Self::MissingJobRoleSelection
            | Self::WorkerHasAssignments { .. }
            | Self::DuplicateWorkerIdentity { .. }
            | Self::UnsupportedDatabaseSchema => None,
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

impl From<slint::PlatformError> for AppError {
    fn from(value: slint::PlatformError) -> Self {
        Self::UiPlatform(value)
    }
}

impl From<slint::EventLoopError> for AppError {
    fn from(value: slint::EventLoopError) -> Self {
        Self::UiEventLoop(value)
    }
}
