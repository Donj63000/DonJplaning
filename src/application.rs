use std::collections::{BTreeMap, btree_map::Entry};
use std::rc::Rc;

use crate::domain::{
    Assignment, JobRole, MonthlyPlanning, PlanningCell, PlanningDate, PlanningError, PlanningRow,
    ShiftKind, Worker, WorkerId,
};
use crate::error::AppError;
use crate::infrastructure::SqliteDatabase;

#[derive(Debug, Default, Clone, Copy)]
pub struct PlanningGenerator;

impl PlanningGenerator {
    pub fn build_month(
        year: i32,
        month: u8,
        workers: &[Worker],
        assignments: &[Assignment],
    ) -> Result<MonthlyPlanning, PlanningError> {
        let total_days = PlanningDate::days_in_month(year, month)?;
        let workers_by_id = Self::index_workers(workers)?;
        let assignments_by_worker =
            Self::index_assignments(year, month, &workers_by_id, assignments)?;

        let mut rows = Vec::with_capacity(workers.len());

        for worker in workers {
            let mut row = PlanningRow::new(worker, total_days);

            if let Some(assignments_for_worker) = assignments_by_worker.get(worker.id()) {
                for assignment in assignments_for_worker.values() {
                    let cell = PlanningCell::new(
                        assignment.date(),
                        assignment.shift_kind(),
                        worker.job_role().clone(),
                    );
                    row.set_cell(assignment.date().day(), cell);
                }
            }

            rows.push(row);
        }

        Ok(MonthlyPlanning::new(year, month, total_days, rows))
    }

    fn index_workers(workers: &[Worker]) -> Result<BTreeMap<WorkerId, Worker>, PlanningError> {
        let mut workers_by_id = BTreeMap::new();

        for worker in workers {
            match workers_by_id.entry(worker.id().clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(worker.clone());
                }
                Entry::Occupied(_) => {
                    return Err(PlanningError::DuplicateWorkerId {
                        worker_id: worker.id().to_string(),
                    });
                }
            }
        }

        Ok(workers_by_id)
    }

    fn index_assignments(
        year: i32,
        month: u8,
        workers_by_id: &BTreeMap<WorkerId, Worker>,
        assignments: &[Assignment],
    ) -> Result<BTreeMap<WorkerId, BTreeMap<u8, Assignment>>, PlanningError> {
        let mut assignments_by_worker: BTreeMap<WorkerId, BTreeMap<u8, Assignment>> =
            BTreeMap::new();

        for assignment in assignments {
            let worker_id = assignment.worker_id();
            let date = assignment.date();

            if date.year() != year || date.month() != month {
                return Err(PlanningError::AssignmentOutsideTargetMonth {
                    expected_year: year,
                    expected_month: month,
                    date,
                });
            }

            if !workers_by_id.contains_key(worker_id) {
                return Err(PlanningError::UnknownWorker {
                    worker_id: worker_id.to_string(),
                });
            }

            let assignments_for_worker =
                assignments_by_worker.entry(worker_id.clone()).or_default();

            match assignments_for_worker.entry(date.day()) {
                Entry::Vacant(entry) => {
                    entry.insert(assignment.clone());
                }
                Entry::Occupied(_) => {
                    return Err(PlanningError::WorkerAlreadyAssignedOnDate {
                        worker_id: worker_id.to_string(),
                        date,
                    });
                }
            }
        }

        Ok(assignments_by_worker)
    }
}

#[derive(Debug, Clone)]
pub struct JobRoleService {
    database: Rc<SqliteDatabase>,
}

impl JobRoleService {
    pub fn new(database: Rc<SqliteDatabase>) -> Self {
        Self { database }
    }

    pub fn list_all(&self) -> Result<Vec<JobRole>, AppError> {
        self.database.list_job_roles()
    }

    pub fn save_role(&self, role_name: impl Into<String>) -> Result<JobRole, AppError> {
        let role = JobRole::new(role_name)?;
        self.database.upsert_job_role(&role)?;
        Ok(role)
    }
}

#[derive(Debug, Clone)]
pub struct WorkerService {
    database: Rc<SqliteDatabase>,
}

impl WorkerService {
    pub fn new(database: Rc<SqliteDatabase>) -> Self {
        Self { database }
    }

    pub fn list_all(&self) -> Result<Vec<Worker>, AppError> {
        self.database.list_workers()
    }

    pub fn save_worker(
        &self,
        worker_id: Option<String>,
        last_name: impl Into<String>,
        first_name: impl Into<String>,
        job_role: JobRole,
    ) -> Result<Worker, AppError> {
        let worker_id = match worker_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(existing_id) => WorkerId::new(existing_id)?,
            None => self.database.generate_worker_id()?,
        };

        let worker = Worker::new(worker_id, last_name, first_name, job_role)?;

        if let Some(existing_worker_id) = self
            .database
            .find_worker_id_by_identity(worker.last_name(), worker.first_name())?
        {
            if existing_worker_id != worker.id().clone() {
                return Err(AppError::DuplicateWorkerIdentity {
                    last_name: worker.last_name().to_owned(),
                    first_name: worker.first_name().to_owned(),
                });
            }
        }

        self.database.upsert_job_role(worker.job_role())?;
        self.database.upsert_worker(&worker)?;
        Ok(worker)
    }

    pub fn delete_worker(&self, worker_id: &WorkerId) -> Result<(), AppError> {
        self.database.delete_worker(worker_id)
    }
}

#[derive(Debug, Clone)]
pub struct AssignmentService {
    database: Rc<SqliteDatabase>,
}

impl AssignmentService {
    pub fn new(database: Rc<SqliteDatabase>) -> Self {
        Self { database }
    }

    pub fn list_month(&self, year: i32, month: u8) -> Result<Vec<Assignment>, AppError> {
        self.database.list_assignments_for_month(year, month)
    }

    pub fn upsert_assignment(
        &self,
        worker_id: &WorkerId,
        date: PlanningDate,
        shift_kind: ShiftKind,
    ) -> Result<(), AppError> {
        self.database
            .upsert_assignment(&Assignment::new(worker_id.clone(), date, shift_kind))
    }

    pub fn delete_assignment(
        &self,
        worker_id: &WorkerId,
        date: PlanningDate,
    ) -> Result<(), AppError> {
        self.database.delete_assignment(worker_id, date)
    }
}

#[derive(Debug, Clone)]
pub struct PlanningFacade {
    worker_service: WorkerService,
    assignment_service: AssignmentService,
}

impl PlanningFacade {
    pub fn new(worker_service: WorkerService, assignment_service: AssignmentService) -> Self {
        Self {
            worker_service,
            assignment_service,
        }
    }

    pub fn load_month(&self, year: i32, month: u8) -> Result<LoadedMonthPlanning, AppError> {
        let workers = self.worker_service.list_all()?;
        let assignments = self.assignment_service.list_month(year, month)?;
        let planning = PlanningGenerator::build_month(year, month, &workers, &assignments)?;

        Ok(LoadedMonthPlanning { workers, planning })
    }
}

#[derive(Debug, Clone)]
pub struct LoadedMonthPlanning {
    workers: Vec<Worker>,
    planning: MonthlyPlanning,
}

impl LoadedMonthPlanning {
    pub fn workers(&self) -> &[Worker] {
        &self.workers
    }

    pub fn planning(&self) -> &MonthlyPlanning {
        &self.planning
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ShiftStyleKey, WorkerId};
    use crate::infrastructure::SqliteDatabase;

    fn role(label: &str) -> JobRole {
        JobRole::new(label).unwrap()
    }

    fn build_worker(id: &str, last_name: &str, first_name: &str, job_role: &str) -> Worker {
        Worker::new(
            WorkerId::new(id).unwrap(),
            last_name,
            first_name,
            role(job_role),
        )
        .unwrap()
    }

    fn assignment(
        worker_id: &str,
        year: i32,
        month: u8,
        day: u8,
        shift_kind: ShiftKind,
    ) -> Assignment {
        Assignment::new(
            WorkerId::new(worker_id).unwrap(),
            PlanningDate::new(year, month, day).unwrap(),
            shift_kind,
        )
    }

    #[test]
    fn generator_builds_monthly_planning_grid() {
        let workers = vec![
            build_worker("worker-01", "Martin", "Alice", "Operateur de production"),
            build_worker("worker-02", "Leroy", "Bruno", "Chef d'equipes"),
        ];
        let assignments = vec![
            assignment("worker-01", 2025, 1, 1, ShiftKind::Night),
            assignment("worker-01", 2025, 1, 3, ShiftKind::Morning),
            assignment("worker-02", 2025, 1, 15, ShiftKind::Day),
            assignment("worker-02", 2025, 1, 31, ShiftKind::Afternoon),
        ];

        let planning = PlanningGenerator::build_month(2025, 1, &workers, &assignments).unwrap();

        assert_eq!(planning.year(), 2025);
        assert_eq!(planning.month(), 1);
        assert_eq!(planning.total_days(), 31);
        assert_eq!(planning.rows().len(), 2);

        let alice = planning.row_for_worker(workers[0].id()).unwrap();
        assert_eq!(alice.worker_name(), "Martin Alice");
        assert_eq!(alice.job_role().label(), "Operateur de production");
        assert_eq!(alice.cells().len(), 31);
        assert_eq!(
            alice.cell_for_day(1).unwrap().style_key(),
            ShiftStyleKey::NightBlue
        );
        assert_eq!(alice.cell_for_day(1).unwrap().shift_label(), "Nuit");
        assert_eq!(alice.cell_for_day(2), None);
        assert_eq!(
            alice.cell_for_day(3).unwrap().shift_kind(),
            ShiftKind::Morning
        );

        let bruno = planning.row_for_worker(workers[1].id()).unwrap();
        assert_eq!(bruno.job_role().label(), "Chef d'equipes");
        assert_eq!(bruno.cell_for_day(15).unwrap().shift_kind(), ShiftKind::Day);
        assert_eq!(
            bruno.cell_for_day(31).unwrap().style_key(),
            ShiftStyleKey::AfternoonRed
        );
    }

    #[test]
    fn generator_supports_leap_day_assignments() {
        let workers = vec![build_worker(
            "worker-01",
            "Martin",
            "Alice",
            "Operateur de salle blanche",
        )];
        let assignments = vec![assignment("worker-01", 2024, 2, 29, ShiftKind::Night)];

        let planning = PlanningGenerator::build_month(2024, 2, &workers, &assignments).unwrap();

        assert_eq!(planning.total_days(), 29);
        let row = planning.row_for_worker(workers[0].id()).unwrap();
        assert_eq!(row.cell_for_day(29).unwrap().shift_kind(), ShiftKind::Night);
    }

    #[test]
    fn generator_rejects_unknown_worker() {
        let workers = vec![build_worker(
            "worker-01",
            "Martin",
            "Alice",
            "Operateur de production",
        )];
        let assignments = vec![assignment("ghost", 2025, 1, 4, ShiftKind::Morning)];

        let error = PlanningGenerator::build_month(2025, 1, &workers, &assignments).unwrap_err();

        assert_eq!(
            error,
            PlanningError::UnknownWorker {
                worker_id: "ghost".to_owned(),
            }
        );
    }

    #[test]
    fn generator_rejects_duplicate_assignment_same_day() {
        let workers = vec![build_worker(
            "worker-01",
            "Martin",
            "Alice",
            "Operateur de production",
        )];
        let assignments = vec![
            assignment("worker-01", 2025, 1, 4, ShiftKind::Morning),
            assignment("worker-01", 2025, 1, 4, ShiftKind::Afternoon),
        ];

        let error = PlanningGenerator::build_month(2025, 1, &workers, &assignments).unwrap_err();

        assert_eq!(
            error,
            PlanningError::WorkerAlreadyAssignedOnDate {
                worker_id: "worker-01".to_owned(),
                date: PlanningDate::new(2025, 1, 4).unwrap(),
            }
        );
    }

    #[test]
    fn generator_rejects_assignment_outside_target_month() {
        let workers = vec![build_worker(
            "worker-01",
            "Martin",
            "Alice",
            "Operateur de production",
        )];
        let assignments = vec![assignment("worker-01", 2025, 2, 1, ShiftKind::Morning)];

        let error = PlanningGenerator::build_month(2025, 1, &workers, &assignments).unwrap_err();

        assert_eq!(
            error,
            PlanningError::AssignmentOutsideTargetMonth {
                expected_year: 2025,
                expected_month: 1,
                date: PlanningDate::new(2025, 2, 1).unwrap(),
            }
        );
    }

    #[test]
    fn generator_rejects_duplicate_worker_ids() {
        let workers = vec![
            build_worker("worker-01", "Martin", "Alice", "Operateur de production"),
            build_worker("worker-01", "Martin", "Alice Bis", "Autre"),
        ];

        let error = PlanningGenerator::build_month(2025, 1, &workers, &[]).unwrap_err();

        assert_eq!(
            error,
            PlanningError::DuplicateWorkerId {
                worker_id: "worker-01".to_owned(),
            }
        );
    }

    #[test]
    fn services_create_and_reload_workers_and_assignments() {
        let database = Rc::new(SqliteDatabase::open_in_memory().unwrap());
        let role_service = JobRoleService::new(database.clone());
        let worker_service = WorkerService::new(database.clone());
        let assignment_service = AssignmentService::new(database.clone());
        let planning_facade =
            PlanningFacade::new(worker_service.clone(), assignment_service.clone());

        role_service.save_role("Chef de ligne").unwrap();
        let worker = worker_service
            .save_worker(None, "Martin", "Alice", role("Chef de ligne"))
            .unwrap();
        assignment_service
            .upsert_assignment(
                worker.id(),
                PlanningDate::new(2026, 4, 8).unwrap(),
                ShiftKind::Night,
            )
            .unwrap();

        let loaded = planning_facade.load_month(2026, 4).unwrap();

        assert_eq!(loaded.workers().len(), 1);
        assert_eq!(loaded.planning().rows().len(), 1);
        assert_eq!(loaded.workers()[0].job_role().label(), "Chef de ligne");
        assert_eq!(
            loaded
                .planning()
                .row_for_worker(worker.id())
                .unwrap()
                .cell_for_day(8)
                .unwrap()
                .style_key(),
            ShiftStyleKey::NightBlue
        );
    }

    #[test]
    fn worker_service_generates_stable_internal_identifier_for_new_person() {
        let database = Rc::new(SqliteDatabase::open_in_memory().unwrap());
        let worker_service = WorkerService::new(database);

        let worker = worker_service
            .save_worker(None, "Martin", "Alice", role("Operateur de production"))
            .unwrap();

        assert!(worker.id().as_str().starts_with("worker-"));
        assert_eq!(worker.display_name(), "Martin Alice");
    }

    #[test]
    fn worker_service_rejects_duplicate_identity_on_create() {
        let database = Rc::new(SqliteDatabase::open_in_memory().unwrap());
        let worker_service = WorkerService::new(database);

        worker_service
            .save_worker(None, "Martin", "Alice", role("Operateur de production"))
            .unwrap();

        let error = worker_service
            .save_worker(None, "Martin", "Alice", role("Chef d'equipes"))
            .unwrap_err();

        assert!(matches!(
            error,
            AppError::DuplicateWorkerIdentity {
                ref last_name,
                ref first_name
            } if last_name == "Martin" && first_name == "Alice"
        ));
    }

    #[test]
    fn worker_service_propagates_delete_guard_when_assignments_exist() {
        let database = Rc::new(SqliteDatabase::open_in_memory().unwrap());
        let worker_service = WorkerService::new(database.clone());
        let assignment_service = AssignmentService::new(database);
        let worker = worker_service
            .save_worker(
                Some("worker-01".to_owned()),
                "Martin",
                "Alice",
                role("Operateur de production"),
            )
            .unwrap();
        assignment_service
            .upsert_assignment(
                worker.id(),
                PlanningDate::new(2026, 4, 8).unwrap(),
                ShiftKind::Night,
            )
            .unwrap();

        let error = worker_service.delete_worker(worker.id()).unwrap_err();

        assert!(matches!(
            error,
            AppError::WorkerHasAssignments { ref worker_id } if worker_id == "worker-01"
        ));
    }
}
