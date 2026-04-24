use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;
use directories::ProjectDirs;
use rusqlite::{Connection, OptionalExtension, params};

use crate::domain::{
    ClockTime, DEFAULT_SHIFT_SLOT_ID_AFTERNOON, DEFAULT_SHIFT_SLOT_ID_DAY,
    DEFAULT_SHIFT_SLOT_ID_MORNING, DEFAULT_SHIFT_SLOT_ID_NIGHT, GeneratedAssignment, JobRole,
    ManualOverride, ManualOverrideKind, PlanningDate, RotationCycle, ShiftSlot, ShiftSlotId,
    ShiftVisualStyle, Team, TeamId, TeamMemberRole, TeamMembership, Worker, WorkerId,
};
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

    pub(crate) fn from_connection(connection: Connection) -> Result<Self, AppError> {
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
            self.ensure_shift_slots_table()?;
            self.ensure_planning_settings_table()?;
            self.ensure_rotation_cycle_table()?;
            self.ensure_teams_table()?;
            self.ensure_team_members_table()?;
            self.ensure_generated_assignments_table()?;
            self.ensure_manual_overrides_table()?;
            self.ensure_workers_indexes()?;
            self.ensure_planning_indexes()?;
            self.seed_default_job_roles()?;
            self.seed_default_shift_slots()?;
            self.seed_default_rotation_cycle()?;
            self.seed_default_teams()?;
            self.migrate_legacy_assignments_table()?;
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

    fn ensure_shift_slots_table(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS shift_slots (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL COLLATE NOCASE,
                short_code TEXT NOT NULL UNIQUE COLLATE NOCASE,
                start_hour INTEGER NOT NULL,
                start_minute INTEGER NOT NULL,
                end_hour INTEGER NOT NULL,
                end_minute INTEGER NOT NULL,
                visual_style TEXT NOT NULL,
                sort_order INTEGER NOT NULL,
                active INTEGER NOT NULL DEFAULT 1
            );
            ",
        )?;

        Ok(())
    }

    fn ensure_planning_settings_table(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS planning_settings (
                id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
                reference_week_start TEXT NOT NULL
            );
            ",
        )?;

        Ok(())
    }

    fn ensure_rotation_cycle_table(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS rotation_cycle_slots (
                position INTEGER PRIMARY KEY NOT NULL,
                shift_slot_id TEXT NOT NULL,
                FOREIGN KEY (shift_slot_id) REFERENCES shift_slots(id) ON DELETE RESTRICT
            );
            ",
        )?;

        Ok(())
    }

    fn ensure_teams_table(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS teams (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                anchor_shift_slot_id TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                FOREIGN KEY (anchor_shift_slot_id) REFERENCES shift_slots(id) ON DELETE RESTRICT
            );
            ",
        )?;

        Ok(())
    }

    fn ensure_team_members_table(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS team_members (
                team_id TEXT NOT NULL,
                worker_id TEXT NOT NULL UNIQUE,
                membership_role TEXT NOT NULL,
                PRIMARY KEY (team_id, worker_id),
                FOREIGN KEY (team_id) REFERENCES teams(id) ON DELETE CASCADE,
                FOREIGN KEY (worker_id) REFERENCES workers(id) ON DELETE RESTRICT
            );
            ",
        )?;

        Ok(())
    }

    fn ensure_generated_assignments_table(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS generated_assignments (
                worker_id TEXT NOT NULL,
                date TEXT NOT NULL,
                shift_slot_id TEXT NOT NULL,
                PRIMARY KEY (worker_id, date),
                FOREIGN KEY (worker_id) REFERENCES workers(id) ON DELETE RESTRICT,
                FOREIGN KEY (shift_slot_id) REFERENCES shift_slots(id) ON DELETE RESTRICT
            );
            ",
        )?;

        Ok(())
    }

    fn ensure_manual_overrides_table(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS manual_overrides (
                worker_id TEXT NOT NULL,
                date TEXT NOT NULL,
                override_kind TEXT NOT NULL,
                shift_slot_id TEXT,
                PRIMARY KEY (worker_id, date),
                FOREIGN KEY (worker_id) REFERENCES workers(id) ON DELETE RESTRICT,
                FOREIGN KEY (shift_slot_id) REFERENCES shift_slots(id) ON DELETE RESTRICT
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

            CREATE INDEX IF NOT EXISTS idx_shift_slots_order
            ON shift_slots(sort_order, name COLLATE NOCASE);

            CREATE INDEX IF NOT EXISTS idx_teams_name
            ON teams(name COLLATE NOCASE);

            CREATE INDEX IF NOT EXISTS idx_team_members_team
            ON team_members(team_id, membership_role);

            CREATE INDEX IF NOT EXISTS idx_generated_assignments_date
            ON generated_assignments(date);

            CREATE INDEX IF NOT EXISTS idx_manual_overrides_date
            ON manual_overrides(date);
            ",
        )?;

        Ok(())
    }

    fn ensure_planning_indexes(&self) -> Result<(), AppError> {
        self.connection.execute_batch(
            "
            CREATE INDEX IF NOT EXISTS idx_generated_assignments_shift
            ON generated_assignments(shift_slot_id);

            CREATE INDEX IF NOT EXISTS idx_manual_overrides_shift
            ON manual_overrides(shift_slot_id);
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

    fn seed_default_shift_slots(&self) -> Result<(), AppError> {
        for shift_slot in ShiftSlot::default_slots() {
            self.upsert_shift_slot(&shift_slot)?;
        }

        Ok(())
    }

    fn seed_default_rotation_cycle(&self) -> Result<(), AppError> {
        let settings_exists: Option<i64> = self
            .connection
            .query_row(
                "SELECT 1 FROM planning_settings WHERE id = 1 LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        if settings_exists.is_none() {
            let today = Local::now().date_naive();
            let reference_week_start =
                PlanningDate::from_naive_date(today)?.start_of_week_monday()?;

            self.connection.execute(
                "
                INSERT INTO planning_settings (id, reference_week_start)
                VALUES (1, ?1)
                ",
                [reference_week_start.to_string()],
            )?;
        }

        let cycle_count: i64 =
            self.connection
                .query_row("SELECT COUNT(*) FROM rotation_cycle_slots", [], |row| {
                    row.get(0)
                })?;

        if cycle_count == 0 {
            let rotation_cycle = ShiftSlot::default_rotation_order();

            for (index, shift_slot_id) in rotation_cycle.iter().enumerate() {
                self.connection.execute(
                    "
                    INSERT INTO rotation_cycle_slots (position, shift_slot_id)
                    VALUES (?1, ?2)
                    ",
                    params![index as i64, shift_slot_id.as_str()],
                )?;
            }
        }

        Ok(())
    }

    fn seed_default_teams(&self) -> Result<(), AppError> {
        let team_count: i64 =
            self.connection
                .query_row("SELECT COUNT(*) FROM teams", [], |row| row.get(0))?;

        if team_count > 0 {
            return Ok(());
        }

        let defaults = vec![
            Team::new(
                TeamId::new("team-a")?,
                "Equipe A",
                ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_AFTERNOON)?,
                true,
            )?,
            Team::new(
                TeamId::new("team-b")?,
                "Equipe B",
                ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_MORNING)?,
                true,
            )?,
            Team::new(
                TeamId::new("team-c")?,
                "Equipe C",
                ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_NIGHT)?,
                true,
            )?,
        ];

        for team in defaults {
            self.upsert_team(&team)?;
        }

        Ok(())
    }

    fn migrate_legacy_assignments_table(&self) -> Result<(), AppError> {
        if !self.table_exists("assignments")? {
            return Ok(());
        }

        let columns = self.table_columns("assignments")?;
        let is_legacy_assignments = ["worker_id", "year", "month", "day", "shift_kind"]
            .iter()
            .all(|expected| columns.iter().any(|column| column == expected));

        if !is_legacy_assignments {
            return Err(AppError::UnsupportedDatabaseSchema);
        }

        let mut statement = self.connection.prepare(
            "
            SELECT worker_id, year, month, day, shift_kind
            FROM assignments
            ORDER BY worker_id ASC, year ASC, month ASC, day ASC
            ",
        )?;
        let mut rows = statement.query([])?;

        while let Some(row) = rows.next()? {
            let worker_id: String = row.get(0)?;
            let year: i32 = row.get(1)?;
            let month: u8 = row.get(2)?;
            let day: u8 = row.get(3)?;
            let shift_kind: String = row.get(4)?;
            let shift_slot_id = match shift_kind.as_str() {
                "morning" => DEFAULT_SHIFT_SLOT_ID_MORNING,
                "afternoon" => DEFAULT_SHIFT_SLOT_ID_AFTERNOON,
                "night" => DEFAULT_SHIFT_SLOT_ID_NIGHT,
                "day" => DEFAULT_SHIFT_SLOT_ID_DAY,
                _ => return Err(AppError::InvalidShiftSlot(shift_kind)),
            };

            let manual_override = ManualOverride::assignment(
                WorkerId::new(worker_id)?,
                PlanningDate::new(year, month, day)?,
                ShiftSlotId::new(shift_slot_id)?,
            );
            self.upsert_manual_override(&manual_override)?;
        }

        drop(rows);
        drop(statement);

        self.connection.execute_batch("DROP TABLE assignments;")?;
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
        let worker_id = self.generate_unique_text_identifier("workers", "id", "worker")?;
        Ok(WorkerId::new(worker_id)?)
    }

    pub fn generate_shift_slot_id(&self) -> Result<ShiftSlotId, AppError> {
        let shift_slot_id = self.generate_unique_text_identifier("shift_slots", "id", "shift")?;
        Ok(ShiftSlotId::new(shift_slot_id)?)
    }

    pub fn generate_team_id(&self) -> Result<TeamId, AppError> {
        let team_id = self.generate_unique_text_identifier("teams", "id", "team")?;
        Ok(TeamId::new(team_id)?)
    }

    fn generate_unique_text_identifier(
        &self,
        table_name: &str,
        column_name: &str,
        prefix: &str,
    ) -> Result<String, AppError> {
        loop {
            let suffix: String =
                self.connection
                    .query_row("SELECT lower(hex(randomblob(8)))", [], |row| row.get(0))?;
            let candidate = format!("{prefix}-{suffix}");

            let query = format!("SELECT COUNT(*) FROM {table_name} WHERE {column_name} = ?1");
            let already_exists: i64 = self
                .connection
                .query_row(&query, [&candidate], |row| row.get(0))?;

            if already_exists == 0 {
                return Ok(candidate);
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

            workers.push(Worker::new(
                WorkerId::new(id)?,
                last_name,
                first_name,
                job_role,
            )?);
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
        let team_member_count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM team_members WHERE worker_id = ?1",
            [worker_id.as_str()],
            |row| row.get(0),
        )?;
        let generated_count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM generated_assignments WHERE worker_id = ?1",
            [worker_id.as_str()],
            |row| row.get(0),
        )?;
        let manual_count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM manual_overrides WHERE worker_id = ?1",
            [worker_id.as_str()],
            |row| row.get(0),
        )?;

        if team_member_count > 0 || generated_count > 0 || manual_count > 0 {
            return Err(AppError::WorkerHasPlanningLinks {
                worker_id: worker_id.to_string(),
            });
        }

        self.connection
            .execute("DELETE FROM workers WHERE id = ?1", [worker_id.as_str()])?;

        Ok(())
    }

    pub fn find_shift_slot_id_by_code(
        &self,
        short_code: &str,
    ) -> Result<Option<ShiftSlotId>, AppError> {
        let shift_slot_id: Option<String> = self
            .connection
            .query_row(
                "
                SELECT id
                FROM shift_slots
                WHERE lower(short_code) = lower(?1)
                LIMIT 1
                ",
                [short_code],
                |row| row.get(0),
            )
            .optional()?;

        shift_slot_id
            .map(ShiftSlotId::new)
            .transpose()
            .map_err(AppError::from)
    }

    pub fn list_shift_slots(&self) -> Result<Vec<ShiftSlot>, AppError> {
        let mut statement = self.connection.prepare(
            "
            SELECT id, name, short_code, start_hour, start_minute, end_hour, end_minute, visual_style, sort_order, active
            FROM shift_slots
            ORDER BY sort_order ASC, name COLLATE NOCASE ASC, id ASC
            ",
        )?;
        let mut rows = statement.query([])?;
        let mut shift_slots = Vec::new();

        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let short_code: String = row.get(2)?;
            let start_hour: u8 = row.get(3)?;
            let start_minute: u8 = row.get(4)?;
            let end_hour: u8 = row.get(5)?;
            let end_minute: u8 = row.get(6)?;
            let visual_style: String = row.get(7)?;
            let sort_order: i32 = row.get(8)?;
            let active: i64 = row.get(9)?;

            let visual_style = ShiftVisualStyle::from_storage_key(&visual_style)
                .ok_or_else(|| AppError::InvalidShiftStyle(visual_style.clone()))?;
            shift_slots.push(ShiftSlot::new(
                ShiftSlotId::new(id)?,
                name,
                short_code,
                ClockTime::new(start_hour, start_minute)?,
                ClockTime::new(end_hour, end_minute)?,
                visual_style,
                sort_order,
                sqlite_bool_to_bool(active),
            )?);
        }

        Ok(shift_slots)
    }

    pub fn upsert_shift_slot(&self, shift_slot: &ShiftSlot) -> Result<(), AppError> {
        self.connection.execute(
            "
            INSERT INTO shift_slots (
                id, name, short_code, start_hour, start_minute, end_hour, end_minute, visual_style, sort_order, active
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                short_code = excluded.short_code,
                start_hour = excluded.start_hour,
                start_minute = excluded.start_minute,
                end_hour = excluded.end_hour,
                end_minute = excluded.end_minute,
                visual_style = excluded.visual_style,
                sort_order = excluded.sort_order,
                active = excluded.active
            ",
            params![
                shift_slot.id().as_str(),
                shift_slot.name(),
                shift_slot.short_code(),
                shift_slot.start_time().hour(),
                shift_slot.start_time().minute(),
                shift_slot.end_time().hour(),
                shift_slot.end_time().minute(),
                shift_slot.visual_style().storage_key(),
                shift_slot.sort_order(),
                bool_to_sqlite(shift_slot.active()),
            ],
        )?;

        Ok(())
    }

    pub fn load_rotation_cycle(&self) -> Result<RotationCycle, AppError> {
        let reference_week_start: String = self.connection.query_row(
            "
            SELECT reference_week_start
            FROM planning_settings
            WHERE id = 1
            ",
            [],
            |row| row.get(0),
        )?;

        let mut statement = self.connection.prepare(
            "
            SELECT shift_slot_id
            FROM rotation_cycle_slots
            ORDER BY position ASC
            ",
        )?;
        let mut rows = statement.query([])?;
        let mut ordered_shift_slot_ids = Vec::new();

        while let Some(row) = rows.next()? {
            let shift_slot_id: String = row.get(0)?;
            ordered_shift_slot_ids.push(ShiftSlotId::new(shift_slot_id)?);
        }

        RotationCycle::new(
            PlanningDate::parse_iso(&reference_week_start)?,
            ordered_shift_slot_ids,
        )
        .map_err(AppError::from)
    }

    pub fn save_rotation_cycle(&self, rotation_cycle: &RotationCycle) -> Result<(), AppError> {
        self.connection.execute_batch("BEGIN IMMEDIATE;")?;

        let result = (|| {
            self.connection.execute(
                "
                INSERT INTO planning_settings (id, reference_week_start)
                VALUES (1, ?1)
                ON CONFLICT(id) DO UPDATE SET
                    reference_week_start = excluded.reference_week_start
                ",
                [rotation_cycle.reference_week_start().to_string()],
            )?;

            self.connection
                .execute("DELETE FROM rotation_cycle_slots", [])?;

            for (position, shift_slot_id) in
                rotation_cycle.ordered_shift_slot_ids().iter().enumerate()
            {
                self.connection.execute(
                    "
                    INSERT INTO rotation_cycle_slots (position, shift_slot_id)
                    VALUES (?1, ?2)
                    ",
                    params![position as i64, shift_slot_id.as_str()],
                )?;
            }

            Ok(())
        })();

        match result {
            Ok(()) => {
                self.connection.execute_batch("COMMIT;")?;
                Ok(())
            }
            Err(error) => {
                let _ = self.connection.execute_batch("ROLLBACK;");
                Err(error)
            }
        }
    }

    pub fn find_team_id_by_name(&self, name: &str) -> Result<Option<TeamId>, AppError> {
        let team_id: Option<String> = self
            .connection
            .query_row(
                "
                SELECT id
                FROM teams
                WHERE lower(name) = lower(?1)
                LIMIT 1
                ",
                [name],
                |row| row.get(0),
            )
            .optional()?;

        team_id.map(TeamId::new).transpose().map_err(AppError::from)
    }

    pub fn list_teams(&self) -> Result<Vec<Team>, AppError> {
        let mut statement = self.connection.prepare(
            "
            SELECT id, name, anchor_shift_slot_id, active
            FROM teams
            ORDER BY name COLLATE NOCASE ASC, id ASC
            ",
        )?;
        let mut rows = statement.query([])?;
        let mut teams = Vec::new();

        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let anchor_shift_slot_id: String = row.get(2)?;
            let active: i64 = row.get(3)?;

            teams.push(Team::new(
                TeamId::new(id)?,
                name,
                ShiftSlotId::new(anchor_shift_slot_id)?,
                sqlite_bool_to_bool(active),
            )?);
        }

        Ok(teams)
    }

    pub fn upsert_team(&self, team: &Team) -> Result<(), AppError> {
        self.connection.execute(
            "
            INSERT INTO teams (id, name, anchor_shift_slot_id, active)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                anchor_shift_slot_id = excluded.anchor_shift_slot_id,
                active = excluded.active
            ",
            params![
                team.id().as_str(),
                team.name(),
                team.anchor_shift_slot_id().as_str(),
                bool_to_sqlite(team.active()),
            ],
        )?;

        Ok(())
    }

    pub fn list_team_memberships(&self) -> Result<Vec<TeamMembership>, AppError> {
        let mut statement = self.connection.prepare(
            "
            SELECT team_id, worker_id, membership_role
            FROM team_members
            ORDER BY team_id ASC,
                     CASE membership_role WHEN 'leader' THEN 0 ELSE 1 END ASC,
                     worker_id ASC
            ",
        )?;
        let mut rows = statement.query([])?;
        let mut memberships = Vec::new();

        while let Some(row) = rows.next()? {
            let team_id: String = row.get(0)?;
            let worker_id: String = row.get(1)?;
            let membership_role: String = row.get(2)?;
            let membership_role =
                TeamMemberRole::from_storage_key(&membership_role).ok_or_else(|| {
                    AppError::InconsistentDatabase(format!(
                        "role de membre d'equipe inconnu: {membership_role}"
                    ))
                })?;

            memberships.push(TeamMembership::new(
                TeamId::new(team_id)?,
                WorkerId::new(worker_id)?,
                membership_role,
            ));
        }

        Ok(memberships)
    }

    pub fn find_team_membership_by_worker(
        &self,
        worker_id: &WorkerId,
    ) -> Result<Option<TeamId>, AppError> {
        let team_id: Option<String> = self
            .connection
            .query_row(
                "
                SELECT team_id
                FROM team_members
                WHERE worker_id = ?1
                LIMIT 1
                ",
                [worker_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;

        team_id.map(TeamId::new).transpose().map_err(AppError::from)
    }

    pub fn upsert_team_membership(&self, membership: &TeamMembership) -> Result<(), AppError> {
        self.connection.execute(
            "
            INSERT INTO team_members (team_id, worker_id, membership_role)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(team_id, worker_id) DO UPDATE SET
                membership_role = excluded.membership_role
            ",
            params![
                membership.team_id().as_str(),
                membership.worker_id().as_str(),
                membership.role().storage_key(),
            ],
        )?;

        Ok(())
    }

    pub fn delete_team_membership(
        &self,
        team_id: &TeamId,
        worker_id: &WorkerId,
    ) -> Result<(), AppError> {
        self.connection.execute(
            "
            DELETE FROM team_members
            WHERE team_id = ?1 AND worker_id = ?2
            ",
            params![team_id.as_str(), worker_id.as_str()],
        )?;

        Ok(())
    }

    pub fn list_generated_assignments_in_range(
        &self,
        start_date: PlanningDate,
        total_days: u32,
    ) -> Result<Vec<GeneratedAssignment>, AppError> {
        let end_date = end_date_from_range(start_date, total_days)?;
        let mut statement = self.connection.prepare(
            "
            SELECT worker_id, date, shift_slot_id
            FROM generated_assignments
            WHERE date >= ?1 AND date <= ?2
            ORDER BY worker_id ASC, date ASC
            ",
        )?;
        let mut rows = statement.query(params![start_date.to_string(), end_date.to_string()])?;
        let mut assignments = Vec::new();

        while let Some(row) = rows.next()? {
            let worker_id: String = row.get(0)?;
            let date: String = row.get(1)?;
            let shift_slot_id: String = row.get(2)?;

            assignments.push(GeneratedAssignment::new(
                WorkerId::new(worker_id)?,
                PlanningDate::parse_iso(&date)?,
                ShiftSlotId::new(shift_slot_id)?,
            ));
        }

        Ok(assignments)
    }

    pub fn replace_generated_assignments_in_range(
        &self,
        start_date: PlanningDate,
        total_days: u32,
        assignments: &[GeneratedAssignment],
    ) -> Result<(), AppError> {
        let end_date = end_date_from_range(start_date, total_days)?;
        self.connection.execute_batch("BEGIN IMMEDIATE;")?;

        let result = (|| {
            self.connection.execute(
                "
                DELETE FROM generated_assignments
                WHERE date >= ?1 AND date <= ?2
                ",
                params![start_date.to_string(), end_date.to_string()],
            )?;

            for assignment in assignments {
                self.connection.execute(
                    "
                    INSERT INTO generated_assignments (worker_id, date, shift_slot_id)
                    VALUES (?1, ?2, ?3)
                    ON CONFLICT(worker_id, date) DO UPDATE SET
                        shift_slot_id = excluded.shift_slot_id
                    ",
                    params![
                        assignment.worker_id().as_str(),
                        assignment.date().to_string(),
                        assignment.shift_slot_id().as_str(),
                    ],
                )?;
            }

            Ok(())
        })();

        match result {
            Ok(()) => {
                self.connection.execute_batch("COMMIT;")?;
                Ok(())
            }
            Err(error) => {
                let _ = self.connection.execute_batch("ROLLBACK;");
                Err(error)
            }
        }
    }

    pub fn list_manual_overrides_in_range(
        &self,
        start_date: PlanningDate,
        total_days: u32,
    ) -> Result<Vec<ManualOverride>, AppError> {
        let end_date = end_date_from_range(start_date, total_days)?;
        let mut statement = self.connection.prepare(
            "
            SELECT worker_id, date, override_kind, shift_slot_id
            FROM manual_overrides
            WHERE date >= ?1 AND date <= ?2
            ORDER BY worker_id ASC, date ASC
            ",
        )?;
        let mut rows = statement.query(params![start_date.to_string(), end_date.to_string()])?;
        let mut overrides = Vec::new();

        while let Some(row) = rows.next()? {
            let worker_id: String = row.get(0)?;
            let date: String = row.get(1)?;
            let override_kind: String = row.get(2)?;
            let shift_slot_id: Option<String> = row.get(3)?;
            let worker_id = WorkerId::new(worker_id)?;
            let date = PlanningDate::parse_iso(&date)?;
            let override_kind =
                ManualOverrideKind::from_storage_key(&override_kind).ok_or_else(|| {
                    AppError::InconsistentDatabase(format!(
                        "type de correction manuelle inconnu: {override_kind}"
                    ))
                })?;

            let manual_override = match override_kind {
                ManualOverrideKind::Assignment => ManualOverride::assignment(
                    worker_id,
                    date,
                    ShiftSlotId::new(shift_slot_id.ok_or_else(|| {
                        AppError::InconsistentDatabase(
                            "une correction manuelle d'affectation n'a pas de plage associee"
                                .to_owned(),
                        )
                    })?)?,
                ),
                ManualOverrideKind::Off => ManualOverride::off(worker_id, date),
            };

            overrides.push(manual_override);
        }

        Ok(overrides)
    }

    pub fn upsert_manual_override(&self, manual_override: &ManualOverride) -> Result<(), AppError> {
        manual_override.validate()?;
        self.connection.execute(
            "
            INSERT INTO manual_overrides (worker_id, date, override_kind, shift_slot_id)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(worker_id, date) DO UPDATE SET
                override_kind = excluded.override_kind,
                shift_slot_id = excluded.shift_slot_id
            ",
            params![
                manual_override.worker_id().as_str(),
                manual_override.date().to_string(),
                manual_override.kind().storage_key(),
                manual_override.shift_slot_id().map(|value| value.as_str()),
            ],
        )?;

        Ok(())
    }

    pub fn delete_manual_override(
        &self,
        worker_id: &WorkerId,
        date: PlanningDate,
    ) -> Result<(), AppError> {
        self.connection.execute(
            "
            DELETE FROM manual_overrides
            WHERE worker_id = ?1 AND date = ?2
            ",
            params![worker_id.as_str(), date.to_string()],
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

fn bool_to_sqlite(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn sqlite_bool_to_bool(value: i64) -> bool {
    value != 0
}

fn end_date_from_range(
    start_date: PlanningDate,
    total_days: u32,
) -> Result<PlanningDate, AppError> {
    if total_days == 0 {
        return Err(crate::domain::PlanningError::InvalidGenerationDays { days: 0 }.into());
    }

    start_date
        .add_days(total_days as i64 - 1)
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::PlanningService;
    use crate::domain::{DEFAULT_SHIFT_SLOT_ID_AFTERNOON, PlanningCell};

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

    #[test]
    fn sqlite_database_starts_with_default_configuration() {
        let database = SqliteDatabase::open_in_memory().unwrap();

        let roles = database.list_job_roles().unwrap();
        let shift_slots = database.list_shift_slots().unwrap();
        let teams = database.list_teams().unwrap();
        let rotation = database.load_rotation_cycle().unwrap();

        assert_eq!(roles.len(), 4);
        assert_eq!(shift_slots.len(), 4);
        assert_eq!(teams.len(), 3);
        assert_eq!(rotation.ordered_shift_slot_ids().len(), 3);
        assert!(teams.iter().any(|team| team.name() == "Equipe A"));
    }

    #[test]
    fn sqlite_database_persists_workers_shift_slots_teams_and_memberships() {
        let database = SqliteDatabase::open_in_memory().unwrap();
        let team_id = database
            .list_teams()
            .unwrap()
            .into_iter()
            .find(|team| team.name() == "Equipe A")
            .unwrap()
            .id()
            .clone();

        let worker = worker("worker-01", "Martin", "Alice", "Chef d'equipes");
        database.upsert_worker(&worker).unwrap();
        database
            .upsert_team_membership(&TeamMembership::new(
                team_id.clone(),
                worker.id().clone(),
                TeamMemberRole::Leader,
            ))
            .unwrap();

        let memberships = database.list_team_memberships().unwrap();

        assert_eq!(database.list_workers().unwrap().len(), 1);
        assert_eq!(memberships.len(), 1);
        assert_eq!(memberships[0].team_id(), &team_id);
        assert_eq!(memberships[0].role(), TeamMemberRole::Leader);
    }

    #[test]
    fn sqlite_database_replaces_generated_assignments_only_in_requested_range() {
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
            .replace_generated_assignments_in_range(
                PlanningDate::new(2026, 1, 1).unwrap(),
                2,
                &[
                    GeneratedAssignment::new(
                        WorkerId::new("worker-01").unwrap(),
                        PlanningDate::new(2026, 1, 1).unwrap(),
                        ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_AFTERNOON).unwrap(),
                    ),
                    GeneratedAssignment::new(
                        WorkerId::new("worker-01").unwrap(),
                        PlanningDate::new(2026, 1, 2).unwrap(),
                        ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_AFTERNOON).unwrap(),
                    ),
                ],
            )
            .unwrap();

        database
            .replace_generated_assignments_in_range(
                PlanningDate::new(2026, 1, 2).unwrap(),
                1,
                &[GeneratedAssignment::new(
                    WorkerId::new("worker-01").unwrap(),
                    PlanningDate::new(2026, 1, 2).unwrap(),
                    ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_NIGHT).unwrap(),
                )],
            )
            .unwrap();

        let assignments = database
            .list_generated_assignments_in_range(PlanningDate::new(2026, 1, 1).unwrap(), 2)
            .unwrap();

        assert_eq!(assignments.len(), 2);
        assert_eq!(
            assignments[0].shift_slot_id().as_str(),
            DEFAULT_SHIFT_SLOT_ID_AFTERNOON
        );
        assert_eq!(
            assignments[1].shift_slot_id().as_str(),
            DEFAULT_SHIFT_SLOT_ID_NIGHT
        );
    }

    #[test]
    fn sqlite_database_migrates_legacy_assignments_into_manual_overrides() {
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
        let manual_overrides = database
            .list_manual_overrides_in_range(PlanningDate::new(2026, 4, 1).unwrap(), 30)
            .unwrap();

        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].last_name(), "Glachant");
        assert_eq!(workers[0].first_name(), "Bryan");
        assert_eq!(manual_overrides.len(), 1);
        assert_eq!(
            manual_overrides[0].shift_slot_id().unwrap().as_str(),
            DEFAULT_SHIFT_SLOT_ID_NIGHT
        );
    }

    #[test]
    fn sqlite_database_prevents_worker_deletion_when_planning_data_exists() {
        let database = SqliteDatabase::open_in_memory().unwrap();
        let worker = worker("worker-01", "Martin", "Alice", "Operateur de production");
        database.upsert_worker(&worker).unwrap();
        database
            .upsert_manual_override(&ManualOverride::off(
                worker.id().clone(),
                PlanningDate::new(2026, 4, 23).unwrap(),
            ))
            .unwrap();

        let error = database.delete_worker(worker.id()).unwrap_err();

        assert!(matches!(
            error,
            AppError::WorkerHasPlanningLinks { ref worker_id } if worker_id == "worker-01"
        ));
    }

    #[test]
    fn integration_with_planning_service_loads_generated_cells() {
        let database = std::rc::Rc::new(SqliteDatabase::open_in_memory().unwrap());
        let planning_service = PlanningService::new(database.clone());

        database
            .upsert_worker(&worker(
                "worker-01",
                "Martin",
                "Alice",
                "Operateur de production",
            ))
            .unwrap();
        database
            .replace_generated_assignments_in_range(
                PlanningDate::new(2026, 4, 20).unwrap(),
                1,
                &[GeneratedAssignment::new(
                    WorkerId::new("worker-01").unwrap(),
                    PlanningDate::new(2026, 4, 20).unwrap(),
                    ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_AFTERNOON).unwrap(),
                )],
            )
            .unwrap();

        let loaded = planning_service
            .load_range(PlanningDate::new(2026, 4, 20).unwrap(), 1)
            .unwrap();
        let row = loaded
            .planning()
            .row_for_worker(&WorkerId::new("worker-01").unwrap())
            .unwrap();

        assert!(matches!(
            row.cell_for_offset(0).unwrap(),
            PlanningCell::Assignment { .. }
        ));
    }
}
