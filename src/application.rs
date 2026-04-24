use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};
use std::rc::Rc;

use crate::domain::{
    AssignmentOrigin, GeneratedAssignment, JobRole, ManualOverride, ManualOverrideKind,
    PlanningCell, PlanningDate, PlanningError, PlanningRow, RangePlanning, RotationCycle,
    ShiftSlot, ShiftSlotId, Team, TeamId, TeamMemberRole, TeamMembership, Worker, WorkerId,
};
use crate::error::AppError;
use crate::infrastructure::SqliteDatabase;

#[derive(Debug, Default, Clone, Copy)]
pub struct PlanningGenerator;

impl PlanningGenerator {
    pub fn generate_assignments(
        workers: &[Worker],
        shift_slots: &[ShiftSlot],
        teams: &[Team],
        memberships: &[TeamMembership],
        rotation_cycle: &RotationCycle,
        start_date: PlanningDate,
        total_days: u32,
    ) -> Result<Vec<GeneratedAssignment>, PlanningError> {
        Self::validate_generation_days(total_days)?;
        let workers_by_id = Self::index_workers(workers)?;
        let shift_slots_by_id = Self::index_shift_slots(shift_slots)?;
        let teams_by_id = Self::index_teams(teams)?;
        let active_teams = teams
            .iter()
            .filter(|team| team.active())
            .cloned()
            .collect::<Vec<Team>>();

        if active_teams.len() != rotation_cycle.ordered_shift_slot_ids().len() {
            return Err(PlanningError::ActiveTeamsDoNotMatchRotationSlots {
                expected_teams: rotation_cycle.ordered_shift_slot_ids().len(),
                actual_teams: active_teams.len(),
            });
        }

        let mut anchor_shift_ids = BTreeSet::new();

        for team in &active_teams {
            if !shift_slots_by_id.contains_key(team.anchor_shift_slot_id()) {
                return Err(PlanningError::UnknownShiftSlot {
                    shift_slot_id: team.anchor_shift_slot_id().to_string(),
                });
            }

            if !rotation_cycle
                .ordered_shift_slot_ids()
                .iter()
                .any(|shift_slot_id| shift_slot_id == team.anchor_shift_slot_id())
            {
                return Err(PlanningError::TeamAnchorShiftNotInRotation {
                    team_id: team.id().to_string(),
                    shift_slot_id: team.anchor_shift_slot_id().to_string(),
                });
            }

            if !anchor_shift_ids.insert(team.anchor_shift_slot_id().clone()) {
                return Err(PlanningError::DuplicateTeamAnchorShift {
                    shift_slot_id: team.anchor_shift_slot_id().to_string(),
                });
            }
        }

        let memberships_by_team =
            Self::index_memberships(&teams_by_id, &workers_by_id, memberships, &active_teams)?;

        let mut assignments = Vec::new();

        for day_offset in 0..total_days {
            let date = start_date.add_days(day_offset as i64)?;

            for team in &active_teams {
                let shift_slot_id = rotation_cycle.shift_for(team.anchor_shift_slot_id(), date)?;
                let Some(team_members) = memberships_by_team.get(team.id()) else {
                    return Err(PlanningError::UnknownTeam {
                        team_id: team.id().to_string(),
                    });
                };

                for membership in team_members {
                    assignments.push(GeneratedAssignment::new(
                        membership.worker_id().clone(),
                        date,
                        shift_slot_id.clone(),
                    ));
                }
            }
        }

        Ok(assignments)
    }

    pub fn build_range(
        workers: &[Worker],
        shift_slots: &[ShiftSlot],
        teams: &[Team],
        memberships: &[TeamMembership],
        generated_assignments: &[GeneratedAssignment],
        manual_overrides: &[ManualOverride],
        start_date: PlanningDate,
        total_days: u32,
    ) -> Result<RangePlanning, PlanningError> {
        Self::validate_generation_days(total_days)?;
        let shift_slots_by_id = Self::index_shift_slots(shift_slots)?;
        let teams_by_id = Self::index_teams(teams)?;
        let team_memberships_by_worker =
            Self::index_memberships_by_worker(&teams_by_id, memberships)?;
        let workers_by_id = Self::index_workers(workers)?;

        let mut generated_by_worker_and_date = BTreeMap::new();

        for assignment in generated_assignments {
            if !workers_by_id.contains_key(assignment.worker_id()) {
                return Err(PlanningError::UnknownWorker {
                    worker_id: assignment.worker_id().to_string(),
                });
            }

            if !shift_slots_by_id.contains_key(assignment.shift_slot_id()) {
                return Err(PlanningError::UnknownShiftSlot {
                    shift_slot_id: assignment.shift_slot_id().to_string(),
                });
            }

            generated_by_worker_and_date.insert(
                (assignment.worker_id().clone(), assignment.date()),
                assignment.shift_slot_id().clone(),
            );
        }

        let mut manual_by_worker_and_date = BTreeMap::new();

        for manual_override in manual_overrides {
            manual_override.validate()?;

            if !workers_by_id.contains_key(manual_override.worker_id()) {
                return Err(PlanningError::UnknownWorker {
                    worker_id: manual_override.worker_id().to_string(),
                });
            }

            if let Some(shift_slot_id) = manual_override.shift_slot_id() {
                if !shift_slots_by_id.contains_key(shift_slot_id) {
                    return Err(PlanningError::UnknownShiftSlot {
                        shift_slot_id: shift_slot_id.to_string(),
                    });
                }
            }

            manual_by_worker_and_date.insert(
                (manual_override.worker_id().clone(), manual_override.date()),
                manual_override.clone(),
            );
        }

        let mut workers_sorted = workers.to_vec();
        workers_sorted.sort_by(|left, right| {
            let left_meta = team_memberships_by_worker.get(left.id());
            let right_meta = team_memberships_by_worker.get(right.id());

            let left_team = left_meta
                .map(|entry| entry.0.as_str())
                .unwrap_or("Sans equipe");
            let right_team = right_meta
                .map(|entry| entry.0.as_str())
                .unwrap_or("Sans equipe");

            left_team
                .cmp(right_team)
                .then_with(|| {
                    let left_rank = left_meta.map(|entry| role_rank(entry.1)).unwrap_or(9);
                    let right_rank = right_meta.map(|entry| role_rank(entry.1)).unwrap_or(9);
                    left_rank.cmp(&right_rank)
                })
                .then_with(|| left.last_name().cmp(right.last_name()))
                .then_with(|| left.first_name().cmp(right.first_name()))
                .then_with(|| left.id().as_str().cmp(right.id().as_str()))
        });

        let mut rows = Vec::with_capacity(workers_sorted.len());

        for worker in &workers_sorted {
            let (team_name, team_role_label) = team_memberships_by_worker
                .get(worker.id())
                .map(|(team_name, role)| (team_name.to_owned(), role.label().to_owned()))
                .unwrap_or_else(|| ("Sans equipe".to_owned(), String::new()));

            let mut cells = Vec::with_capacity(total_days as usize);

            for offset in 0..total_days {
                let date = start_date.add_days(offset as i64)?;
                let key = (worker.id().clone(), date);

                if let Some(manual_override) = manual_by_worker_and_date.get(&key) {
                    let cell = match manual_override.kind() {
                        ManualOverrideKind::Assignment => {
                            let shift_slot_id =
                                manual_override.shift_slot_id().ok_or_else(|| {
                                    PlanningError::ManualOverrideMissingShift {
                                        worker_id: manual_override.worker_id().to_string(),
                                        date,
                                    }
                                })?;
                            let shift_slot = shift_slots_by_id
                                .get(shift_slot_id)
                                .ok_or_else(|| PlanningError::UnknownShiftSlot {
                                    shift_slot_id: shift_slot_id.to_string(),
                                })?
                                .clone();

                            PlanningCell::Assignment {
                                date,
                                shift_slot,
                                origin: AssignmentOrigin::Manual,
                            }
                        }
                        ManualOverrideKind::Off => PlanningCell::ManualOff { date },
                    };

                    cells.push(cell);
                    continue;
                }

                if let Some(shift_slot_id) = generated_by_worker_and_date.get(&key) {
                    let shift_slot = shift_slots_by_id
                        .get(shift_slot_id)
                        .ok_or_else(|| PlanningError::UnknownShiftSlot {
                            shift_slot_id: shift_slot_id.to_string(),
                        })?
                        .clone();

                    cells.push(PlanningCell::Assignment {
                        date,
                        shift_slot,
                        origin: AssignmentOrigin::Generated,
                    });
                    continue;
                }

                cells.push(PlanningCell::Empty { date });
            }

            rows.push(PlanningRow::new(worker, team_name, team_role_label, cells));
        }

        Ok(RangePlanning::new(start_date, total_days, rows))
    }

    fn validate_generation_days(total_days: u32) -> Result<(), PlanningError> {
        if total_days == 0 {
            return Err(PlanningError::InvalidGenerationDays { days: total_days });
        }

        Ok(())
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

    fn index_shift_slots(
        shift_slots: &[ShiftSlot],
    ) -> Result<BTreeMap<ShiftSlotId, ShiftSlot>, PlanningError> {
        let mut shift_slots_by_id = BTreeMap::new();
        let mut seen_codes = BTreeSet::new();

        for shift_slot in shift_slots {
            if !seen_codes.insert(shift_slot.short_code().to_owned()) {
                return Err(PlanningError::DuplicateShiftSlotCode {
                    short_code: shift_slot.short_code().to_owned(),
                });
            }

            match shift_slots_by_id.entry(shift_slot.id().clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(shift_slot.clone());
                }
                Entry::Occupied(_) => {
                    return Err(PlanningError::DuplicateShiftSlotId {
                        shift_slot_id: shift_slot.id().to_string(),
                    });
                }
            }
        }

        Ok(shift_slots_by_id)
    }

    fn index_teams(teams: &[Team]) -> Result<BTreeMap<TeamId, Team>, PlanningError> {
        let mut teams_by_id = BTreeMap::new();

        for team in teams {
            match teams_by_id.entry(team.id().clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(team.clone());
                }
                Entry::Occupied(_) => {
                    return Err(PlanningError::DuplicateTeamId {
                        team_id: team.id().to_string(),
                    });
                }
            }
        }

        Ok(teams_by_id)
    }

    fn index_memberships<'a>(
        teams_by_id: &BTreeMap<TeamId, Team>,
        workers_by_id: &BTreeMap<WorkerId, Worker>,
        memberships: &'a [TeamMembership],
        active_teams: &[Team],
    ) -> Result<BTreeMap<TeamId, Vec<&'a TeamMembership>>, PlanningError> {
        let mut memberships_by_team: BTreeMap<TeamId, Vec<&'a TeamMembership>> = BTreeMap::new();
        let active_team_ids = active_teams
            .iter()
            .map(|team| team.id().clone())
            .collect::<BTreeSet<TeamId>>();
        let mut worker_assignment_to_team = BTreeMap::new();

        for membership in memberships {
            if !teams_by_id.contains_key(membership.team_id()) {
                return Err(PlanningError::UnknownTeam {
                    team_id: membership.team_id().to_string(),
                });
            }

            if !workers_by_id.contains_key(membership.worker_id()) {
                return Err(PlanningError::UnknownWorker {
                    worker_id: membership.worker_id().to_string(),
                });
            }

            if active_team_ids.contains(membership.team_id()) {
                match worker_assignment_to_team.entry(membership.worker_id().clone()) {
                    Entry::Vacant(entry) => {
                        entry.insert(membership.team_id().clone());
                    }
                    Entry::Occupied(existing_team_id) => {
                        if existing_team_id.get() != membership.team_id() {
                            return Err(PlanningError::DuplicateWorkerAcrossTeams {
                                worker_id: membership.worker_id().to_string(),
                            });
                        }
                    }
                }
            }

            memberships_by_team
                .entry(membership.team_id().clone())
                .or_default()
                .push(membership);
        }

        for team in active_teams {
            let team_members = memberships_by_team
                .get(team.id())
                .cloned()
                .unwrap_or_default();

            let leader_count = team_members
                .iter()
                .filter(|membership| membership.role() == TeamMemberRole::Leader)
                .count();

            if leader_count == 0 {
                return Err(PlanningError::TeamMissingLeader {
                    team_id: team.id().to_string(),
                });
            }

            if leader_count > 1 {
                return Err(PlanningError::TeamHasMultipleLeaders {
                    team_id: team.id().to_string(),
                });
            }

            let operator_count = team_members
                .iter()
                .filter(|membership| membership.role() == TeamMemberRole::Operator)
                .count();

            if operator_count == 0 {
                return Err(PlanningError::TeamMissingOperator {
                    team_id: team.id().to_string(),
                });
            }
        }

        Ok(memberships_by_team)
    }

    fn index_memberships_by_worker(
        teams_by_id: &BTreeMap<TeamId, Team>,
        memberships: &[TeamMembership],
    ) -> Result<BTreeMap<WorkerId, (String, TeamMemberRole)>, PlanningError> {
        let mut by_worker = BTreeMap::new();

        for membership in memberships {
            let team = teams_by_id.get(membership.team_id()).ok_or_else(|| {
                PlanningError::UnknownTeam {
                    team_id: membership.team_id().to_string(),
                }
            })?;

            by_worker.insert(
                membership.worker_id().clone(),
                (team.name().to_owned(), membership.role()),
            );
        }

        Ok(by_worker)
    }
}

fn role_rank(role: TeamMemberRole) -> u8 {
    match role {
        TeamMemberRole::Leader => 0,
        TeamMemberRole::Operator => 1,
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
pub struct ShiftSlotService {
    database: Rc<SqliteDatabase>,
}

impl ShiftSlotService {
    pub fn new(database: Rc<SqliteDatabase>) -> Self {
        Self { database }
    }

    pub fn list_all(&self) -> Result<Vec<ShiftSlot>, AppError> {
        self.database.list_shift_slots()
    }

    pub fn list_active(&self) -> Result<Vec<ShiftSlot>, AppError> {
        Ok(self
            .database
            .list_shift_slots()?
            .into_iter()
            .filter(|shift_slot| shift_slot.active())
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_shift_slot(
        &self,
        shift_slot_id: Option<String>,
        name: impl Into<String>,
        short_code: impl Into<String>,
        start_hour: u8,
        start_minute: u8,
        end_hour: u8,
        end_minute: u8,
        visual_style: crate::domain::ShiftVisualStyle,
        active: bool,
    ) -> Result<ShiftSlot, AppError> {
        let short_code = short_code.into();
        let shift_slot_id = match shift_slot_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(existing_id) => ShiftSlotId::new(existing_id)?,
            None => self.database.generate_shift_slot_id()?,
        };
        let start_time = crate::domain::ClockTime::new(start_hour, start_minute)?;
        let end_time = crate::domain::ClockTime::new(end_hour, end_minute)?;
        let sort_order = i32::from(start_time.minutes_since_midnight());

        let shift_slot = ShiftSlot::new(
            shift_slot_id,
            name,
            short_code.clone(),
            start_time,
            end_time,
            visual_style,
            sort_order,
            active,
        )?;

        if let Some(existing_shift_slot_id) =
            self.database.find_shift_slot_id_by_code(&short_code)?
        {
            if existing_shift_slot_id != shift_slot.id().clone() {
                return Err(AppError::DuplicateShiftSlotCode {
                    short_code: shift_slot.short_code().to_owned(),
                });
            }
        }

        self.database.upsert_shift_slot(&shift_slot)?;
        Ok(shift_slot)
    }
}

#[derive(Debug, Clone)]
pub struct TeamService {
    database: Rc<SqliteDatabase>,
}

impl TeamService {
    pub fn new(database: Rc<SqliteDatabase>) -> Self {
        Self { database }
    }

    pub fn list_teams(&self) -> Result<Vec<Team>, AppError> {
        self.database.list_teams()
    }

    pub fn list_memberships(&self) -> Result<Vec<TeamMembership>, AppError> {
        self.database.list_team_memberships()
    }

    pub fn save_team(
        &self,
        team_id: Option<String>,
        name: impl Into<String>,
        anchor_shift_slot_id: ShiftSlotId,
        active: bool,
    ) -> Result<Team, AppError> {
        let team_name = name.into();
        let team_id = match team_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(existing_id) => TeamId::new(existing_id)?,
            None => self.database.generate_team_id()?,
        };

        let team = Team::new(team_id, team_name, anchor_shift_slot_id, active)?;

        if let Some(existing_team_id) = self.database.find_team_id_by_name(team.name())? {
            if existing_team_id != team.id().clone() {
                return Err(AppError::DuplicateTeamName {
                    team_name: team.name().to_owned(),
                });
            }
        }

        self.database.upsert_team(&team)?;
        Ok(team)
    }

    pub fn upsert_member(
        &self,
        team_id: &TeamId,
        worker_id: &WorkerId,
        role: TeamMemberRole,
    ) -> Result<TeamMembership, AppError> {
        let teams = self.database.list_teams()?;
        let memberships = self.database.list_team_memberships()?;
        let team = teams
            .iter()
            .find(|team| team.id() == team_id)
            .ok_or_else(|| {
                AppError::Planning(PlanningError::UnknownTeam {
                    team_id: team_id.to_string(),
                })
            })?;

        if let Some(existing_team_id) = self.database.find_team_membership_by_worker(worker_id)? {
            if existing_team_id != team_id.clone() {
                let existing_team_name = teams
                    .iter()
                    .find(|candidate| candidate.id() == &existing_team_id)
                    .map(|candidate| candidate.name().to_owned())
                    .unwrap_or_else(|| existing_team_id.to_string());

                return Err(AppError::WorkerAlreadyAssignedToTeam {
                    worker_id: worker_id.to_string(),
                    team_name: existing_team_name,
                });
            }
        }

        if role == TeamMemberRole::Leader {
            if let Some(existing_leader) = memberships.iter().find(|membership| {
                membership.team_id() == team_id
                    && membership.role() == TeamMemberRole::Leader
                    && membership.worker_id() != worker_id
            }) {
                let _ = existing_leader;
                return Err(AppError::TeamAlreadyHasLeader {
                    team_name: team.name().to_owned(),
                });
            }
        }

        let membership = TeamMembership::new(team_id.clone(), worker_id.clone(), role);
        self.database.upsert_team_membership(&membership)?;
        Ok(membership)
    }

    pub fn remove_member(&self, team_id: &TeamId, worker_id: &WorkerId) -> Result<(), AppError> {
        self.database.delete_team_membership(team_id, worker_id)
    }
}

#[derive(Debug, Clone)]
pub struct PlanningService {
    database: Rc<SqliteDatabase>,
}

impl PlanningService {
    pub fn new(database: Rc<SqliteDatabase>) -> Self {
        Self { database }
    }

    pub fn load_rotation_cycle(&self) -> Result<RotationCycle, AppError> {
        self.database.load_rotation_cycle()
    }

    pub fn save_rotation_cycle(
        &self,
        reference_week_start: PlanningDate,
        ordered_shift_slot_ids: Vec<ShiftSlotId>,
    ) -> Result<RotationCycle, AppError> {
        let rotation_cycle = RotationCycle::new(reference_week_start, ordered_shift_slot_ids)?;
        self.database.save_rotation_cycle(&rotation_cycle)?;
        Ok(rotation_cycle)
    }

    pub fn generate_range(
        &self,
        start_date: PlanningDate,
        total_days: u32,
    ) -> Result<GenerationReport, AppError> {
        let workers = self.database.list_workers()?;
        let shift_slots = self.database.list_shift_slots()?;
        let teams = self.database.list_teams()?;
        let memberships = self.database.list_team_memberships()?;
        let rotation_cycle = self.database.load_rotation_cycle()?;
        let generated_assignments = PlanningGenerator::generate_assignments(
            &workers,
            &shift_slots,
            &teams,
            &memberships,
            &rotation_cycle,
            start_date,
            total_days,
        )?;

        self.database.replace_generated_assignments_in_range(
            start_date,
            total_days,
            &generated_assignments,
        )?;

        let active_team_count = teams.iter().filter(|team| team.active()).count();
        let planned_worker_count = memberships
            .iter()
            .map(|membership| membership.worker_id().clone())
            .collect::<BTreeSet<WorkerId>>()
            .len();

        Ok(GenerationReport {
            start_date,
            total_days,
            generated_assignment_count: generated_assignments.len(),
            active_team_count,
            planned_worker_count,
        })
    }

    pub fn load_range(
        &self,
        start_date: PlanningDate,
        total_days: u32,
    ) -> Result<LoadedPlanningRange, AppError> {
        let workers = self.database.list_workers()?;
        let shift_slots = self.database.list_shift_slots()?;
        let teams = self.database.list_teams()?;
        let team_memberships = self.database.list_team_memberships()?;
        let rotation_cycle = self.database.load_rotation_cycle()?;
        let generated_assignments = self
            .database
            .list_generated_assignments_in_range(start_date, total_days)?;
        let manual_overrides = self
            .database
            .list_manual_overrides_in_range(start_date, total_days)?;
        let planning = PlanningGenerator::build_range(
            &workers,
            &shift_slots,
            &teams,
            &team_memberships,
            &generated_assignments,
            &manual_overrides,
            start_date,
            total_days,
        )?;

        Ok(LoadedPlanningRange {
            workers,
            shift_slots,
            teams,
            team_memberships,
            rotation_cycle,
            planning,
            generated_assignment_count: generated_assignments.len(),
            manual_override_count: manual_overrides.len(),
        })
    }

    pub fn save_manual_assignment(
        &self,
        worker_id: &WorkerId,
        date: PlanningDate,
        shift_slot_id: &ShiftSlotId,
    ) -> Result<(), AppError> {
        let manual_override =
            ManualOverride::assignment(worker_id.clone(), date, shift_slot_id.clone());
        self.database.upsert_manual_override(&manual_override)
    }

    pub fn mark_manual_off(
        &self,
        worker_id: &WorkerId,
        date: PlanningDate,
    ) -> Result<(), AppError> {
        let manual_override = ManualOverride::off(worker_id.clone(), date);
        self.database.upsert_manual_override(&manual_override)
    }

    pub fn clear_manual_override(
        &self,
        worker_id: &WorkerId,
        date: PlanningDate,
    ) -> Result<(), AppError> {
        self.database.delete_manual_override(worker_id, date)
    }
}

#[derive(Debug, Clone)]
pub struct GenerationReport {
    start_date: PlanningDate,
    total_days: u32,
    generated_assignment_count: usize,
    active_team_count: usize,
    planned_worker_count: usize,
}

impl GenerationReport {
    pub fn start_date(&self) -> PlanningDate {
        self.start_date
    }

    pub const fn total_days(&self) -> u32 {
        self.total_days
    }

    pub const fn generated_assignment_count(&self) -> usize {
        self.generated_assignment_count
    }

    pub const fn active_team_count(&self) -> usize {
        self.active_team_count
    }

    pub const fn planned_worker_count(&self) -> usize {
        self.planned_worker_count
    }
}

#[derive(Debug, Clone)]
pub struct LoadedPlanningRange {
    workers: Vec<Worker>,
    shift_slots: Vec<ShiftSlot>,
    teams: Vec<Team>,
    team_memberships: Vec<TeamMembership>,
    rotation_cycle: RotationCycle,
    planning: RangePlanning,
    generated_assignment_count: usize,
    manual_override_count: usize,
}

impl LoadedPlanningRange {
    pub fn workers(&self) -> &[Worker] {
        &self.workers
    }

    pub fn shift_slots(&self) -> &[ShiftSlot] {
        &self.shift_slots
    }

    pub fn teams(&self) -> &[Team] {
        &self.teams
    }

    pub fn team_memberships(&self) -> &[TeamMembership] {
        &self.team_memberships
    }

    pub fn rotation_cycle(&self) -> &RotationCycle {
        &self.rotation_cycle
    }

    pub fn planning(&self) -> &RangePlanning {
        &self.planning
    }

    pub const fn generated_assignment_count(&self) -> usize {
        self.generated_assignment_count
    }

    pub const fn manual_override_count(&self) -> usize {
        self.manual_override_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        DEFAULT_SHIFT_SLOT_ID_AFTERNOON, DEFAULT_SHIFT_SLOT_ID_MORNING,
        DEFAULT_SHIFT_SLOT_ID_NIGHT, ShiftVisualStyle,
    };
    use crate::infrastructure::SqliteDatabase;

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

    fn team(team_id: &str, name: &str, anchor_shift: &str) -> Team {
        Team::new(
            TeamId::new(team_id).unwrap(),
            name,
            ShiftSlotId::new(anchor_shift).unwrap(),
            true,
        )
        .unwrap()
    }

    fn membership(team_id: &str, worker_id: &str, role: TeamMemberRole) -> TeamMembership {
        TeamMembership::new(
            TeamId::new(team_id).unwrap(),
            WorkerId::new(worker_id).unwrap(),
            role,
        )
    }

    #[test]
    fn generator_assigns_every_team_member_for_each_day_of_the_range() {
        let workers = vec![
            worker("worker-a1", "Martin", "Alice", "Chef d'equipes"),
            worker("worker-a2", "Durand", "Luc", "Operateur de production"),
            worker("worker-b1", "Leroy", "Emma", "Chef d'equipes"),
            worker("worker-b2", "Petit", "Noe", "Operateur de production"),
            worker("worker-c1", "Renard", "Lea", "Chef d'equipes"),
            worker("worker-c2", "Lopez", "Ilyes", "Operateur de production"),
        ];
        let shift_slots = ShiftSlot::default_slots();
        let teams = vec![
            team("team-a", "Equipe A", DEFAULT_SHIFT_SLOT_ID_AFTERNOON),
            team("team-b", "Equipe B", DEFAULT_SHIFT_SLOT_ID_MORNING),
            team("team-c", "Equipe C", DEFAULT_SHIFT_SLOT_ID_NIGHT),
        ];
        let memberships = vec![
            membership("team-a", "worker-a1", TeamMemberRole::Leader),
            membership("team-a", "worker-a2", TeamMemberRole::Operator),
            membership("team-b", "worker-b1", TeamMemberRole::Leader),
            membership("team-b", "worker-b2", TeamMemberRole::Operator),
            membership("team-c", "worker-c1", TeamMemberRole::Leader),
            membership("team-c", "worker-c2", TeamMemberRole::Operator),
        ];

        let generated = PlanningGenerator::generate_assignments(
            &workers,
            &shift_slots,
            &teams,
            &memberships,
            &RotationCycle::default(PlanningDate::new(2026, 1, 5).unwrap()),
            PlanningDate::new(2026, 1, 5).unwrap(),
            14,
        )
        .unwrap();

        assert_eq!(generated.len(), 6 * 14);

        let first_week_assignment = generated
            .iter()
            .find(|assignment| {
                assignment.worker_id().as_str() == "worker-a1"
                    && assignment.date() == PlanningDate::new(2026, 1, 5).unwrap()
            })
            .unwrap();
        let second_week_assignment = generated
            .iter()
            .find(|assignment| {
                assignment.worker_id().as_str() == "worker-a1"
                    && assignment.date() == PlanningDate::new(2026, 1, 12).unwrap()
            })
            .unwrap();

        assert_eq!(
            first_week_assignment.shift_slot_id().as_str(),
            DEFAULT_SHIFT_SLOT_ID_AFTERNOON
        );
        assert_eq!(
            second_week_assignment.shift_slot_id().as_str(),
            DEFAULT_SHIFT_SLOT_ID_MORNING
        );
    }

    #[test]
    fn generator_rejects_when_active_team_count_does_not_match_three_shifts() {
        let workers = vec![
            worker("worker-a1", "Martin", "Alice", "Chef d'equipes"),
            worker("worker-a2", "Durand", "Luc", "Operateur de production"),
            worker("worker-b1", "Leroy", "Emma", "Chef d'equipes"),
            worker("worker-b2", "Petit", "Noe", "Operateur de production"),
        ];
        let shift_slots = ShiftSlot::default_slots();
        let teams = vec![
            team("team-a", "Equipe A", DEFAULT_SHIFT_SLOT_ID_AFTERNOON),
            team("team-b", "Equipe B", DEFAULT_SHIFT_SLOT_ID_MORNING),
        ];
        let memberships = vec![
            membership("team-a", "worker-a1", TeamMemberRole::Leader),
            membership("team-a", "worker-a2", TeamMemberRole::Operator),
            membership("team-b", "worker-b1", TeamMemberRole::Leader),
            membership("team-b", "worker-b2", TeamMemberRole::Operator),
        ];

        let error = PlanningGenerator::generate_assignments(
            &workers,
            &shift_slots,
            &teams,
            &memberships,
            &RotationCycle::default(PlanningDate::new(2026, 1, 5).unwrap()),
            PlanningDate::new(2026, 1, 5).unwrap(),
            7,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            PlanningError::ActiveTeamsDoNotMatchRotationSlots {
                expected_teams: 3,
                actual_teams: 2
            }
        ));
    }

    #[test]
    fn planning_service_overlays_manual_override_over_generated_schedule() {
        let database = Rc::new(SqliteDatabase::open_in_memory().unwrap());
        let role_service = JobRoleService::new(database.clone());
        let worker_service = WorkerService::new(database.clone());
        let team_service = TeamService::new(database.clone());
        let planning_service = PlanningService::new(database.clone());

        role_service.save_role("Chef d'equipes").unwrap();
        role_service.save_role("Operateur de production").unwrap();

        let leader_a = worker_service
            .save_worker(None, "Martin", "Alice", role("Chef d'equipes"))
            .unwrap();
        let operator_a = worker_service
            .save_worker(None, "Durand", "Luc", role("Operateur de production"))
            .unwrap();
        let leader_b = worker_service
            .save_worker(None, "Leroy", "Emma", role("Chef d'equipes"))
            .unwrap();
        let operator_b = worker_service
            .save_worker(None, "Petit", "Noe", role("Operateur de production"))
            .unwrap();
        let leader_c = worker_service
            .save_worker(None, "Renard", "Lea", role("Chef d'equipes"))
            .unwrap();
        let operator_c = worker_service
            .save_worker(None, "Lopez", "Ilyes", role("Operateur de production"))
            .unwrap();

        let teams = team_service.list_teams().unwrap();
        let team_a = teams.iter().find(|team| team.name() == "Equipe A").unwrap();
        let team_b = teams.iter().find(|team| team.name() == "Equipe B").unwrap();
        let team_c = teams.iter().find(|team| team.name() == "Equipe C").unwrap();

        team_service
            .upsert_member(team_a.id(), leader_a.id(), TeamMemberRole::Leader)
            .unwrap();
        team_service
            .upsert_member(team_a.id(), operator_a.id(), TeamMemberRole::Operator)
            .unwrap();
        team_service
            .upsert_member(team_b.id(), leader_b.id(), TeamMemberRole::Leader)
            .unwrap();
        team_service
            .upsert_member(team_b.id(), operator_b.id(), TeamMemberRole::Operator)
            .unwrap();
        team_service
            .upsert_member(team_c.id(), leader_c.id(), TeamMemberRole::Leader)
            .unwrap();
        team_service
            .upsert_member(team_c.id(), operator_c.id(), TeamMemberRole::Operator)
            .unwrap();

        planning_service
            .save_rotation_cycle(
                PlanningDate::new(2026, 1, 5).unwrap(),
                vec![
                    ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_AFTERNOON).unwrap(),
                    ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_MORNING).unwrap(),
                    ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_NIGHT).unwrap(),
                ],
            )
            .unwrap();

        let report = planning_service
            .generate_range(PlanningDate::new(2026, 1, 5).unwrap(), 10)
            .unwrap();

        assert_eq!(report.active_team_count(), 3);
        assert_eq!(report.planned_worker_count(), 6);

        planning_service
            .save_manual_assignment(
                operator_a.id(),
                PlanningDate::new(2026, 1, 6).unwrap(),
                &ShiftSlotId::new(DEFAULT_SHIFT_SLOT_ID_NIGHT).unwrap(),
            )
            .unwrap();
        planning_service
            .mark_manual_off(leader_b.id(), PlanningDate::new(2026, 1, 6).unwrap())
            .unwrap();

        let loaded = planning_service
            .load_range(PlanningDate::new(2026, 1, 5).unwrap(), 10)
            .unwrap();
        let operator_a_row = loaded.planning().row_for_worker(operator_a.id()).unwrap();
        let leader_b_row = loaded.planning().row_for_worker(leader_b.id()).unwrap();

        assert_eq!(loaded.manual_override_count(), 2);
        assert_eq!(
            operator_a_row
                .cell_for_offset(1)
                .unwrap()
                .shift_slot()
                .unwrap()
                .id()
                .as_str(),
            DEFAULT_SHIFT_SLOT_ID_NIGHT
        );
        assert!(operator_a_row.cell_for_offset(1).unwrap().is_manual());
        assert!(leader_b_row.cell_for_offset(1).unwrap().is_manual_off());
    }

    #[test]
    fn shift_slot_service_rejects_duplicate_short_code() {
        let database = Rc::new(SqliteDatabase::open_in_memory().unwrap());
        let shift_slot_service = ShiftSlotService::new(database);

        let _ = shift_slot_service
            .save_shift_slot(
                None,
                "Horaire test 1",
                "X",
                6,
                0,
                14,
                0,
                ShiftVisualStyle::NeutralGray,
                true,
            )
            .unwrap();

        let error = shift_slot_service
            .save_shift_slot(
                None,
                "Horaire test 2",
                "X",
                14,
                0,
                22,
                0,
                ShiftVisualStyle::AfternoonRed,
                true,
            )
            .unwrap_err();

        assert!(matches!(
            error,
            AppError::DuplicateShiftSlotCode { ref short_code } if short_code == "X"
        ));
    }
}
