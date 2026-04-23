use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use chrono::{Datelike, Local, NaiveDate, Weekday};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::application::{AssignmentService, JobRoleService, PlanningFacade, WorkerService};
use crate::domain::{
    JobRole, MonthlyPlanning, PlanningDate, PlanningRow, ShiftKind, Worker, WorkerId,
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
    let assignment_service = AssignmentService::new(database.clone());
    let planning_facade = PlanningFacade::new(worker_service.clone(), assignment_service.clone());
    let controller = Rc::new(RefCell::new(AppController::new(
        job_role_service,
        worker_service,
        assignment_service,
        planning_facade,
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

#[derive(Debug)]
struct AppController {
    job_role_service: JobRoleService,
    worker_service: WorkerService,
    assignment_service: AssignmentService,
    planning_facade: PlanningFacade,
    current_year: i32,
    current_month: u8,
    current_workers: Vec<Worker>,
    current_job_roles: Vec<JobRole>,
    current_planning: Option<MonthlyPlanning>,
    database_path_hint: Option<PathBuf>,
}

impl AppController {
    fn new(
        job_role_service: JobRoleService,
        worker_service: WorkerService,
        assignment_service: AssignmentService,
        planning_facade: PlanningFacade,
        database_path_hint: Option<PathBuf>,
    ) -> Self {
        let today = Local::now().date_naive();

        Self {
            job_role_service,
            worker_service,
            assignment_service,
            planning_facade,
            current_year: today.year(),
            current_month: today.month() as u8,
            current_workers: Vec::new(),
            current_job_roles: Vec::new(),
            current_planning: None,
            database_path_hint,
        }
    }

    fn initialize(&mut self, ui: &AppWindow) -> Result<(), AppError> {
        ui.set_shift_options(model_from_vec(
            ShiftKind::ALL
                .iter()
                .map(|shift| SharedString::from(shift.label()))
                .collect(),
        ));
        ui.set_current_page(0);
        ui.set_status_message(SharedString::new());
        ui.set_assignment_summary(SharedString::new());
        ui.set_worker_delete_confirmation_pending(false);
        ui.set_assignment_delete_confirmation_pending(false);
        ui.set_assignment_existing(false);
        ui.set_db_path_hint(
            self.database_path_hint
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "Base locale".to_owned())
                .into(),
        );
        self.refresh_ui(ui)?;
        self.clear_worker_form(ui);
        self.clear_assignment_form(ui);
        Ok(())
    }

    fn refresh_ui(&mut self, ui: &AppWindow) -> Result<(), AppError> {
        let loaded = self
            .planning_facade
            .load_month(self.current_year, self.current_month)?;
        let workers = loaded.workers().to_vec();
        let planning = loaded.planning().clone();
        let job_roles = self.job_role_service.list_all()?;

        ui.set_role_options(model_from_vec(
            job_roles
                .iter()
                .map(|role| SharedString::from(role.label()))
                .collect(),
        ));
        ui.set_worker_rows(model_from_vec(build_worker_rows(&workers)));
        ui.set_assignment_worker_options(model_from_vec(build_assignment_worker_options(&workers)));
        ui.set_day_headers(model_from_vec(build_day_headers(
            self.current_year,
            self.current_month,
            planning.total_days(),
        )));
        ui.set_planning_rows(model_from_vec(build_planning_rows(&planning)));
        ui.set_total_days(planning.total_days() as i32);
        ui.set_selected_year(self.current_year);
        ui.set_selected_month(self.current_month as i32);
        self.clear_worker_delete_confirmation(ui);
        self.clear_assignment_delete_confirmation(ui);

        if ui.get_assignment_worker_index() >= workers.len() as i32 {
            ui.set_assignment_worker_index(-1);
            ui.set_assignment_existing(false);
        }

        let default_role_index = if job_roles.is_empty() { -1 } else { 0 };

        if ui.get_worker_form_role_index() < 0
            || ui.get_worker_form_role_index() >= job_roles.len() as i32
        {
            ui.set_worker_form_role_index(default_role_index);
        }

        self.current_workers = workers;
        self.current_job_roles = job_roles;
        self.current_planning = Some(planning);
        Ok(())
    }

    fn clear_worker_form(&self, ui: &AppWindow) {
        ui.set_worker_form_id(SharedString::new());
        ui.set_worker_form_last_name(SharedString::new());
        ui.set_worker_form_first_name(SharedString::new());
        ui.set_new_role_name(SharedString::new());
        ui.set_worker_form_role_index(self.default_role_index());
        ui.set_worker_form_existing(false);
        self.clear_worker_delete_confirmation(ui);
        self.clear_status(ui);
    }

    fn select_worker(&self, ui: &AppWindow, worker_id: &str) {
        if let Some(worker) = self
            .current_workers
            .iter()
            .find(|worker| worker.id().as_str() == worker_id)
        {
            ui.set_current_page(0);
            ui.set_worker_form_id(worker.id().to_string().into());
            ui.set_worker_form_last_name(worker.last_name().into());
            ui.set_worker_form_first_name(worker.first_name().into());
            ui.set_worker_form_role_index(self.job_role_to_index(worker.job_role()));
            ui.set_worker_form_existing(true);
            ui.set_new_role_name(SharedString::new());
            self.clear_worker_delete_confirmation(ui);
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

        self.refresh_ui(ui)?;
        self.select_worker(ui, worker.id().as_str());
        self.show_success(ui, "Fiche enregistree.");
        Ok(())
    }

    fn add_job_role(&mut self, ui: &AppWindow, role_name: SharedString) -> Result<(), AppError> {
        let role = self.job_role_service.save_role(role_name.to_string())?;

        self.refresh_ui(ui)?;
        ui.set_new_role_name(SharedString::new());
        ui.set_worker_form_role_index(self.job_role_to_index(&role));
        self.show_success(ui, "Poste ajoute.");
        Ok(())
    }

    fn delete_worker(&mut self, ui: &AppWindow, worker_id: SharedString) -> Result<(), AppError> {
        self.clear_worker_delete_confirmation(ui);
        let worker_id = WorkerId::new(worker_id.to_string())?;
        self.worker_service.delete_worker(&worker_id)?;
        self.refresh_ui(ui)?;
        self.clear_worker_form(ui);
        self.show_success(ui, "Fiche supprimee.");
        Ok(())
    }

    fn clear_assignment_form(&self, ui: &AppWindow) {
        ui.set_assignment_worker_index(-1);
        ui.set_assignment_shift_index(-1);
        ui.set_assignment_day(0);
        ui.set_assignment_summary(SharedString::new());
        ui.set_assignment_existing(false);
        self.clear_assignment_delete_confirmation(ui);
        self.clear_status(ui);
    }

    fn select_cell(&self, ui: &AppWindow, worker_id: &str, day: i32) -> Result<(), AppError> {
        let worker_index = self
            .current_workers
            .iter()
            .position(|worker| worker.id().as_str() == worker_id)
            .ok_or(AppError::MissingWorkerSelection)? as i32;
        let worker = &self.current_workers[worker_index as usize];
        let date = PlanningDate::new(self.current_year, self.current_month, to_day(day)?)?;

        ui.set_current_page(1);
        ui.set_assignment_worker_index(worker_index);
        ui.set_assignment_day(day);
        self.clear_assignment_delete_confirmation(ui);

        if let Some(planning) = &self.current_planning {
            if let Some(cell) = planning
                .row_for_worker(worker.id())
                .and_then(|row| row.cell_for_day(date.day()))
            {
                ui.set_assignment_existing(true);
                ui.set_assignment_shift_index(shift_to_index(cell.shift_kind()));
                ui.set_assignment_summary(
                    format!(
                        "{} - {:02}/{:02}/{:04} - {} {}",
                        worker.display_name(),
                        date.day(),
                        date.month(),
                        date.year(),
                        cell.shift_kind().label(),
                        cell.shift_kind().time_range_label()
                    )
                    .into(),
                );
                self.clear_status(ui);
                return Ok(());
            }
        }

        ui.set_assignment_existing(false);
        ui.set_assignment_shift_index(-1);
        ui.set_assignment_summary(
            format!(
                "{} - {:02}/{:02}/{:04} - Libre",
                worker.display_name(),
                date.day(),
                date.month(),
                date.year()
            )
            .into(),
        );
        self.clear_status(ui);
        Ok(())
    }

    fn update_assignment_worker(&self, ui: &AppWindow, worker_index: i32) -> Result<(), AppError> {
        ui.set_assignment_worker_index(worker_index);
        self.clear_assignment_delete_confirmation(ui);

        if worker_index < 0 {
            ui.set_assignment_existing(false);
            ui.set_assignment_shift_index(-1);
            ui.set_assignment_summary(SharedString::new());
            self.clear_status(ui);
            return Ok(());
        }

        if ui.get_assignment_day() > 0 {
            let worker_id = self.worker_id_from_index(worker_index)?.clone();
            self.select_cell(ui, worker_id.as_str(), ui.get_assignment_day())?;
            return Ok(());
        }

        ui.set_assignment_existing(false);
        ui.set_assignment_shift_index(-1);
        ui.set_assignment_summary(SharedString::new());
        self.clear_status(ui);
        Ok(())
    }

    fn update_assignment_shift(&self, ui: &AppWindow, shift_index: i32) -> Result<(), AppError> {
        ui.set_assignment_shift_index(shift_index);
        self.clear_assignment_delete_confirmation(ui);

        if ui.get_assignment_worker_index() < 0 || ui.get_assignment_day() <= 0 {
            if shift_index < 0 {
                ui.set_assignment_summary(SharedString::new());
            }
            self.clear_status(ui);
            return Ok(());
        }

        self.refresh_assignment_summary(ui)?;
        self.clear_status(ui);
        Ok(())
    }

    fn save_assignment(
        &mut self,
        ui: &AppWindow,
        worker_index: i32,
        year: i32,
        month: i32,
        day: i32,
        shift_index: i32,
    ) -> Result<(), AppError> {
        let worker_id = self.worker_id_from_index(worker_index)?.clone();
        let date = PlanningDate::new(year, to_month(month)?, to_day(day)?)?;
        let shift_kind = shift_from_index(shift_index)?;

        self.clear_assignment_delete_confirmation(ui);
        self.assignment_service
            .upsert_assignment(&worker_id, date, shift_kind)?;
        self.refresh_ui(ui)?;
        self.select_cell(ui, worker_id.as_str(), day)?;
        self.show_success(ui, "Affectation enregistree.");
        Ok(())
    }

    fn delete_assignment(
        &mut self,
        ui: &AppWindow,
        worker_index: i32,
        year: i32,
        month: i32,
        day: i32,
    ) -> Result<(), AppError> {
        self.clear_assignment_delete_confirmation(ui);
        if !ui.get_assignment_existing() {
            return Err(AppError::MissingAssignmentSelection);
        }
        let worker_id = self.worker_id_from_index(worker_index)?.clone();
        let date = PlanningDate::new(year, to_month(month)?, to_day(day)?)?;

        self.assignment_service
            .delete_assignment(&worker_id, date)?;
        self.refresh_ui(ui)?;
        self.select_cell(ui, worker_id.as_str(), day)?;
        self.show_success(ui, "Affectation supprimee.");
        Ok(())
    }

    fn previous_month(&mut self, ui: &AppWindow) -> Result<(), AppError> {
        if self.current_month == 1 {
            self.current_month = 12;
            self.current_year -= 1;
        } else {
            self.current_month -= 1;
        }

        self.refresh_ui(ui)?;
        self.clear_assignment_form(ui);
        self.clear_status(ui);
        Ok(())
    }

    fn next_month(&mut self, ui: &AppWindow) -> Result<(), AppError> {
        if self.current_month == 12 {
            self.current_month = 1;
            self.current_year += 1;
        } else {
            self.current_month += 1;
        }

        self.refresh_ui(ui)?;
        self.clear_assignment_form(ui);
        self.clear_status(ui);
        Ok(())
    }

    fn reload_month(&mut self, ui: &AppWindow, year: i32, month: i32) -> Result<(), AppError> {
        self.current_year = year;
        self.current_month = to_month(month)?;
        self.refresh_ui(ui)?;
        self.clear_assignment_form(ui);
        self.clear_status(ui);
        Ok(())
    }

    fn worker_id_from_index(&self, worker_index: i32) -> Result<&WorkerId, AppError> {
        if worker_index < 0 {
            return Err(AppError::MissingWorkerSelection);
        }

        self.current_workers
            .get(worker_index as usize)
            .map(|worker| worker.id())
            .ok_or(AppError::MissingWorkerSelection)
    }

    fn job_role_from_index(&self, role_index: i32) -> Result<JobRole, AppError> {
        if role_index < 0 {
            return Err(AppError::MissingJobRoleSelection);
        }

        self.current_job_roles
            .get(role_index as usize)
            .cloned()
            .ok_or(AppError::MissingJobRoleSelection)
    }

    fn job_role_to_index(&self, role: &JobRole) -> i32 {
        self.current_job_roles
            .iter()
            .position(|candidate| candidate == role)
            .map(|index| index as i32)
            .unwrap_or(self.default_role_index())
    }

    fn default_role_index(&self) -> i32 {
        if self.current_job_roles.is_empty() {
            -1
        } else {
            0
        }
    }

    fn request_worker_delete_confirmation(&self, ui: &AppWindow) -> Result<(), AppError> {
        if !ui.get_worker_form_existing() || ui.get_worker_form_id().is_empty() {
            return Err(AppError::MissingWorkerSelection);
        }

        ui.set_worker_delete_confirmation_pending(true);
        self.clear_status(ui);
        Ok(())
    }

    fn cancel_worker_delete_confirmation(&self, ui: &AppWindow) {
        self.clear_worker_delete_confirmation(ui);
        self.clear_status(ui);
    }

    fn clear_worker_delete_confirmation(&self, ui: &AppWindow) {
        ui.set_worker_delete_confirmation_pending(false);
    }

    fn request_assignment_delete_confirmation(&self, ui: &AppWindow) -> Result<(), AppError> {
        if ui.get_assignment_worker_index() < 0 {
            return Err(AppError::MissingWorkerSelection);
        }

        if ui.get_assignment_day() <= 0 {
            return Err(AppError::InvalidDayInput(ui.get_assignment_day()));
        }

        if !ui.get_assignment_existing() {
            return Err(AppError::MissingAssignmentSelection);
        }

        ui.set_assignment_delete_confirmation_pending(true);
        self.clear_status(ui);
        Ok(())
    }

    fn cancel_assignment_delete_confirmation(&self, ui: &AppWindow) {
        self.clear_assignment_delete_confirmation(ui);
        self.clear_status(ui);
    }

    fn clear_assignment_delete_confirmation(&self, ui: &AppWindow) {
        ui.set_assignment_delete_confirmation_pending(false);
    }

    fn refresh_assignment_summary(&self, ui: &AppWindow) -> Result<(), AppError> {
        let worker_index = ui.get_assignment_worker_index();
        let day = ui.get_assignment_day();

        if worker_index < 0 || day <= 0 {
            ui.set_assignment_summary(SharedString::new());
            return Ok(());
        }

        let worker = self
            .current_workers
            .get(worker_index as usize)
            .ok_or(AppError::MissingWorkerSelection)?;
        let date = PlanningDate::new(self.current_year, self.current_month, to_day(day)?)?;
        let base = format!(
            "{} - {:02}/{:02}/{:04}",
            worker.display_name(),
            date.day(),
            date.month(),
            date.year()
        );

        let summary = if ui.get_assignment_shift_index() >= 0 {
            let shift_kind = shift_from_index(ui.get_assignment_shift_index())?;

            if ui.get_assignment_existing() {
                format!(
                    "{base} - Edition {} {}",
                    shift_kind.label(),
                    shift_kind.time_range_label()
                )
            } else {
                format!(
                    "{base} - Preparation {} {}",
                    shift_kind.label(),
                    shift_kind.time_range_label()
                )
            }
        } else {
            format!("{base} - Libre")
        };

        ui.set_assignment_summary(summary.into());
        Ok(())
    }

    fn clear_status(&self, ui: &AppWindow) {
        ui.set_status_message(SharedString::new());
    }

    fn show_error(&self, ui: &AppWindow, error: AppError) {
        ui.set_status_message(error.to_string().into());
    }

    fn show_success(&self, ui: &AppWindow, message: impl Into<SharedString>) {
        ui.set_status_message(message.into());
    }
}

fn attach_callbacks(ui: &AppWindow, controller: Rc<RefCell<AppController>>) {
    let weak = ui.as_weak();
    let controller_clear_worker = controller.clone();
    ui.on_clear_worker_form(move || {
        if let Some(ui) = weak.upgrade() {
            controller_clear_worker.borrow().clear_worker_form(&ui);
        }
    });

    let weak = ui.as_weak();
    let controller_select_worker = controller.clone();
    ui.on_select_worker(move |worker_id| {
        if let Some(ui) = weak.upgrade() {
            controller_select_worker
                .borrow()
                .select_worker(&ui, worker_id.as_str());
        }
    });

    let weak = ui.as_weak();
    let controller_save_worker = controller.clone();
    ui.on_save_worker(move |worker_id, last_name, first_name, role_index| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_save_worker
                .borrow_mut()
                .save_worker(&ui, worker_id, last_name, first_name, role_index)
            {
                controller_save_worker.borrow().show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    let controller_add_job_role = controller.clone();
    ui.on_add_job_role(move |role_name| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_add_job_role
                .borrow_mut()
                .add_job_role(&ui, role_name)
            {
                controller_add_job_role.borrow().show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    let controller_request_worker_delete = controller.clone();
    ui.on_request_worker_delete_confirmation(move || {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_request_worker_delete
                .borrow()
                .request_worker_delete_confirmation(&ui)
            {
                controller_request_worker_delete
                    .borrow()
                    .show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    let controller_cancel_worker_delete = controller.clone();
    ui.on_cancel_worker_delete_confirmation(move || {
        if let Some(ui) = weak.upgrade() {
            controller_cancel_worker_delete
                .borrow()
                .cancel_worker_delete_confirmation(&ui);
        }
    });

    let weak = ui.as_weak();
    let controller_delete_worker = controller.clone();
    ui.on_delete_worker(move |worker_id| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_delete_worker
                .borrow_mut()
                .delete_worker(&ui, worker_id)
            {
                controller_delete_worker.borrow().show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    let controller_previous_month = controller.clone();
    ui.on_previous_month(move || {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_previous_month.borrow_mut().previous_month(&ui) {
                controller_previous_month.borrow().show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    let controller_next_month = controller.clone();
    ui.on_next_month(move || {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_next_month.borrow_mut().next_month(&ui) {
                controller_next_month.borrow().show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    let controller_load_month = controller.clone();
    ui.on_load_month(move |year, month| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_load_month
                .borrow_mut()
                .reload_month(&ui, year, month)
            {
                controller_load_month.borrow().show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    let controller_clear_assignment = controller.clone();
    ui.on_clear_assignment_form(move || {
        if let Some(ui) = weak.upgrade() {
            controller_clear_assignment
                .borrow()
                .clear_assignment_form(&ui);
        }
    });

    let weak = ui.as_weak();
    let controller_select_cell = controller.clone();
    ui.on_select_cell(move |worker_id, day| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) =
                controller_select_cell
                    .borrow()
                    .select_cell(&ui, worker_id.as_str(), day)
            {
                controller_select_cell.borrow().show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    let controller_assignment_worker_changed = controller.clone();
    ui.on_assignment_worker_changed(move |worker_index| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_assignment_worker_changed
                .borrow()
                .update_assignment_worker(&ui, worker_index)
            {
                controller_assignment_worker_changed
                    .borrow()
                    .show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    let controller_assignment_shift_changed = controller.clone();
    ui.on_assignment_shift_changed(move |shift_index| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_assignment_shift_changed
                .borrow()
                .update_assignment_shift(&ui, shift_index)
            {
                controller_assignment_shift_changed
                    .borrow()
                    .show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    let controller_request_assignment_delete = controller.clone();
    ui.on_request_assignment_delete_confirmation(move || {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_request_assignment_delete
                .borrow()
                .request_assignment_delete_confirmation(&ui)
            {
                controller_request_assignment_delete
                    .borrow()
                    .show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    let controller_cancel_assignment_delete = controller.clone();
    ui.on_cancel_assignment_delete_confirmation(move || {
        if let Some(ui) = weak.upgrade() {
            controller_cancel_assignment_delete
                .borrow()
                .cancel_assignment_delete_confirmation(&ui);
        }
    });

    let weak = ui.as_weak();
    let controller_save_assignment = controller.clone();
    ui.on_save_assignment(move |worker_index, year, month, day, shift_index| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_save_assignment.borrow_mut().save_assignment(
                &ui,
                worker_index,
                year,
                month,
                day,
                shift_index,
            ) {
                controller_save_assignment.borrow().show_error(&ui, error);
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_delete_assignment(move |worker_index, year, month, day| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) =
                controller
                    .borrow_mut()
                    .delete_assignment(&ui, worker_index, year, month, day)
            {
                controller.borrow().show_error(&ui, error);
            }
        }
    });
}

fn model_from_vec<T: Clone + 'static>(values: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(values))
}

fn build_worker_rows(workers: &[Worker]) -> Vec<WorkerRowData> {
    workers
        .iter()
        .map(|worker| WorkerRowData {
            id: worker.id().to_string().into(),
            last_name: worker.last_name().into(),
            first_name: worker.first_name().into(),
            job_role_label: worker.job_role().label().into(),
        })
        .collect()
}

fn build_assignment_worker_options(workers: &[Worker]) -> Vec<SharedString> {
    workers
        .iter()
        .map(|worker| format!("{} - {}", worker.display_name(), worker.job_role().label()).into())
        .collect()
}

fn build_day_headers(year: i32, month: u8, total_days: u8) -> Vec<DayHeaderData> {
    (1..=total_days)
        .map(|day| DayHeaderData {
            day: day as i32,
            label: day.to_string().into(),
            weekday_label: weekday_label_from_date(year, month, day).into(),
            is_weekend: is_weekend_date(year, month, day),
        })
        .collect()
}

fn weekday_label_from_date(year: i32, month: u8, day: u8) -> &'static str {
    NaiveDate::from_ymd_opt(year, month as u32, day as u32)
        .map(|date| match date.weekday() {
            Weekday::Mon => "Lu",
            Weekday::Tue => "Ma",
            Weekday::Wed => "Me",
            Weekday::Thu => "Je",
            Weekday::Fri => "Ve",
            Weekday::Sat => "Sa",
            Weekday::Sun => "Di",
        })
        .unwrap_or("")
}

fn is_weekend_date(year: i32, month: u8, day: u8) -> bool {
    NaiveDate::from_ymd_opt(year, month as u32, day as u32)
        .map(|date| matches!(date.weekday(), Weekday::Sat | Weekday::Sun))
        .unwrap_or(false)
}

fn build_planning_rows(planning: &MonthlyPlanning) -> Vec<PlanningRowData> {
    planning
        .rows()
        .iter()
        .map(|row| PlanningRowData {
            worker_id: row.worker_id().to_string().into(),
            worker_name: row.worker_name().into(),
            job_role_label: row.job_role().label().into(),
            cells: model_from_vec(build_planning_cells(planning, row)),
        })
        .collect()
}

fn build_planning_cells(planning: &MonthlyPlanning, row: &PlanningRow) -> Vec<PlanningCellData> {
    (1..=planning.total_days())
        .map(|day| match row.cell_for_day(day) {
            Some(cell) => PlanningCellData {
                day: day as i32,
                short_code: cell.shift_kind().short_code().into(),
                shift_label: cell.shift_kind().label().into(),
                time_range: cell.shift_kind().time_range_label().into(),
                style_token: cell.style_key().token().into(),
                has_assignment: true,
            },
            None => PlanningCellData {
                day: day as i32,
                short_code: SharedString::new(),
                shift_label: SharedString::new(),
                time_range: SharedString::new(),
                style_token: "empty".into(),
                has_assignment: false,
            },
        })
        .collect()
}

fn non_empty_shared_string(value: &SharedString) -> Option<String> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn shift_from_index(index: i32) -> Result<ShiftKind, AppError> {
    if index < 0 {
        return Err(AppError::MissingShiftSelection);
    }

    ShiftKind::ALL
        .get(index as usize)
        .copied()
        .ok_or_else(|| AppError::InvalidShiftKind(index.to_string()))
}

fn shift_to_index(shift_kind: ShiftKind) -> i32 {
    ShiftKind::ALL
        .iter()
        .position(|candidate| *candidate == shift_kind)
        .map(|index| index as i32)
        .unwrap_or(-1)
}

fn to_month(value: i32) -> Result<u8, AppError> {
    if !(1..=12).contains(&value) {
        return Err(AppError::InvalidMonthInput(value));
    }

    Ok(value as u8)
}

fn to_day(value: i32) -> Result<u8, AppError> {
    if value <= 0 {
        return Err(AppError::InvalidDayInput(value));
    }

    Ok(value as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::PlanningGenerator;
    use crate::domain::Assignment;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::{Mutex, MutexGuard, Once, OnceLock};

    fn ensure_ui_test_backend() {
        static INIT: Once = Once::new();

        INIT.call_once(i_slint_backend_testing::init_integration_test_with_mock_time);
    }

    fn ui_test_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn role(label: &str) -> JobRole {
        JobRole::new(label).unwrap()
    }

    #[derive(Default)]
    struct FakeStartupWindow {
        maximized_requested: Cell<bool>,
    }

    impl StartupWindowControl for FakeStartupWindow {
        fn request_maximized(&self) {
            self.maximized_requested.set(true);
        }
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

    fn build_test_controller() -> (AppWindow, AppController, WorkerService, AssignmentService) {
        ensure_ui_test_backend();

        let database = Rc::new(SqliteDatabase::open_in_memory().unwrap());
        let worker_service = WorkerService::new(database.clone());
        let assignment_service = AssignmentService::new(database.clone());
        let controller = AppController::new(
            JobRoleService::new(database.clone()),
            worker_service.clone(),
            assignment_service.clone(),
            PlanningFacade::new(worker_service.clone(), assignment_service.clone()),
            database.database_path_hint(),
        );
        let ui = AppWindow::new().unwrap();

        (ui, controller, worker_service, assignment_service)
    }

    #[test]
    fn ui_mapping_builds_filled_and_empty_cells() {
        let workers = vec![worker(
            "worker-01",
            "Martin",
            "Alice",
            "Operateur de production",
        )];
        let assignments = vec![Assignment::new(
            WorkerId::new("worker-01").unwrap(),
            PlanningDate::new(2026, 4, 8).unwrap(),
            ShiftKind::Night,
        )];
        let planning = PlanningGenerator::build_month(2026, 4, &workers, &assignments).unwrap();
        let cells = build_planning_cells(&planning, &planning.rows()[0]);

        assert_eq!(cells.len(), planning.total_days() as usize);
        assert!(cells[7].has_assignment);
        assert_eq!(cells[7].style_token, SharedString::from("night-blue"));
        assert!(!cells[0].has_assignment);
    }

    #[test]
    fn ui_mapping_builds_worker_rows_and_options() {
        let workers = vec![
            worker("worker-01", "Martin", "Alice", "Operateur de production"),
            worker("worker-02", "Leroy", "Bruno", "Chef d'equipes"),
        ];

        let rows = build_worker_rows(&workers);
        let options = build_assignment_worker_options(&workers);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].last_name, SharedString::from("Martin"));
        assert_eq!(rows[0].first_name, SharedString::from("Alice"));
        assert_eq!(rows[1].job_role_label, SharedString::from("Chef d'equipes"));
        assert!(options[0].contains("Martin Alice"));
        assert!(options[1].contains("Chef d'equipes"));
    }

    #[test]
    fn ui_mapping_builds_day_headers_with_weekday_context() {
        let headers = build_day_headers(2026, 4, 7);

        assert_eq!(headers.len(), 7);
        assert_eq!(headers[0].label, SharedString::from("1"));
        assert_eq!(headers[0].weekday_label, SharedString::from("Me"));
        assert!(!headers[0].is_weekend);
        assert_eq!(headers[3].weekday_label, SharedString::from("Sa"));
        assert!(headers[3].is_weekend);
        assert_eq!(headers[4].weekday_label, SharedString::from("Di"));
        assert!(headers[4].is_weekend);
    }

    #[test]
    fn non_empty_shared_string_returns_none_for_blank_values() {
        assert_eq!(non_empty_shared_string(&SharedString::from("  ")), None);
        assert_eq!(
            non_empty_shared_string(&SharedString::from(" worker-01 ")),
            Some("worker-01".to_owned())
        );
    }

    #[test]
    fn startup_window_requests_maximized_state() {
        let window = FakeStartupWindow::default();

        configure_startup_window(&window);

        assert!(window.maximized_requested.get());
    }

    #[test]
    fn ui_window_state_and_confirmation_flows_are_consistent() {
        let _guard = ui_test_lock();
        ensure_ui_test_backend();

        let ui = AppWindow::new().unwrap();
        schedule_startup_window(&ui).unwrap();

        assert_eq!(
            ui.get_worker_form_mode_label(),
            SharedString::from("Nouvelle fiche")
        );
        assert_eq!(
            ui.get_worker_form_identifier_label(),
            SharedString::from("Genere automatiquement a l'enregistrement")
        );
        assert_eq!(
            ui.get_worker_form_action_hint_label(),
            SharedString::from("Je complete d'abord la fiche pour activer l'enregistrement.")
        );
        assert_eq!(
            ui.get_planning_selection_label(),
            SharedString::from("Aucune cellule selectionnee")
        );
        assert_eq!(
            ui.get_planning_selection_state_label(),
            SharedString::from(
                "Je selectionne une cellule dans la grille pour preparer une affectation."
            )
        );
        assert_eq!(
            ui.get_planning_assignment_action_hint_label(),
            SharedString::from(
                "Je selectionne un effectif, un jour et un horaire avant d'enregistrer."
            )
        );

        assert!(!ui.get_can_add_job_role());
        assert!(!ui.get_can_save_worker());
        assert!(!ui.get_can_save_assignment());
        assert!(!ui.get_can_delete_assignment());

        ui.set_new_role_name("Chef de ligne".into());
        assert!(ui.get_can_add_job_role());

        ui.set_worker_form_last_name("Martin".into());
        ui.set_worker_form_first_name("Alice".into());
        ui.set_worker_form_role_index(0);
        ui.set_worker_form_existing(true);
        assert!(ui.get_can_save_worker());
        assert!(ui.get_can_delete_worker());
        assert_eq!(
            ui.get_worker_form_mode_label(),
            SharedString::from("Modification")
        );
        assert_eq!(
            ui.get_worker_form_validation_label(),
            SharedString::from("La fiche est prete a etre enregistree.")
        );
        assert_eq!(
            ui.get_worker_form_action_hint_label(),
            SharedString::from("Le bouton d'enregistrement reste disponible en bas de la fiche.")
        );

        ui.set_assignment_worker_index(0);
        ui.set_assignment_day(8);
        ui.set_assignment_shift_index(2);
        assert!(ui.get_can_save_assignment());
        assert!(!ui.get_can_delete_assignment());
        assert_eq!(
            ui.get_planning_selection_label(),
            SharedString::from(format!("Jour 8 / {}", ui.get_planning_period_label()))
        );
        assert_eq!(
            ui.get_planning_selection_state_label(),
            SharedString::from("Cette nouvelle affectation est prete a etre enregistree.")
        );
        assert_eq!(
            ui.get_planning_assignment_action_hint_label(),
            SharedString::from(
                "Le bouton d'enregistrement reste disponible dans la barre d'actions."
            )
        );

        ui.set_assignment_existing(true);
        assert!(ui.get_can_delete_assignment());
        assert_eq!(
            ui.get_planning_selection_state_label(),
            SharedString::from(
                "Cette cellule contient deja une affectation que je peux modifier ou supprimer."
            )
        );

        ui.set_assignment_shift_index(-1);
        assert!(ui.get_can_delete_assignment());
        assert_eq!(
            ui.get_planning_selection_state_label(),
            SharedString::from(
                "Cette cellule contient deja une affectation que je peux modifier ou supprimer."
            )
        );
        ui.set_assignment_existing(false);
        assert!(!ui.get_can_delete_assignment());
        assert_eq!(
            ui.get_planning_selection_state_label(),
            SharedString::from(
                "Cette cellule est libre. Je choisis un horaire pour l'enregistrer."
            )
        );

        let (ui, mut controller, worker_service, assignment_service) = build_test_controller();
        worker_service
            .save_worker(
                Some("worker-01".to_owned()),
                "Martin",
                "Alice",
                role("Operateur de production"),
            )
            .unwrap();
        worker_service
            .save_worker(
                Some("worker-02".to_owned()),
                "Leroy",
                "Bruno",
                role("Chef d'equipes"),
            )
            .unwrap();
        let worker = worker_service
            .list_all()
            .unwrap()
            .into_iter()
            .find(|worker| worker.id().as_str() == "worker-01")
            .unwrap();
        assignment_service
            .upsert_assignment(
                worker.id(),
                PlanningDate::new(2026, 4, 8).unwrap(),
                ShiftKind::Night,
            )
            .unwrap();
        controller.initialize(&ui).unwrap();

        assert!(matches!(
            controller.request_assignment_delete_confirmation(&ui),
            Err(AppError::MissingWorkerSelection)
        ));

        controller
            .select_cell(&ui, worker.id().as_str(), 7)
            .unwrap();
        assert!(matches!(
            controller.request_assignment_delete_confirmation(&ui),
            Err(AppError::MissingAssignmentSelection)
        ));

        controller
            .select_cell(&ui, worker.id().as_str(), 8)
            .unwrap();
        controller
            .request_assignment_delete_confirmation(&ui)
            .unwrap();
        assert!(ui.get_assignment_delete_confirmation_pending());
        assert_eq!(
            ui.get_planning_assignment_action_hint_label(),
            SharedString::from("Je confirme ou j'annule d'abord la suppression en cours.")
        );

        controller.cancel_assignment_delete_confirmation(&ui);
        assert!(!ui.get_assignment_delete_confirmation_pending());

        controller
            .request_assignment_delete_confirmation(&ui)
            .unwrap();
        controller
            .select_cell(&ui, worker.id().as_str(), 7)
            .unwrap();
        assert!(!ui.get_assignment_delete_confirmation_pending());

        let worker_b_index = controller
            .current_workers
            .iter()
            .position(|candidate| candidate.id().as_str() == "worker-02")
            .unwrap() as i32;
        controller
            .select_cell(&ui, worker.id().as_str(), 8)
            .unwrap();
        controller
            .request_assignment_delete_confirmation(&ui)
            .unwrap();
        controller
            .update_assignment_worker(&ui, worker_b_index)
            .unwrap();
        assert_eq!(ui.get_assignment_worker_index(), worker_b_index);
        assert_eq!(ui.get_assignment_day(), 8);
        assert!(!ui.get_assignment_existing());
        assert_eq!(ui.get_assignment_shift_index(), -1);
        assert!(!ui.get_can_delete_assignment());
        assert!(!ui.get_assignment_delete_confirmation_pending());

        controller
            .update_assignment_shift(&ui, shift_to_index(ShiftKind::Day))
            .unwrap();
        assert!(ui.get_can_save_assignment());
        assert!(!ui.get_can_delete_assignment());
        assert!(
            ui.get_assignment_summary()
                .to_string()
                .contains("Preparation Journee 08h30 - 16h30")
        );
        assert!(matches!(
            controller.request_assignment_delete_confirmation(&ui),
            Err(AppError::MissingAssignmentSelection)
        ));

        controller.clear_assignment_form(&ui);
        assert_eq!(ui.get_assignment_worker_index(), -1);
        assert_eq!(ui.get_assignment_shift_index(), -1);
        assert_eq!(ui.get_assignment_day(), 0);
        assert!(!ui.get_assignment_existing());
        assert!(!ui.get_assignment_delete_confirmation_pending());

        assert!(matches!(
            controller.request_worker_delete_confirmation(&ui),
            Err(AppError::MissingWorkerSelection)
        ));

        controller.select_worker(&ui, worker.id().as_str());
        controller.request_worker_delete_confirmation(&ui).unwrap();
        assert!(ui.get_worker_delete_confirmation_pending());

        controller.cancel_worker_delete_confirmation(&ui);
        assert!(!ui.get_worker_delete_confirmation_pending());

        controller.request_worker_delete_confirmation(&ui).unwrap();
        controller.clear_worker_form(&ui);
        assert!(!ui.get_worker_delete_confirmation_pending());
    }
}
