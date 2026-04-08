use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use rusqlite::{Connection, params};

use crate::domain::{Assignment, JobRole, PlanningDate, ShiftKind, Worker, WorkerId};
use crate::error::AppError;

#[derive(Debug)]
pub struct SqliteDatabase {
    connection: Connection,
}

impl SqliteDatabase {
    pub fn open_or_create_default() -> Result<Self, AppError> {
        let project_dirs = ProjectDirs::from("com", "DonJplaning", "DonJplaning")
            .ok_or(AppError::DirectoriesUnavailable)?;
        let data_dir = project_dirs.data_local_dir();
        fs::create_dir_all(data_dir)?;

        Self::open_at(data_dir.join("don_jplaning.db"))
    }

    pub fn open_at(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let path = path.as_ref();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self, AppError> {
        let connection = Connection::open_in_memory()?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self, AppError> {
        let database = Self { connection };
        database.initialize()?;
        Ok(database)
    }

    fn initialize(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS workers (
                id TEXT PRIMARY KEY NOT NULL,
                display_name TEXT NOT NULL,
                job_role TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS assignments (
                worker_id TEXT NOT NULL,
                year INTEGER NOT NULL,
                month INTEGER NOT NULL,
                day INTEGER NOT NULL,
                shift_kind TEXT NOT NULL,
                PRIMARY KEY (worker_id, year, month, day),
                FOREIGN KEY (worker_id) REFERENCES workers(id) ON DELETE RESTRICT
            );
            ",
        )?;

        Ok(())
    }

    pub fn list_workers(&self) -> Result<Vec<Worker>, AppError> {
        let mut statement = self.connection.prepare(
            "
            SELECT id, display_name, job_role
            FROM workers
            ORDER BY display_name COLLATE NOCASE ASC, id ASC
            ",
        )?;
        let mut rows = statement.query([])?;
        let mut workers = Vec::new();

        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let display_name: String = row.get(1)?;
            let job_role_key: String = row.get(2)?;
            let job_role = JobRole::from_storage_key(&job_role_key)
                .ok_or_else(|| AppError::InvalidJobRole(job_role_key.clone()))?;

            let worker = Worker::new(WorkerId::new(id)?, display_name, job_role)?;
            workers.push(worker);
        }

        Ok(workers)
    }

    pub fn upsert_worker(&self, worker: &Worker) -> Result<(), AppError> {
        self.connection.execute(
            "
            INSERT INTO workers (id, display_name, job_role)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                job_role = excluded.job_role
            ",
            params![
                worker.id().as_str(),
                worker.display_name(),
                worker.job_role().storage_key(),
            ],
        )?;

        Ok(())
    }

    pub fn delete_worker(&self, worker_id: &WorkerId) -> Result<(), AppError> {
        let assignment_count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM assignments WHERE worker_id = ?1",
            [worker_id.as_str()],
            |row| row.get(0),
        )?;

        if assignment_count > 0 {
            return Err(AppError::WorkerHasAssignments {
                worker_id: worker_id.to_string(),
            });
        }

        self.connection
            .execute("DELETE FROM workers WHERE id = ?1", [worker_id.as_str()])?;

        Ok(())
    }

    pub fn list_assignments_for_month(
        &self,
        year: i32,
        month: u8,
    ) -> Result<Vec<Assignment>, AppError> {
        let mut statement = self.connection.prepare(
            "
            SELECT worker_id, year, month, day, shift_kind
            FROM assignments
            WHERE year = ?1 AND month = ?2
            ORDER BY worker_id ASC, day ASC
            ",
        )?;
        let mut rows = statement.query(params![year, month])?;
        let mut assignments = Vec::new();

        while let Some(row) = rows.next()? {
            let worker_id: String = row.get(0)?;
            let year: i32 = row.get(1)?;
            let month: u8 = row.get(2)?;
            let day: u8 = row.get(3)?;
            let shift_key: String = row.get(4)?;
            let shift_kind = ShiftKind::from_storage_key(&shift_key)
                .ok_or_else(|| AppError::InvalidShiftKind(shift_key.clone()))?;

            assignments.push(Assignment::new(
                WorkerId::new(worker_id)?,
                PlanningDate::new(year, month, day)?,
                shift_kind,
            ));
        }

        Ok(assignments)
    }

    pub fn upsert_assignment(&self, assignment: &Assignment) -> Result<(), AppError> {
        self.connection.execute(
            "
            INSERT INTO assignments (worker_id, year, month, day, shift_kind)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(worker_id, year, month, day) DO UPDATE SET
                shift_kind = excluded.shift_kind
            ",
            params![
                assignment.worker_id().as_str(),
                assignment.date().year(),
                assignment.date().month(),
                assignment.date().day(),
                assignment.shift_kind().storage_key(),
            ],
        )?;

        Ok(())
    }

    pub fn delete_assignment(
        &self,
        worker_id: &WorkerId,
        date: PlanningDate,
    ) -> Result<(), AppError> {
        self.connection.execute(
            "
            DELETE FROM assignments
            WHERE worker_id = ?1 AND year = ?2 AND month = ?3 AND day = ?4
            ",
            params![worker_id.as_str(), date.year(), date.month(), date.day()],
        )?;

        Ok(())
    }

    pub fn database_path_hint(&self) -> Option<PathBuf> {
        self.connection.path().map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{JobRole, ShiftKind};

    fn worker(worker_id: &str, name: &str, job_role: JobRole) -> Worker {
        Worker::new(WorkerId::new(worker_id).unwrap(), name, job_role).unwrap()
    }

    fn assignment(worker_id: &str, year: i32, month: u8, day: u8, shift: ShiftKind) -> Assignment {
        Assignment::new(
            WorkerId::new(worker_id).unwrap(),
            PlanningDate::new(year, month, day).unwrap(),
            shift,
        )
    }

    #[test]
    fn sqlite_database_starts_with_empty_tables() {
        let database = SqliteDatabase::open_in_memory().unwrap();

        assert!(database.list_workers().unwrap().is_empty());
        assert!(
            database
                .list_assignments_for_month(2026, 4)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn sqlite_database_persists_and_lists_workers() {
        let database = SqliteDatabase::open_in_memory().unwrap();
        database
            .upsert_worker(&worker(
                "worker-01",
                "Alice Martin",
                JobRole::OperateurProduction,
            ))
            .unwrap();
        database
            .upsert_worker(&worker("worker-02", "Bruno Leroy", JobRole::ChefDEquipes))
            .unwrap();

        let workers = database.list_workers().unwrap();

        assert_eq!(workers.len(), 2);
        assert_eq!(workers[0].display_name(), "Alice Martin");
        assert_eq!(workers[1].job_role(), JobRole::ChefDEquipes);
    }

    #[test]
    fn sqlite_database_upserts_assignments_for_a_month() {
        let database = SqliteDatabase::open_in_memory().unwrap();
        database
            .upsert_worker(&worker(
                "worker-01",
                "Alice Martin",
                JobRole::OperateurProduction,
            ))
            .unwrap();

        database
            .upsert_assignment(&assignment("worker-01", 2026, 4, 8, ShiftKind::Night))
            .unwrap();
        database
            .upsert_assignment(&assignment("worker-01", 2026, 4, 8, ShiftKind::Afternoon))
            .unwrap();

        let assignments = database.list_assignments_for_month(2026, 4).unwrap();

        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].shift_kind(), ShiftKind::Afternoon);
    }

    #[test]
    fn sqlite_database_refuses_deleting_worker_with_assignments() {
        let database = SqliteDatabase::open_in_memory().unwrap();
        let worker = worker("worker-01", "Alice Martin", JobRole::OperateurProduction);
        database.upsert_worker(&worker).unwrap();
        database
            .upsert_assignment(&assignment("worker-01", 2026, 4, 8, ShiftKind::Night))
            .unwrap();

        let error = database.delete_worker(worker.id()).unwrap_err();

        assert!(matches!(
            error,
            AppError::WorkerHasAssignments { ref worker_id } if worker_id == "worker-01"
        ));
    }
}
