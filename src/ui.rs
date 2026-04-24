use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use chrono::{Datelike, Local, Weekday};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::application::{
    JobRoleService, PlanningService, ShiftSlotService, TeamService, WorkerService,
};
use crate::domain::{
    JobRole, PlanningCell, PlanningDate, RangePlanning, RotationCycle, ShiftSlot, ShiftSlotId,
    ShiftVisualStyle, Team, TeamId, TeamMemberRole, TeamMembership, Worker, WorkerId,
};
use crate::error::AppError;
use crate::infrastructure::SqliteDatabase;

slint::include_modules!();

trait StartupWindowControl {
    fn request_maximized(&self);
}

impl StartupWindowControl for AppWindow {
    fn request_maximized(&self) {
        self.window().set_maximized(true);
    }
}

pub fn run() -> Result<(), AppError> {
    let database = Rc::new(SqliteDatabase::open_or_create_default()?);
    let job_role_service = JobRoleService::new(database.clone());
    let worker_service = WorkerService::new(database.clone());
    let shift_slot_service = ShiftSlotService::new(database.clone());
    let team_service = TeamService::new(database.clone());
    let planning_service = PlanningService::new(database.clone());
    let controller = Rc::new(RefCell::new(AppController::new(
        job_role_service,
        worker_service,
        shift_slot_service,
        team_service,
        planning_service,
        database.database_path_hint(),
    )));

    let ui = AppWindow::new()?;
    attach_callbacks(&ui, controller.clone());
    controller.borrow_mut().initialize(&ui)?;
    schedule_startup_window(&ui)?;
    ui.run()?;
    Ok(())
}

fn configure_startup_window(window: &impl StartupWindowControl) {
    window.request_maximized();
}

fn schedule_startup_window(ui: &AppWindow) -> Result<(), AppError> {
    let weak = ui.as_weak();

    slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            configure_startup_window(&ui);
            apply_native_startup_window(&ui);
        }
    })?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn apply_native_startup_window(ui: &AppWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_MAXIMIZE, ShowWindow};

    let native_window = ui.window().window_handle();
    let Ok(window_handle) = native_window.window_handle() else {
        return;
    };

    if let RawWindowHandle::Win32(handle) = window_handle.as_raw() {
        // Ici je demande aussi a Windows de maximiser la vraie fenetre native.
        unsafe {
            ShowWindow(handle.hwnd.get() as _, SW_MAXIMIZE);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_native_startup_window(_ui: &AppWindow) {}

#[derive(Debug, Clone)]
struct SelectedPlanningCell {
    worker_id: WorkerId,
    date: PlanningDate,
}

#[derive(Debug)]
struct AppController {
    job_role_service: JobRoleService,
    worker_service: WorkerService,
    shift_slot_service: ShiftSlotService,
    team_service: TeamService,
    planning_service: PlanningService,
    current_start_date: PlanningDate,
    current_total_days: u32,
    current_workers: Vec<Worker>,
    current_job_roles: Vec<JobRole>,
    current_shift_slots: Vec<ShiftSlot>,
    current_teams: Vec<Team>,
    current_team_memberships: Vec<TeamMembership>,
    current_rotation_cycle: RotationCycle,
    current_planning: Option<RangePlanning>,
    current_active_shift_slots: Vec<ShiftSlot>,
    current_team_member_candidates: Vec<Worker>,
    selected_planning_cell: Option<SelectedPlanningCell>,
    database_path_hint: Option<PathBuf>,
}

impl AppController {
    fn new(
        job_role_service: JobRoleService,
        worker_service: WorkerService,
        shift_slot_service: ShiftSlotService,
        team_service: TeamService,
        planning_service: PlanningService,
        database_path_hint: Option<PathBuf>,
    ) -> Self {
        let today = Local::now().date_naive();
        let current_start_date = PlanningDate::from_naive_date(today)
            .unwrap_or_else(|_| PlanningDate::new(2026, 1, 1).unwrap());

        Self {
            job_role_service,
            worker_service,
            shift_slot_service,
            team_service,
            planning_service,
            current_start_date,
            current_total_days: 90,
            current_workers: Vec::new(),
            current_job_roles: Vec::new(),
            current_shift_slots: Vec::new(),
            current_teams: Vec::new(),
            current_team_memberships: Vec::new(),
            current_rotation_cycle: RotationCycle::default(
                current_start_date
                    .start_of_week_monday()
                    .unwrap_or(current_start_date),
            ),
            current_planning: None,
            current_active_shift_slots: Vec::new(),
            current_team_member_candidates: Vec::new(),
            selected_planning_cell: None,
            database_path_hint,
        }
    }

    fn initialize(&mut self, ui: &AppWindow) -> Result<(), AppError> {
        ui.set_current_page(0);
        ui.set_status_message(SharedString::new());
        ui.set_db_path_hint(
            self.database_path_hint
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "Base locale".to_owned())
                .into(),
        );

        ui.set_shift_style_options(model_from_vec(
            ShiftVisualStyle::ALL
                .iter()
                .map(|style| SharedString::from(style.label()))
                .collect(),
        ));
        ui.set_active_state_options(model_from_vec(vec![
            SharedString::from("Actif"),
            SharedString::from("Inactif"),
        ]));
        ui.set_team_member_role_options(model_from_vec(
            TeamMemberRole::ALL
                .iter()
                .map(|role| SharedString::from(role.label()))
                .collect(),
        ));

        self.refresh_all(ui)?;
        self.clear_worker_form(ui);
        self.clear_shift_slot_form(ui);
        self.clear_team_form(ui);
        self.clear_planning_selection(ui);
        Ok(())
    }

    fn refresh_all(&mut self, ui: &AppWindow) -> Result<(), AppError> {
        let loaded_planning = self
            .planning_service
            .load_range(self.current_start_date, self.current_total_days)?;
        let workers = loaded_planning.workers().to_vec();
        let job_roles = self.job_role_service.list_all()?;
        let shift_slots = loaded_planning.shift_slots().to_vec();
        let teams = loaded_planning.teams().to_vec();
        let team_memberships = loaded_planning.team_memberships().to_vec();
        let rotation_cycle = loaded_planning.rotation_cycle().clone();
        let planning = loaded_planning.planning().clone();
        let active_shift_slots = shift_slots
            .iter()
            .filter(|shift_slot| shift_slot.active())
            .cloned()
            .collect::<Vec<ShiftSlot>>();

        self.current_workers = workers;
        self.current_job_roles = job_roles;
        self.current_shift_slots = shift_slots;
        self.current_teams = teams;
        self.current_team_memberships = team_memberships;
        self.current_rotation_cycle = rotation_cycle;
        self.current_planning = Some(planning);
        self.current_active_shift_slots = active_shift_slots;

        ui.set_role_options(model_from_vec(
            self.current_job_roles
                .iter()
                .map(|role| SharedString::from(role.label()))
                .collect(),
        ));
        ui.set_worker_rows(model_from_vec(build_worker_rows(
            &self.current_workers,
            &self.current_teams,
            &self.current_team_memberships,
        )));
        ui.set_shift_slot_rows(model_from_vec(build_shift_slot_rows(
            &self.current_shift_slots,
        )));
        ui.set_team_rows(model_from_vec(build_team_rows(
            &self.current_teams,
            &self.current_team_memberships,
            &self.current_workers,
            &self.current_shift_slots,
        )));
        ui.set_active_shift_slot_options(model_from_vec(build_shift_slot_options(
            &self.current_active_shift_slots,
        )));
        ui.set_day_headers(model_from_vec(build_day_headers(
            self.current_start_date,
            self.current_total_days,
        )));
        let planning_rows = self
            .current_planning
            .as_ref()
            .map(build_planning_rows)
            .unwrap_or_default();
        ui.set_planning_rows(model_from_vec(planning_rows));
        ui.set_planning_start_date(self.current_start_date.to_string().into());
        ui.set_planning_days(self.current_total_days.to_string().into());
        ui.set_summary_generated_assignments(loaded_planning.generated_assignment_count() as i32);
        ui.set_summary_manual_overrides(loaded_planning.manual_override_count() as i32);
        ui.set_summary_active_teams(
            self.current_teams
                .iter()
                .filter(|team| team.active())
                .count() as i32,
        );
        ui.set_summary_planned_workers(
            self.current_team_memberships
                .iter()
                .map(|membership| membership.worker_id().clone())
                .collect::<std::collections::BTreeSet<WorkerId>>()
                .len() as i32,
        );
        ui.set_rotation_reference_week_start(
            self.current_rotation_cycle
                .reference_week_start()
                .to_string()
                .into(),
        );
        self.refresh_rotation_indices(ui);
        self.refresh_worker_role_index(ui);
        self.refresh_team_anchor_index(ui);
        self.refresh_selected_team_data(ui);
        self.refresh_selected_cell_after_reload(ui);

        Ok(())
    }

    fn refresh_selected_cell_after_reload(&self, ui: &AppWindow) {
        if let Some(selected) = &self.selected_planning_cell {
            if self
                .current_workers
                .iter()
                .any(|worker| worker.id() == &selected.worker_id)
            {
                self.apply_planning_selection(ui, &selected.worker_id, selected.date);
                return;
            }
        }

        self.clear_planning_selection(ui);
    }

    fn refresh_rotation_indices(&self, ui: &AppWindow) {
        let ordered = self.current_rotation_cycle.ordered_shift_slot_ids();

        ui.set_rotation_week_one_shift_index(
            ordered
                .get(0)
                .map(|shift_slot_id| self.active_shift_slot_index(shift_slot_id))
                .unwrap_or(-1),
        );
        ui.set_rotation_week_two_shift_index(
            ordered
                .get(1)
                .map(|shift_slot_id| self.active_shift_slot_index(shift_slot_id))
                .unwrap_or(-1),
        );
        ui.set_rotation_week_three_shift_index(
            ordered
                .get(2)
                .map(|shift_slot_id| self.active_shift_slot_index(shift_slot_id))
                .unwrap_or(-1),
        );
    }

    fn refresh_worker_role_index(&self, ui: &AppWindow) {
        let role_index = ui.get_worker_role_index();
        let max_index = self.current_job_roles.len() as i32 - 1;

        if self.current_job_roles.is_empty() {
            ui.set_worker_role_index(-1);
            return;
        }

        if role_index < 0 || role_index > max_index {
            ui.set_worker_role_index(0);
        }
    }

    fn refresh_team_anchor_index(&self, ui: &AppWindow) {
        if ui.get_team_form_id().is_empty() {
            if self.current_active_shift_slots.is_empty() {
                ui.set_team_anchor_shift_index(-1);
            } else if ui.get_team_anchor_shift_index() < 0 {
                ui.set_team_anchor_shift_index(0);
            }
            return;
        }

        let selected_team_id = match TeamId::new(ui.get_team_form_id().to_string()) {
            Ok(value) => value,
            Err(_) => {
                ui.set_team_anchor_shift_index(-1);
                return;
            }
        };

        if let Some(team) = self
            .current_teams
            .iter()
            .find(|team| team.id() == &selected_team_id)
        {
            ui.set_team_anchor_shift_index(
                self.active_shift_slot_index(team.anchor_shift_slot_id()),
            );
        }
    }

    fn refresh_selected_team_data(&mut self, ui: &AppWindow) {
        let selected_team_id = non_empty_shared_string(&ui.get_team_form_id())
            .and_then(|value| TeamId::new(value).ok());

        self.current_team_member_candidates =
            self.build_team_member_candidates(selected_team_id.as_ref());

        ui.set_team_member_candidate_options(model_from_vec(
            self.current_team_member_candidates
                .iter()
                .map(|worker| {
                    SharedString::from(format!(
                        "{} - {}",
                        worker.display_name(),
                        worker.job_role().label()
                    ))
                })
                .collect(),
        ));
        ui.set_selected_team_member_rows(model_from_vec(build_team_member_rows(
            selected_team_id.as_ref(),
            &self.current_team_memberships,
            &self.current_workers,
        )));

        if self.current_team_member_candidates.is_empty() {
            ui.set_team_member_worker_index(-1);
        } else if ui.get_team_member_worker_index() < 0
            || ui.get_team_member_worker_index() >= self.current_team_member_candidates.len() as i32
        {
            ui.set_team_member_worker_index(0);
        }

        if ui.get_team_member_role_index() < 0 || ui.get_team_member_role_index() > 1 {
            ui.set_team_member_role_index(1);
        }
    }

    fn build_team_member_candidates(&self, selected_team_id: Option<&TeamId>) -> Vec<Worker> {
        let mut team_by_worker = BTreeMap::new();

        for membership in &self.current_team_memberships {
            team_by_worker.insert(membership.worker_id().clone(), membership.team_id().clone());
        }

        let mut candidates = self
            .current_workers
            .iter()
            .filter(|worker| match team_by_worker.get(worker.id()) {
                Some(team_id) => Some(team_id) == selected_team_id,
                None => true,
            })
            .cloned()
            .collect::<Vec<Worker>>();

        candidates.sort_by(|left, right| {
            left.last_name()
                .cmp(right.last_name())
                .then_with(|| left.first_name().cmp(right.first_name()))
                .then_with(|| left.id().as_str().cmp(right.id().as_str()))
        });

        candidates
    }

    fn clear_worker_form(&self, ui: &AppWindow) {
        ui.set_worker_form_id(SharedString::new());
        ui.set_worker_last_name(SharedString::new());
        ui.set_worker_first_name(SharedString::new());
        ui.set_new_role_name(SharedString::new());
        ui.set_worker_existing(false);
        ui.set_worker_role_index(if self.current_job_roles.is_empty() {
            -1
        } else {
            0
        });
        self.clear_status(ui);
    }

    fn select_worker(&self, ui: &AppWindow, worker_id: &str) {
        if let Some(worker) = self
            .current_workers
            .iter()
            .find(|worker| worker.id().as_str() == worker_id)
        {
            ui.set_current_page(3);
            ui.set_worker_form_id(worker.id().to_string().into());
            ui.set_worker_last_name(worker.last_name().into());
            ui.set_worker_first_name(worker.first_name().into());
            ui.set_worker_role_index(self.job_role_index(worker.job_role()));
            ui.set_worker_existing(true);
            ui.set_new_role_name(SharedString::new());
            self.clear_status(ui);
        }
    }

    fn save_worker(
        &mut self,
        ui: &AppWindow,
        worker_id: SharedString,
        last_name: SharedString,
        first_name: SharedString,
        role_index: i32,
    ) -> Result<(), AppError> {
        let job_role = self.job_role_from_index(role_index)?;
        let worker = self.worker_service.save_worker(
            non_empty_shared_string(&worker_id),
            last_name.to_string(),
            first_name.to_string(),
            job_role,
        )?;

        self.refresh_all(ui)?;
        self.select_worker(ui, worker.id().as_str());
        self.show_success(ui, "Fiche salarie enregistree.");
        Ok(())
    }

    fn add_job_role(&mut self, ui: &AppWindow, role_name: SharedString) -> Result<(), AppError> {
        let role = self.job_role_service.save_role(role_name.to_string())?;

        self.refresh_all(ui)?;
        ui.set_new_role_name(SharedString::new());
        ui.set_worker_role_index(self.job_role_index(&role));
        ui.set_current_page(3);
        self.show_success(ui, "Poste ajoute au catalogue.");
        Ok(())
    }

    fn delete_worker(&mut self, ui: &AppWindow, worker_id: SharedString) -> Result<(), AppError> {
        let worker_id = WorkerId::new(worker_id.to_string())?;
        self.worker_service.delete_worker(&worker_id)?;
        self.refresh_all(ui)?;
        self.clear_worker_form(ui);
        self.show_success(ui, "Salarie supprime.");
        Ok(())
    }

    fn clear_shift_slot_form(&self, ui: &AppWindow) {
        ui.set_shift_slot_form_id(SharedString::new());
        ui.set_shift_slot_name(SharedString::new());
        ui.set_shift_slot_code(SharedString::new());
        ui.set_shift_slot_start_hour(SharedString::new());
        ui.set_shift_slot_start_minute(SharedString::new());
        ui.set_shift_slot_end_hour(SharedString::new());
        ui.set_shift_slot_end_minute(SharedString::new());
        ui.set_shift_slot_style_index(0);
        ui.set_shift_slot_active_index(0);
        ui.set_shift_slot_existing(false);
        self.clear_status(ui);
    }

    fn select_shift_slot(&self, ui: &AppWindow, shift_slot_id: &str) {
        if let Some(shift_slot) = self
            .current_shift_slots
            .iter()
            .find(|shift_slot| shift_slot.id().as_str() == shift_slot_id)
        {
            ui.set_current_page(2);
            ui.set_shift_slot_form_id(shift_slot.id().to_string().into());
            ui.set_shift_slot_name(shift_slot.name().into());
            ui.set_shift_slot_code(shift_slot.short_code().into());
            ui.set_shift_slot_start_hour(format!("{:02}", shift_slot.start_time().hour()).into());
            ui.set_shift_slot_start_minute(
                format!("{:02}", shift_slot.start_time().minute()).into(),
            );
            ui.set_shift_slot_end_hour(format!("{:02}", shift_slot.end_time().hour()).into());
            ui.set_shift_slot_end_minute(format!("{:02}", shift_slot.end_time().minute()).into());
            ui.set_shift_slot_style_index(shift_visual_style_to_index(shift_slot.visual_style()));
            ui.set_shift_slot_active_index(bool_to_active_index(shift_slot.active()));
            ui.set_shift_slot_existing(true);
            self.clear_status(ui);
        }
    }

    fn save_shift_slot(
        &mut self,
        ui: &AppWindow,
        shift_slot_id: SharedString,
        name: SharedString,
        short_code: SharedString,
        start_hour: SharedString,
        start_minute: SharedString,
        end_hour: SharedString,
        end_minute: SharedString,
        style_index: i32,
        active_index: i32,
    ) -> Result<(), AppError> {
        let visual_style = shift_visual_style_from_index(style_index)?;
        let active = active_index_to_bool(active_index);
        let shift_slot = self.shift_slot_service.save_shift_slot(
            non_empty_shared_string(&shift_slot_id),
            name.to_string(),
            short_code.to_string(),
            parse_u8_input(&start_hour)?,
            parse_u8_input(&start_minute)?,
            parse_u8_input(&end_hour)?,
            parse_u8_input(&end_minute)?,
            visual_style,
            active,
        )?;

        self.refresh_all(ui)?;
        self.select_shift_slot(ui, shift_slot.id().as_str());
        self.show_success(ui, "Plage horaire enregistree.");
        Ok(())
    }

    fn save_rotation_cycle(
        &mut self,
        ui: &AppWindow,
        reference_week_start: SharedString,
        week_one_shift_index: i32,
        week_two_shift_index: i32,
        week_three_shift_index: i32,
    ) -> Result<(), AppError> {
        let reference_week_start = parse_date_input(&reference_week_start)?;
        let ordered_shift_slot_ids = vec![
            self.active_shift_slot_from_index(week_one_shift_index)?
                .id()
                .clone(),
            self.active_shift_slot_from_index(week_two_shift_index)?
                .id()
                .clone(),
            self.active_shift_slot_from_index(week_three_shift_index)?
                .id()
                .clone(),
        ];

        self.planning_service
            .save_rotation_cycle(reference_week_start, ordered_shift_slot_ids)?;
        self.refresh_all(ui)?;
        ui.set_current_page(2);
        self.show_success(ui, "Cycle 3x8 enregistre.");
        Ok(())
    }

    fn clear_team_form(&mut self, ui: &AppWindow) {
        ui.set_team_form_id(SharedString::new());
        ui.set_team_name(SharedString::new());
        ui.set_team_anchor_shift_index(if self.current_active_shift_slots.is_empty() {
            -1
        } else {
            0
        });
        ui.set_team_active_index(0);
        ui.set_team_existing(false);
        ui.set_team_member_worker_index(-1);
        ui.set_team_member_role_index(1);
        self.refresh_selected_team_data(ui);
        self.clear_status(ui);
    }

    fn select_team(&mut self, ui: &AppWindow, team_id: &str) {
        let selected_team_id = TeamId::new(team_id.to_owned());
        let Ok(selected_team_id) = selected_team_id else {
            return;
        };

        if let Some(team) = self
            .current_teams
            .iter()
            .find(|team| team.id() == &selected_team_id)
        {
            ui.set_current_page(1);
            ui.set_team_form_id(team.id().to_string().into());
            ui.set_team_name(team.name().into());
            ui.set_team_anchor_shift_index(
                self.active_shift_slot_index(team.anchor_shift_slot_id()),
            );
            ui.set_team_active_index(bool_to_active_index(team.active()));
            ui.set_team_existing(true);
            ui.set_team_member_role_index(1);
            self.refresh_selected_team_data(ui);
            self.clear_status(ui);
        }
    }

    fn save_team(
        &mut self,
        ui: &AppWindow,
        team_id: SharedString,
        team_name: SharedString,
        anchor_shift_index: i32,
        active_index: i32,
    ) -> Result<(), AppError> {
        let anchor_shift_slot_id = self
            .active_shift_slot_from_index(anchor_shift_index)?
            .id()
            .clone();
        let active = active_index_to_bool(active_index);
        let team = self.team_service.save_team(
            non_empty_shared_string(&team_id),
            team_name.to_string(),
            anchor_shift_slot_id,
            active,
        )?;

        self.refresh_all(ui)?;
        self.select_team(ui, team.id().as_str());
        self.show_success(ui, "Equipe enregistree.");
        Ok(())
    }

    fn add_team_member(
        &mut self,
        ui: &AppWindow,
        team_id: SharedString,
        worker_index: i32,
        role_index: i32,
    ) -> Result<(), AppError> {
        let team_id = TeamId::new(team_id.to_string())?;
        let worker = self
            .current_team_member_candidates
            .get(worker_index as usize)
            .cloned()
            .ok_or(AppError::MissingWorkerSelection)?;
        let role = team_member_role_from_index(role_index)?;

        self.team_service
            .upsert_member(&team_id, worker.id(), role)?;
        self.refresh_all(ui)?;
        self.select_team(ui, team_id.as_str());
        self.show_success(ui, "Membre d'equipe enregistre.");
        Ok(())
    }

    fn remove_team_member(
        &mut self,
        ui: &AppWindow,
        team_id: SharedString,
        worker_id: SharedString,
    ) -> Result<(), AppError> {
        let team_id = TeamId::new(team_id.to_string())?;
        let worker_id = WorkerId::new(worker_id.to_string())?;

        self.team_service.remove_member(&team_id, &worker_id)?;
        self.refresh_all(ui)?;
        self.select_team(ui, team_id.as_str());
        self.show_success(ui, "Membre retire de l'equipe.");
        Ok(())
    }

    fn generate_planning(
        &mut self,
        ui: &AppWindow,
        start_date: SharedString,
        total_days: SharedString,
    ) -> Result<(), AppError> {
        let start_date = parse_date_input(&start_date)?;
        let total_days = parse_u32_input(&total_days)?;

        self.current_start_date = start_date;
        self.current_total_days = total_days;

        let report = self
            .planning_service
            .generate_range(self.current_start_date, self.current_total_days)?;

        self.refresh_all(ui)?;
        self.clear_planning_selection(ui);
        ui.set_current_page(0);
        self.show_success(
            ui,
            &format!(
                "Planning genere pour {} jours a partir du {}. {} affectations automatiques calculees.",
                report.total_days(),
                report.start_date(),
                report.generated_assignment_count()
            ),
        );
        Ok(())
    }

    fn apply_planning_selection(&self, ui: &AppWindow, worker_id: &WorkerId, date: PlanningDate) {
        let Some(planning) = &self.current_planning else {
            self.clear_planning_selection(ui);
            return;
        };

        let date_offset = date
            .signed_days_since(planning.start_date())
            .try_into()
            .unwrap_or(0_u32);

        let summary = match planning.row_for_worker(worker_id) {
            Some(row) => match row.cell_for_offset(date_offset) {
                Some(PlanningCell::Assignment {
                    shift_slot, origin, ..
                }) => {
                    let origin_label = match origin {
                        crate::domain::AssignmentOrigin::Generated => "Automatique",
                        crate::domain::AssignmentOrigin::Manual => "Manuel",
                    };

                    ui.set_manual_shift_index(self.active_shift_slot_index(shift_slot.id()));
                    format!(
                        "{} • {} • {} {} • {}",
                        row.worker_name(),
                        date,
                        shift_slot.name(),
                        shift_slot.time_range_label(),
                        origin_label
                    )
                }
                Some(PlanningCell::ManualOff { .. }) => {
                    ui.set_manual_shift_index(-1);
                    format!("{} • {} • Sans poste (manuel)", row.worker_name(), date)
                }
                Some(PlanningCell::Empty { .. }) => {
                    ui.set_manual_shift_index(-1);
                    format!("{} • {} • Cellule vide", row.worker_name(), date)
                }
                None => {
                    ui.set_manual_shift_index(-1);
                    format!("{} • {} • Cellule hors plage", row.worker_name(), date)
                }
            },
            None => {
                ui.set_manual_shift_index(-1);
                format!("{} • {} • Salarie introuvable", worker_id, date)
            }
        };

        ui.set_selected_cell_worker_id(worker_id.to_string().into());
        ui.set_selected_cell_date(date.to_string().into());
        ui.set_selected_cell_summary(summary.into());
    }

    fn select_planning_cell(
        &mut self,
        ui: &AppWindow,
        worker_id: SharedString,
        day_offset: i32,
    ) -> Result<(), AppError> {
        let worker_id = WorkerId::new(worker_id.to_string())?;
        let date = self.current_start_date.add_days(day_offset as i64)?;
        self.selected_planning_cell = Some(SelectedPlanningCell {
            worker_id: worker_id.clone(),
            date,
        });
        self.apply_planning_selection(ui, &worker_id, date);
        ui.set_current_page(0);
        Ok(())
    }

    fn save_manual_assignment(
        &mut self,
        ui: &AppWindow,
        worker_id: SharedString,
        date: SharedString,
        shift_slot_index: i32,
    ) -> Result<(), AppError> {
        let worker_id = WorkerId::new(worker_id.to_string())?;
        let date = parse_date_input(&date)?;
        let shift_slot_id = self
            .active_shift_slot_from_index(shift_slot_index)?
            .id()
            .clone();

        self.planning_service
            .save_manual_assignment(&worker_id, date, &shift_slot_id)?;
        self.refresh_all(ui)?;
        self.selected_planning_cell = Some(SelectedPlanningCell {
            worker_id: worker_id.clone(),
            date,
        });
        self.apply_planning_selection(ui, &worker_id, date);
        self.show_success(ui, "Correction manuelle enregistree.");
        Ok(())
    }

    fn mark_manual_off(
        &mut self,
        ui: &AppWindow,
        worker_id: SharedString,
        date: SharedString,
    ) -> Result<(), AppError> {
        let worker_id = WorkerId::new(worker_id.to_string())?;
        let date = parse_date_input(&date)?;

        self.planning_service.mark_manual_off(&worker_id, date)?;
        self.refresh_all(ui)?;
        self.selected_planning_cell = Some(SelectedPlanningCell {
            worker_id: worker_id.clone(),
            date,
        });
        self.apply_planning_selection(ui, &worker_id, date);
        self.show_success(ui, "Cellule marquee sans poste.");
        Ok(())
    }

    fn clear_manual_override(
        &mut self,
        ui: &AppWindow,
        worker_id: SharedString,
        date: SharedString,
    ) -> Result<(), AppError> {
        let worker_id = WorkerId::new(worker_id.to_string())?;
        let date = parse_date_input(&date)?;

        self.planning_service
            .clear_manual_override(&worker_id, date)?;
        self.refresh_all(ui)?;
        self.selected_planning_cell = Some(SelectedPlanningCell {
            worker_id: worker_id.clone(),
            date,
        });
        self.apply_planning_selection(ui, &worker_id, date);
        self.show_success(ui, "Retour au planning automatique effectue.");
        Ok(())
    }

    fn clear_planning_selection(&self, ui: &AppWindow) {
        ui.set_selected_cell_worker_id(SharedString::new());
        ui.set_selected_cell_date(SharedString::new());
        ui.set_selected_cell_summary("Aucune cellule selectionnee.".into());
        ui.set_manual_shift_index(-1);
    }

    fn job_role_from_index(&self, index: i32) -> Result<JobRole, AppError> {
        if index < 0 {
            return Err(AppError::MissingJobRoleSelection);
        }

        self.current_job_roles
            .get(index as usize)
            .cloned()
            .ok_or(AppError::MissingJobRoleSelection)
    }

    fn job_role_index(&self, job_role: &JobRole) -> i32 {
        self.current_job_roles
            .iter()
            .position(|role| role == job_role)
            .map(|index| index as i32)
            .unwrap_or(-1)
    }

    fn active_shift_slot_from_index(&self, index: i32) -> Result<&ShiftSlot, AppError> {
        if index < 0 {
            return Err(AppError::MissingShiftSlotSelection);
        }

        self.current_active_shift_slots
            .get(index as usize)
            .ok_or(AppError::MissingShiftSlotSelection)
    }

    fn active_shift_slot_index(&self, shift_slot_id: &ShiftSlotId) -> i32 {
        self.current_active_shift_slots
            .iter()
            .position(|shift_slot| shift_slot.id() == shift_slot_id)
            .map(|index| index as i32)
            .unwrap_or(-1)
    }

    fn clear_status(&self, ui: &AppWindow) {
        ui.set_status_message(SharedString::new());
    }

    fn show_success(&self, ui: &AppWindow, message: &str) {
        ui.set_status_message(message.into());
    }

    fn show_error(&self, ui: &AppWindow, error: &AppError) {
        ui.set_status_message(error.to_string().into());
    }
}

fn attach_callbacks(ui: &AppWindow, controller: Rc<RefCell<AppController>>) {
    let weak = ui.as_weak();

    ui.on_select_page({
        let controller = controller.clone();
        move |page_index| {
            if let Some(ui) = weak.upgrade() {
                ui.set_current_page(page_index);
                controller.borrow().clear_status(&ui);
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_generate_planning({
        let controller = controller.clone();
        move |start_date, total_days| {
            if let Some(ui) = weak.upgrade() {
                let result = {
                    controller
                        .borrow_mut()
                        .generate_planning(&ui, start_date, total_days)
                };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_select_planning_cell({
        let controller = controller.clone();
        move |worker_id, day_offset| {
            if let Some(ui) = weak.upgrade() {
                let result = {
                    controller
                        .borrow_mut()
                        .select_planning_cell(&ui, worker_id, day_offset)
                };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_save_manual_assignment({
        let controller = controller.clone();
        move |worker_id, date, shift_slot_index| {
            if let Some(ui) = weak.upgrade() {
                let result = {
                    controller.borrow_mut().save_manual_assignment(
                        &ui,
                        worker_id,
                        date,
                        shift_slot_index,
                    )
                };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_mark_manual_off({
        let controller = controller.clone();
        move |worker_id, date| {
            if let Some(ui) = weak.upgrade() {
                let result = {
                    controller
                        .borrow_mut()
                        .mark_manual_off(&ui, worker_id, date)
                };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_clear_manual_override({
        let controller = controller.clone();
        move |worker_id, date| {
            if let Some(ui) = weak.upgrade() {
                let result = {
                    controller
                        .borrow_mut()
                        .clear_manual_override(&ui, worker_id, date)
                };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_clear_planning_selection({
        let controller = controller.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                controller.borrow_mut().selected_planning_cell = None;
                controller.borrow().clear_planning_selection(&ui);
                controller.borrow().clear_status(&ui);
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_clear_worker_form({
        let controller = controller.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                controller.borrow().clear_worker_form(&ui);
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_select_worker({
        let controller = controller.clone();
        move |worker_id| {
            if let Some(ui) = weak.upgrade() {
                controller.borrow().select_worker(&ui, &worker_id);
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_save_worker({
        let controller = controller.clone();
        move |worker_id, last_name, first_name, role_index| {
            if let Some(ui) = weak.upgrade() {
                let result = {
                    controller
                        .borrow_mut()
                        .save_worker(&ui, worker_id, last_name, first_name, role_index)
                };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_add_job_role({
        let controller = controller.clone();
        move |role_name| {
            if let Some(ui) = weak.upgrade() {
                let result = { controller.borrow_mut().add_job_role(&ui, role_name) };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_delete_worker({
        let controller = controller.clone();
        move |worker_id| {
            if let Some(ui) = weak.upgrade() {
                let result = { controller.borrow_mut().delete_worker(&ui, worker_id) };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_clear_shift_slot_form({
        let controller = controller.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                controller.borrow().clear_shift_slot_form(&ui);
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_select_shift_slot({
        let controller = controller.clone();
        move |shift_slot_id| {
            if let Some(ui) = weak.upgrade() {
                controller.borrow().select_shift_slot(&ui, &shift_slot_id);
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_save_shift_slot({
        let controller = controller.clone();
        move |shift_slot_id,
              name,
              short_code,
              start_hour,
              start_minute,
              end_hour,
              end_minute,
              style_index,
              active_index| {
            if let Some(ui) = weak.upgrade() {
                let result = {
                    controller.borrow_mut().save_shift_slot(
                        &ui,
                        shift_slot_id,
                        name,
                        short_code,
                        start_hour,
                        start_minute,
                        end_hour,
                        end_minute,
                        style_index,
                        active_index,
                    )
                };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_save_rotation_cycle({
        let controller = controller.clone();
        move |reference_week_start, week_one, week_two, week_three| {
            if let Some(ui) = weak.upgrade() {
                let result = {
                    controller.borrow_mut().save_rotation_cycle(
                        &ui,
                        reference_week_start,
                        week_one,
                        week_two,
                        week_three,
                    )
                };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_clear_team_form({
        let controller = controller.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                controller.borrow_mut().clear_team_form(&ui);
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_select_team({
        let controller = controller.clone();
        move |team_id| {
            if let Some(ui) = weak.upgrade() {
                controller.borrow_mut().select_team(&ui, &team_id);
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_save_team({
        let controller = controller.clone();
        move |team_id, team_name, anchor_shift_index, active_index| {
            if let Some(ui) = weak.upgrade() {
                let result = {
                    controller.borrow_mut().save_team(
                        &ui,
                        team_id,
                        team_name,
                        anchor_shift_index,
                        active_index,
                    )
                };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_add_team_member({
        let controller = controller.clone();
        move |team_id, worker_index, role_index| {
            if let Some(ui) = weak.upgrade() {
                let result = {
                    controller
                        .borrow_mut()
                        .add_team_member(&ui, team_id, worker_index, role_index)
                };

                if let Err(error) = result {
                    controller.borrow().show_error(&ui, &error);
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_remove_team_member(move |team_id, worker_id| {
        if let Some(ui) = weak.upgrade() {
            let result = {
                controller
                    .borrow_mut()
                    .remove_team_member(&ui, team_id, worker_id)
            };

            if let Err(error) = result {
                controller.borrow().show_error(&ui, &error);
            }
        }
    });
}

fn model_from_vec<T: Clone + 'static>(values: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(values))
}

fn build_worker_rows(
    workers: &[Worker],
    teams: &[Team],
    memberships: &[TeamMembership],
) -> Vec<WorkerRowData> {
    let teams_by_id = teams
        .iter()
        .map(|team| (team.id().clone(), team))
        .collect::<BTreeMap<TeamId, &Team>>();
    let memberships_by_worker = memberships
        .iter()
        .map(|membership| (membership.worker_id().clone(), membership))
        .collect::<BTreeMap<WorkerId, &TeamMembership>>();

    workers
        .iter()
        .map(|worker| {
            let team_name = memberships_by_worker
                .get(worker.id())
                .and_then(|membership| teams_by_id.get(membership.team_id()))
                .map(|team| team.name())
                .unwrap_or("");

            WorkerRowData {
                id: worker.id().to_string().into(),
                display_name: worker.display_name().into(),
                job_role_label: worker.job_role().label().into(),
                team_name: team_name.into(),
            }
        })
        .collect()
}

fn build_shift_slot_rows(shift_slots: &[ShiftSlot]) -> Vec<ShiftSlotRowData> {
    shift_slots
        .iter()
        .map(|shift_slot| ShiftSlotRowData {
            id: shift_slot.id().to_string().into(),
            name: shift_slot.name().into(),
            short_code: shift_slot.short_code().into(),
            time_range: shift_slot.time_range_label().into(),
            style_label: shift_slot.visual_style().label().into(),
            active_label: if shift_slot.active() {
                "Actif".into()
            } else {
                "Inactif".into()
            },
        })
        .collect()
}

fn build_team_rows(
    teams: &[Team],
    memberships: &[TeamMembership],
    workers: &[Worker],
    shift_slots: &[ShiftSlot],
) -> Vec<TeamRowData> {
    let workers_by_id = workers
        .iter()
        .map(|worker| (worker.id().clone(), worker))
        .collect::<BTreeMap<WorkerId, &Worker>>();
    let shift_slots_by_id = shift_slots
        .iter()
        .map(|shift_slot| (shift_slot.id().clone(), shift_slot))
        .collect::<BTreeMap<ShiftSlotId, &ShiftSlot>>();

    teams
        .iter()
        .map(|team| {
            let team_members = memberships
                .iter()
                .filter(|membership| membership.team_id() == team.id())
                .collect::<Vec<&TeamMembership>>();
            let leader_name = team_members
                .iter()
                .find(|membership| membership.role() == TeamMemberRole::Leader)
                .and_then(|membership| workers_by_id.get(membership.worker_id()))
                .map(|worker| worker.display_name())
                .unwrap_or("");
            let anchor_shift_label = shift_slots_by_id
                .get(team.anchor_shift_slot_id())
                .map(|shift_slot| {
                    format!("{} ({})", shift_slot.name(), shift_slot.time_range_label())
                })
                .unwrap_or_else(|| team.anchor_shift_slot_id().to_string());

            TeamRowData {
                id: team.id().to_string().into(),
                name: team.name().into(),
                anchor_shift_label: anchor_shift_label.into(),
                member_count: team_members.len() as i32,
                leader_name: leader_name.into(),
                active_label: if team.active() {
                    "Actif".into()
                } else {
                    "Inactif".into()
                },
            }
        })
        .collect()
}

fn build_team_member_rows(
    selected_team_id: Option<&TeamId>,
    memberships: &[TeamMembership],
    workers: &[Worker],
) -> Vec<TeamMemberRowData> {
    let Some(selected_team_id) = selected_team_id else {
        return Vec::new();
    };

    let workers_by_id = workers
        .iter()
        .map(|worker| (worker.id().clone(), worker))
        .collect::<BTreeMap<WorkerId, &Worker>>();

    memberships
        .iter()
        .filter(|membership| membership.team_id() == selected_team_id)
        .filter_map(|membership| {
            workers_by_id
                .get(membership.worker_id())
                .map(|worker| TeamMemberRowData {
                    worker_id: worker.id().to_string().into(),
                    worker_name: worker.display_name().into(),
                    role_label: membership.role().label().into(),
                })
        })
        .collect()
}

fn build_shift_slot_options(shift_slots: &[ShiftSlot]) -> Vec<SharedString> {
    shift_slots
        .iter()
        .map(|shift_slot| {
            format!("{} - {}", shift_slot.name(), shift_slot.time_range_label()).into()
        })
        .collect()
}

fn build_day_headers(start_date: PlanningDate, total_days: u32) -> Vec<DayHeaderData> {
    (0..total_days)
        .filter_map(|offset| {
            let date = start_date.add_days(offset as i64).ok()?;
            let naive_date = date.to_naive_date();

            Some(DayHeaderData {
                day_offset: offset as i32,
                date_label: format!("{:02}", date.day()).into(),
                weekday_label: weekday_label_from_weekday(naive_date.weekday()).into(),
                month_label: month_label_short(date.month()).into(),
                is_weekend: matches!(naive_date.weekday(), Weekday::Sat | Weekday::Sun),
            })
        })
        .collect()
}

fn build_planning_rows(planning: &RangePlanning) -> Vec<PlanningRowData> {
    planning
        .rows()
        .iter()
        .map(|row| PlanningRowData {
            worker_id: row.worker_id().to_string().into(),
            worker_name: row.worker_name().into(),
            team_name: row.team_name().into(),
            team_role_label: row.team_role_label().into(),
            job_role_label: row.job_role().label().into(),
            cells: model_from_vec(build_planning_cells(row)),
        })
        .collect()
}

fn build_planning_cells(row: &crate::domain::PlanningRow) -> Vec<PlanningCellData> {
    row.cells()
        .iter()
        .enumerate()
        .map(|(offset, cell)| PlanningCellData {
            day_offset: offset as i32,
            short_code: SharedString::from(cell.short_code()),
            label: SharedString::from(cell.label()),
            time_range: SharedString::from(cell.time_range_label()),
            style_token: cell.style_token().into(),
            has_assignment: cell.has_assignment(),
            is_manual: cell.is_manual(),
            is_manual_off: cell.is_manual_off(),
        })
        .collect()
}

fn weekday_label_from_weekday(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "Lu",
        Weekday::Tue => "Ma",
        Weekday::Wed => "Me",
        Weekday::Thu => "Je",
        Weekday::Fri => "Ve",
        Weekday::Sat => "Sa",
        Weekday::Sun => "Di",
    }
}

fn month_label_short(month: u8) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Fev",
        3 => "Mar",
        4 => "Avr",
        5 => "Mai",
        6 => "Juin",
        7 => "Juil",
        8 => "Aou",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "",
    }
}

fn shift_visual_style_to_index(style: ShiftVisualStyle) -> i32 {
    ShiftVisualStyle::ALL
        .iter()
        .position(|candidate| *candidate == style)
        .map(|index| index as i32)
        .unwrap_or(-1)
}

fn shift_visual_style_from_index(index: i32) -> Result<ShiftVisualStyle, AppError> {
    if index < 0 {
        return Err(AppError::MissingShiftSlotSelection);
    }

    ShiftVisualStyle::ALL
        .get(index as usize)
        .copied()
        .ok_or(AppError::MissingShiftSlotSelection)
}

fn team_member_role_from_index(index: i32) -> Result<TeamMemberRole, AppError> {
    if index < 0 {
        return Err(AppError::MissingTeamMemberRoleSelection);
    }

    TeamMemberRole::ALL
        .get(index as usize)
        .copied()
        .ok_or(AppError::MissingTeamMemberRoleSelection)
}

fn active_index_to_bool(index: i32) -> bool {
    index == 0
}

fn bool_to_active_index(value: bool) -> i32 {
    if value { 0 } else { 1 }
}

fn parse_u8_input(value: &SharedString) -> Result<u8, AppError> {
    value
        .trim()
        .parse::<u8>()
        .map_err(|_| AppError::InvalidNumericInput(value.to_string()))
}

fn parse_u32_input(value: &SharedString) -> Result<u32, AppError> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| AppError::InvalidNumericInput(value.to_string()))
}

fn parse_date_input(value: &SharedString) -> Result<PlanningDate, AppError> {
    let normalized = value.to_string();
    PlanningDate::parse_iso(&normalized).map_err(|_| AppError::InvalidDateInput(normalized))
}

fn non_empty_shared_string(value: &SharedString) -> Option<String> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_headers_follow_start_date_offsets() {
        let headers = build_day_headers(PlanningDate::new(2026, 4, 23).unwrap(), 3);

        assert_eq!(headers.len(), 3);
        assert_eq!(headers[0].date_label, SharedString::from("23"));
        assert_eq!(headers[1].date_label, SharedString::from("24"));
        assert_eq!(headers[2].date_label, SharedString::from("25"));
    }

    #[test]
    fn parse_helpers_reject_invalid_values() {
        assert!(parse_u8_input(&SharedString::from("abc")).is_err());
        assert!(parse_date_input(&SharedString::from("2026/04/23")).is_err());
    }

    #[test]
    fn bool_to_active_index_roundtrips() {
        assert!(active_index_to_bool(bool_to_active_index(true)));
        assert!(!active_index_to_bool(bool_to_active_index(false)));
    }

    #[test]
    fn non_empty_shared_string_trims_input() {
        assert_eq!(
            non_empty_shared_string(&SharedString::from("  equipe-a  ")),
            Some("equipe-a".to_owned())
        );
        assert_eq!(non_empty_shared_string(&SharedString::from("   ")), None);
    }
}
