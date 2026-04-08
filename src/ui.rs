use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use chrono::{Datelike, Local};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::application::{AssignmentService, PlanningFacade, WorkerService};
use crate::domain::{
    JobRole, MonthlyPlanning, PlanningDate, PlanningRow, ShiftKind, Worker, WorkerId,
};
use crate::error::AppError;
use crate::infrastructure::SqliteDatabase;

slint::include_modules!();

pub fn run() -> Result<(), AppError> {
    let database = Rc::new(SqliteDatabase::open_or_create_default()?);
    let worker_service = WorkerService::new(database.clone());
    let assignment_service = AssignmentService::new(database.clone());
    let planning_facade = PlanningFacade::new(worker_service.clone(), assignment_service.clone());
    let controller = Rc::new(RefCell::new(AppController::new(
        worker_service,
        assignment_service,
        planning_facade,
        database.database_path_hint(),
    )));

    let ui = AppWindow::new()?;
    attach_callbacks(&ui, controller.clone());
    controller.borrow_mut().initialize(&ui)?;
    ui.run()?;
    Ok(())
}

#[derive(Debug)]
struct AppController {
    worker_service: WorkerService,
    assignment_service: AssignmentService,
    planning_facade: PlanningFacade,
    current_year: i32,
    current_month: u8,
    current_workers: Vec<Worker>,
    current_planning: Option<MonthlyPlanning>,
    database_path_hint: Option<PathBuf>,
}

impl AppController {
    fn new(
        worker_service: WorkerService,
        assignment_service: AssignmentService,
        planning_facade: PlanningFacade,
        database_path_hint: Option<PathBuf>,
    ) -> Self {
        let today = Local::now().date_naive();

        Self {
            worker_service,
            assignment_service,
            planning_facade,
            current_year: today.year(),
            current_month: today.month() as u8,
            current_workers: Vec::new(),
            current_planning: None,
            database_path_hint,
        }
    }

    fn initialize(&mut self, ui: &AppWindow) -> Result<(), AppError> {
        ui.set_role_options(model_from_vec(
            JobRole::ALL
                .iter()
                .map(|role| SharedString::from(role.label()))
                .collect(),
        ));
        ui.set_shift_options(model_from_vec(
            ShiftKind::ALL
                .iter()
                .map(|shift| SharedString::from(shift.label()))
                .collect(),
        ));
        ui.set_current_page(0);
        ui.set_status_message(SharedString::new());
        ui.set_assignment_summary(
            "Selectionnez une case du planning pour preparer une affectation.".into(),
        );
        ui.set_db_path_hint(
            self.database_path_hint
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "Base locale".to_owned())
                .into(),
        );
        self.clear_worker_form(ui);
        self.clear_assignment_form(ui);
        self.refresh_ui(ui)?;
        Ok(())
    }

    fn refresh_ui(&mut self, ui: &AppWindow) -> Result<(), AppError> {
        let loaded = self
            .planning_facade
            .load_month(self.current_year, self.current_month)?;
        let workers = loaded.workers().to_vec();
        let planning = loaded.planning().clone();

        ui.set_worker_rows(model_from_vec(build_worker_rows(&workers)));
        ui.set_assignment_worker_options(model_from_vec(build_assignment_worker_options(&workers)));
        ui.set_day_headers(model_from_vec(build_day_headers(planning.total_days())));
        ui.set_planning_rows(model_from_vec(build_planning_rows(&planning)));
        ui.set_total_days(planning.total_days() as i32);
        ui.set_selected_year(self.current_year);
        ui.set_selected_month(self.current_month as i32);

        if ui.get_assignment_worker_index() >= workers.len() as i32 {
            ui.set_assignment_worker_index(-1);
        }

        self.current_workers = workers;
        self.current_planning = Some(planning);
        Ok(())
    }

    fn clear_worker_form(&self, ui: &AppWindow) {
        ui.set_worker_form_id(SharedString::new());
        ui.set_worker_form_name(SharedString::new());
        ui.set_worker_form_role_index(0);
        ui.set_worker_form_existing(false);
    }

    fn select_worker(&self, ui: &AppWindow, worker_id: &str) {
        if let Some(worker) = self
            .current_workers
            .iter()
            .find(|worker| worker.id().as_str() == worker_id)
        {
            ui.set_current_page(0);
            ui.set_worker_form_id(worker.id().to_string().into());
            ui.set_worker_form_name(worker.display_name().into());
            ui.set_worker_form_role_index(job_role_to_index(worker.job_role()));
            ui.set_worker_form_existing(true);
            self.show_success(ui, "Ouvrier charge dans le formulaire.");
        }
    }

    fn save_worker(
        &mut self,
        ui: &AppWindow,
        worker_id: SharedString,
        display_name: SharedString,
        role_index: i32,
    ) -> Result<(), AppError> {
        let job_role = role_from_index(role_index)?;
        let worker = self.worker_service.save_worker(
            worker_id.to_string(),
            display_name.to_string(),
            job_role,
        )?;

        self.refresh_ui(ui)?;
        self.select_worker(ui, worker.id().as_str());
        self.show_success(ui, "Ouvrier enregistre avec succes.");
        Ok(())
    }

    fn delete_worker(&mut self, ui: &AppWindow, worker_id: SharedString) -> Result<(), AppError> {
        let worker_id = WorkerId::new(worker_id.to_string())?;
        self.worker_service.delete_worker(&worker_id)?;
        self.refresh_ui(ui)?;
        self.clear_worker_form(ui);
        self.show_success(ui, "Ouvrier supprime.");
        Ok(())
    }

    fn clear_assignment_form(&self, ui: &AppWindow) {
        ui.set_assignment_worker_index(-1);
        ui.set_assignment_shift_index(-1);
        ui.set_assignment_day(0);
        ui.set_assignment_summary(
            "Selectionnez une case du planning pour preparer une affectation.".into(),
        );
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

        if let Some(planning) = &self.current_planning {
            if let Some(cell) = planning
                .row_for_worker(worker.id())
                .and_then(|row| row.cell_for_day(date.day()))
            {
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
                return Ok(());
            }
        }

        ui.set_assignment_shift_index(-1);
        ui.set_assignment_summary(
            format!(
                "{} - {:02}/{:02}/{:04} - Case libre",
                worker.display_name(),
                date.day(),
                date.month(),
                date.year()
            )
            .into(),
        );
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
        self.show_success(ui, "Mois precedent charge.");
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
        self.show_success(ui, "Mois suivant charge.");
        Ok(())
    }

    fn reload_month(&mut self, ui: &AppWindow, year: i32, month: i32) -> Result<(), AppError> {
        self.current_year = year;
        self.current_month = to_month(month)?;
        self.refresh_ui(ui)?;
        self.clear_assignment_form(ui);
        self.show_success(ui, "Planning recharge.");
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
    ui.on_save_worker(move |worker_id, display_name, role_index| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = controller_save_worker.borrow_mut().save_worker(
                &ui,
                worker_id,
                display_name,
                role_index,
            ) {
                controller_save_worker.borrow().show_error(&ui, error);
            }
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
            display_name: worker.display_name().into(),
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

fn build_day_headers(total_days: u8) -> Vec<DayHeaderData> {
    (1..=total_days)
        .map(|day| DayHeaderData {
            day: day as i32,
            label: day.to_string().into(),
        })
        .collect()
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

fn role_from_index(index: i32) -> Result<JobRole, AppError> {
    JobRole::ALL
        .get(index as usize)
        .copied()
        .ok_or_else(|| AppError::InvalidJobRole(index.to_string()))
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

fn job_role_to_index(role: JobRole) -> i32 {
    JobRole::ALL
        .iter()
        .position(|candidate| *candidate == role)
        .map(|index| index as i32)
        .unwrap_or(0)
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

    fn worker(worker_id: &str, display_name: &str, job_role: JobRole) -> Worker {
        Worker::new(WorkerId::new(worker_id).unwrap(), display_name, job_role).unwrap()
    }

    #[test]
    fn ui_mapping_builds_filled_and_empty_cells() {
        let workers = vec![worker(
            "worker-01",
            "Alice Martin",
            JobRole::OperateurProduction,
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
    fn ui_mapping_builds_worker_options() {
        let workers = vec![
            worker("worker-01", "Alice Martin", JobRole::OperateurProduction),
            worker("worker-02", "Bruno Leroy", JobRole::ChefDEquipes),
        ];
        let options = build_assignment_worker_options(&workers);

        assert_eq!(options.len(), 2);
        assert!(options[0].contains("Alice Martin"));
        assert!(options[1].contains("Chef d'equipes"));
    }
}
