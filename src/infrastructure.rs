use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use rusqlite::{Connection, OptionalExtension, params};

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
        self.connection
            .execute_batch("PRAGMA foreign_keys = OFF; BEGIN IMMEDIATE;")?;

        let result = (|| {
            self.ensure_job_roles_table()?;
            self.ensure_workers_schema()?;
            self.populate_job_roles_from_workers()?;
            self.ensure_assignments_table()?;
            self.ensure_workers_indexes()?;
            self.seed_default_job_roles()?;
            Ok(())
        })();

        match result {
            Ok(()) => {
                self.connection
                    .execute_batch("COMMIT; PRAGMA foreign_keys = ON;")?;
                Ok(())
            }
            Err(error) => {
                let _ = self
                    .connection
                    .execute_batch("ROLLBACK; PRAGMA foreign_keys = ON;");
                Err(error)
            }
        }
    }

    fn ensure_job_roles_table(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS job_roles (
                name TEXT PRIMARY KEY NOT NULL COLLATE NOCASE
            );
            ",
        )?;

        Ok(())
    }

    fn ensure_workers_schema(&self) -> Result<(), AppError> {
        if !self.table_exists("workers")? {
            self.create_current_workers_table()?;
            return Ok(());
        }

        let columns = self.table_columns("workers")?;

        if Self::is_current_workers_schema(&columns) {
            return Ok(());
        }

        if Self::is_legacy_workers_schema(&columns) {
            self.migrate_legacy_workers_table()?;
            return Ok(());
        }

        Err(AppError::UnsupportedDatabaseSchema)
    }

    fn create_current_workers_table(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS workers (
                id TEXT PRIMARY KEY NOT NULL,
                last_name TEXT NOT NULL,
                first_name TEXT NOT NULL,
                job_role_name TEXT NOT NULL,
                FOREIGN KEY (job_role_name) REFERENCES job_roles(name) ON DELETE RESTRICT
            );
            ",
        )?;

        Ok(())
    }

    fn migrate_legacy_workers_table(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            DROP TABLE IF EXISTS workers_new;

            CREATE TABLE workers_new (
                id TEXT PRIMARY KEY NOT NULL,
                last_name TEXT NOT NULL,
                first_name TEXT NOT NULL,
                job_role_name TEXT NOT NULL,
                FOREIGN KEY (job_role_name) REFERENCES job_roles(name) ON DELETE RESTRICT
            );
            ",
        )?;

        let mut statement = self.connection.prepare(
            "
            SELECT id, display_name, job_role
            FROM workers
            ORDER BY rowid ASC
            ",
        )?;
        let mut rows = statement.query([])?;

        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let display_name: String = row.get(1)?;
            let legacy_job_role: String = row.get(2)?;

            let (last_name, first_name) = split_legacy_display_name(&display_name);
            let job_role = JobRole::from_legacy_storage_key(&legacy_job_role)
                .map(Ok)
                .unwrap_or_else(|| JobRole::new(legacy_job_role.clone()))
                .map_err(|_| AppError::InvalidJobRole(legacy_job_role.clone()))?;
            let worker = Worker::new(WorkerId::new(id)?, last_name, first_name, job_role)?;

            self.upsert_job_role(worker.job_role())?;
            self.connection.execute(
                "
                INSERT INTO workers_new (id, last_name, first_name, job_role_name)
                VALUES (?1, ?2, ?3, ?4)
                ",
                params![
                    worker.id().as_str(),
                    worker.last_name(),
                    worker.first_name(),
                    worker.job_role().label(),
                ],
            )?;
        }

        drop(rows);
        drop(statement);

        self.connection.execute_batch(
            "
            DROP TABLE workers;
            ALTER TABLE workers_new RENAME TO workers;
            ",
        )?;

        Ok(())
    }

    fn populate_job_roles_from_workers(&self) -> Result<(), AppError> {
        if !self.table_exists("workers")? {
            return Ok(());
        }

        self.connection.execute(
            "
            INSERT OR IGNORE INTO job_roles (name)
            SELECT DISTINCT job_role_name
            FROM workers
            WHERE TRIM(job_role_name) <> ''
            ",
            [],
        )?;

        Ok(())
    }

    fn ensure_assignments_table(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
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

    fn ensure_workers_indexes(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            CREATE INDEX IF NOT EXISTS idx_workers_name
            ON workers(last_name COLLATE NOCASE, first_name COLLATE NOCASE);

            CREATE INDEX IF NOT EXISTS idx_workers_job_role
            ON workers(job_role_name COLLATE NOCASE);
            ",
        )?;

        Ok(())
    }

    fn seed_default_job_roles(&self) -> Result<(), AppError> {
        for role in JobRole::default_roles() {
            self.upsert_job_role(&role)?;
        }

        Ok(())
    }

    fn table_exists(&self, table_name: &str) -> Result<bool, AppError> {
        let exists: Option<i64> = self
            .connection
            .query_row(
                "
                SELECT 1
                FROM sqlite_master
                WHERE type = 'table' AND name = ?1
                LIMIT 1
                ",
                [table_name],
                |row| row.get(0),
            )
            .optional()?;

        Ok(exists.is_some())
    }

    fn table_columns(&self, table_name: &str) -> Result<Vec<String>, AppError> {
        let mut statement = self
            .connection
            .prepare(&format!("PRAGMA table_info({table_name})"))?;
        let mut rows = statement.query([])?;
        let mut columns = Vec::new();

        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            columns.push(name);
        }

        Ok(columns)
    }

    fn is_current_workers_schema(columns: &[String]) -> bool {
        columns.iter().any(|column| column == "last_name")
            && columns.iter().any(|column| column == "first_name")
            && columns.iter().any(|column| column == "job_role_name")
    }

    fn is_legacy_workers_schema(columns: &[String]) -> bool {
        columns.iter().any(|column| column == "display_name")
            && columns.iter().any(|column| column == "job_role")
    }

    pub fn list_job_roles(&self) -> Result<Vec<JobRole>, AppError> {
        let mut statement = self.connection.prepare(
            "
            SELECT name
            FROM job_roles
            ORDER BY name COLLATE NOCASE ASC
            ",
        )?;
        let mut rows = statement.query([])?;
        let mut roles = Vec::new();

        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            roles.push(JobRole::new(name)?);
        }

        Ok(roles)
    }

    pub fn upsert_job_role(&self, job_role: &JobRole) -> Result<(), AppError> {
        self.connection.execute(
            "
            INSERT OR IGNORE INTO job_roles (name)
            VALUES (?1)
            ",
            [job_role.label()],
        )?;

        Ok(())
    }

    pub fn generate_worker_id(&self) -> Result<WorkerId, AppError> {
        loop {
            let suffix: String =
                self.connection
                    .query_row("SELECT lower(hex(randomblob(8)))", [], |row| row.get(0))?;
            let candidate = format!("worker-{suffix}");

            let already_exists: i64 = self.connection.query_row(
                "SELECT COUNT(*) FROM workers WHERE id = ?1",
                [&candidate],
                |row| row.get(0),
            )?;

            if already_exists == 0 {
                return Ok(WorkerId::new(candidate)?);
            }
        }
    }

    pub fn find_worker_id_by_identity(
        &self,
        last_name: &str,
        first_name: &str,
    ) -> Result<Option<WorkerId>, AppError> {
        let worker_id: Option<String> = self
            .connection
            .query_row(
                "
                SELECT id
                FROM workers
                WHERE lower(last_name) = lower(?1)
                  AND lower(first_name) = lower(?2)
                LIMIT 1
                ",
                params![last_name, first_name],
                |row| row.get(0),
            )
            .optional()?;

        worker_id
            .map(WorkerId::new)
            .transpose()
            .map_err(AppError::from)
    }

    pub fn list_workers(&self) -> Result<Vec<Worker>, AppError> {
        let mut statement = self.connection.prepare(
            "
            SELECT id, last_name, first_name, job_role_name
            FROM workers
            ORDER BY last_name COLLATE NOCASE ASC, first_name COLLATE NOCASE ASC, id ASC
            ",
        )?;
        let mut rows = statement.query([])?;
        let mut workers = Vec::new();

        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let last_name: String = row.get(1)?;
            let first_name: String = row.get(2)?;
            let job_role_name: String = row.get(3)?;
            let job_role = JobRole::from_storage_value(&job_role_name)?;

            let worker = Worker::new(WorkerId::new(id)?, last_name, first_name, job_role)?;
            workers.push(worker);
        }

        Ok(workers)
    }

    pub fn upsert_worker(&self, worker: &Worker) -> Result<(), AppError> {
        self.upsert_job_role(worker.job_role())?;
        self.connection.execute(
            "
            INSERT INTO workers (id, last_name, first_name, job_role_name)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(id) DO UPDATE SET
                last_name = excluded.last_name,
                first_name = excluded.first_name,
                job_role_name = excluded.job_role_name
            ",
            params![
                worker.id().as_str(),
                worker.last_name(),
                worker.first_name(),
                worker.job_role().label(),
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

fn split_legacy_display_name(display_name: &str) -> (String, String) {
    let parts: Vec<&str> = display_name.split_whitespace().collect();

    match parts.as_slice() {
        [] => ("Sans nom".to_owned(), "Inconnu".to_owned()),
        [single] => ((*single).to_owned(), "Inconnu".to_owned()),
        [last_name, first_name @ ..] => ((*last_name).to_owned(), first_name.join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(label: &str) -> JobRole {
        JobRole::new(label).unwrap()
    }

    fn worker(worker_id: &str, last_name: &str, first_name: &str, job_role: &str) -> Worker {
        Worker::new(
            WorkerId::new(worker_id).unwrap(),
            last_name,
            first_name,
            role(job_role),
        )
        .unwrap()
    }

    fn assignment(worker_id: &str, year: i32, month: u8, day: u8, shift: ShiftKind) -> Assignment {
        Assignment::new(
            WorkerId::new(worker_id).unwrap(),
            PlanningDate::new(year, month, day).unwrap(),
            shift,
        )
    }

    #[test]
    fn sqlite_database_starts_with_empty_tables_and_default_roles() {
        let database = SqliteDatabase::open_in_memory().unwrap();

        assert!(database.list_workers().unwrap().is_empty());
        assert!(
            database
                .list_assignments_for_month(2026, 4)
                .unwrap()
                .is_empty()
        );
        assert_eq!(database.list_job_roles().unwrap().len(), 4);
    }

    #[test]
    fn sqlite_database_persists_and_lists_workers() {
        let database = SqliteDatabase::open_in_memory().unwrap();
        database
            .upsert_worker(&worker(
                "worker-01",
                "Martin",
                "Alice",
                "Operateur de production",
            ))
            .unwrap();
        database
            .upsert_worker(&worker("worker-02", "Leroy", "Bruno", "Chef d'equipes"))
            .unwrap();

        let workers = database.list_workers().unwrap();

        assert_eq!(workers.len(), 2);
        assert_eq!(workers[0].display_name(), "Leroy Bruno");
        assert_eq!(workers[1].job_role().label(), "Operateur de production");
    }

    #[test]
    fn sqlite_database_persists_custom_job_roles() {
        let database = SqliteDatabase::open_in_memory().unwrap();
        database
            .upsert_job_role(&role("Conducteur de ligne"))
            .unwrap();

        let roles = database.list_job_roles().unwrap();

        assert!(
            roles
                .iter()
                .any(|entry| entry.label() == "Conducteur de ligne")
        );
    }

    #[test]
    fn sqlite_database_upserts_assignments_for_a_month() {
        let database = SqliteDatabase::open_in_memory().unwrap();
        database
            .upsert_worker(&worker(
                "worker-01",
                "Martin",
                "Alice",
                "Operateur de production",
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
        let worker = worker("worker-01", "Martin", "Alice", "Operateur de production");
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

    #[test]
    fn sqlite_database_generates_unique_worker_identifiers() {
        let database = SqliteDatabase::open_in_memory().unwrap();

        let first = database.generate_worker_id().unwrap();
        let second = database.generate_worker_id().unwrap();

        assert!(first.as_str().starts_with("worker-"));
        assert!(second.as_str().starts_with("worker-"));
        assert_ne!(first, second);
    }

    #[test]
    fn sqlite_database_finds_existing_identity_case_insensitively() {
        let database = SqliteDatabase::open_in_memory().unwrap();
        let worker = worker("worker-01", "Martin", "Alice", "Operateur de production");
        database.upsert_worker(&worker).unwrap();

        let found = database
            .find_worker_id_by_identity("martin", "alice")
            .unwrap();

        assert_eq!(found, Some(worker.id().clone()));
    }

    #[test]
    fn sqlite_database_migrates_legacy_workers_schema() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "
                PRAGMA foreign_keys = OFF;

                CREATE TABLE workers (
                    id TEXT PRIMARY KEY NOT NULL,
                    display_name TEXT NOT NULL,
                    job_role TEXT NOT NULL
                );

                CREATE TABLE assignments (
                    worker_id TEXT NOT NULL,
                    year INTEGER NOT NULL,
                    month INTEGER NOT NULL,
                    day INTEGER NOT NULL,
                    shift_kind TEXT NOT NULL,
                    PRIMARY KEY (worker_id, year, month, day),
                    FOREIGN KEY (worker_id) REFERENCES workers(id) ON DELETE RESTRICT
                );

                INSERT INTO workers (id, display_name, job_role)
                VALUES ('worker-01', 'Glachant Bryan', 'chef_d_equipes');

                INSERT INTO assignments (worker_id, year, month, day, shift_kind)
                VALUES ('worker-01', 2026, 4, 8, 'night');
                ",
            )
            .unwrap();

        let database = SqliteDatabase::from_connection(connection).unwrap();

        let workers = database.list_workers().unwrap();
        let assignments = database.list_assignments_for_month(2026, 4).unwrap();
        let roles = database.list_job_roles().unwrap();

        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].last_name(), "Glachant");
        assert_eq!(workers[0].first_name(), "Bryan");
        assert_eq!(workers[0].job_role().label(), "Chef d'equipes");
        assert_eq!(assignments.len(), 1);
        assert!(roles.iter().any(|role| role.label() == "Chef d'equipes"));
    }

    #[test]
    fn legacy_display_name_split_keeps_first_token_as_last_name() {
        assert_eq!(
            split_legacy_display_name("Glachant Bryan"),
            ("Glachant".to_owned(), "Bryan".to_owned())
        );
        assert_eq!(
            split_legacy_display_name("Dupont Jean Pierre"),
            ("Dupont".to_owned(), "Jean Pierre".to_owned())
        );
        assert_eq!(
            split_legacy_display_name("Mononyme"),
            ("Mononyme".to_owned(), "Inconnu".to_owned())
        );
    }
}
