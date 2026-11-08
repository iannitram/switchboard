//! Tauri commands: the GUI face's only door into the core. Each returns JSON the React face
//! renders, or the core's error as `{ "error": { "command", "lines" } }` (SHELL-3).

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use switchboard_core::budget::{self, PoolEdit, PoolRow};
use switchboard_core::clock::SystemClock;
use switchboard_core::fleet::{FleetBridge, FleetError, FleetErrorLog};
use switchboard_core::waiting::LocalItems;
use switchboard_core::{settings, CoreError};

/// What the shell holds between commands: the settings file's path, the app data folder and
/// this session's fleet error log.
pub struct AppState {
    pub settings_path: PathBuf,
    pub data_dir: PathBuf,
    pub errors: Mutex<FleetErrorLog>,
}

impl AppState {
    pub fn new(settings_path: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            settings_path,
            data_dir,
            errors: Mutex::new(FleetErrorLog::default()),
        }
    }
}

/// A core error as the face reads it: the command's name and the first 5 lines of the message.
#[derive(Debug, Serialize)]
pub struct FaceError {
    pub command: String,
    pub lines: Vec<String>,
}

impl FaceError {
    fn new(command: &str, err: &CoreError) -> Self {
        Self {
            command: command.to_string(),
            lines: err
                .to_string()
                .lines()
                .take(5)
                .map(str::to_string)
                .collect(),
        }
    }
}

/// CORE-2 as the face reads a recorded fleet error. The `reason` is not sent.
#[derive(Debug, Serialize)]
pub struct FleetErrorRow {
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub stderr: String,
}

impl From<&FleetError> for FleetErrorRow {
    fn from(err: &FleetError) -> Self {
        Self {
            command: err.command.clone(),
            exit_code: err.exit_code,
            stderr: err.stderr.clone(),
        }
    }
}

/// SHELL-1: the views the window shows, in tab order.
#[tauri::command]
pub fn views() -> Vec<&'static str> {
    vec!["Sky", "Waiting on you", "Reports", "Budget", "Chat"]
}

/// CORE-5: the names of the settings not set, in the core's order.
#[tauri::command]
pub fn settings_missing(state: tauri::State<'_, AppState>) -> Result<Vec<&'static str>, FaceError> {
    settings::load(&state.settings_path)
        .map(|s| s.missing())
        .map_err(|e| FaceError::new("settings_missing", &e))
}

/// CORE-2: the fleet errors this session recorded, newest first.
#[tauri::command]
pub fn fleet_errors(state: tauri::State<'_, AppState>) -> Vec<FleetErrorRow> {
    let log = state.errors.lock().unwrap_or_else(|p| p.into_inner());
    log.errors.iter().map(FleetErrorRow::from).collect()
}

/// The fleet bridge over the pipeline repo the settings file names.
pub fn bridge(settings_path: &Path) -> Result<FleetBridge, CoreError> {
    let settings = settings::load(settings_path)?;
    Ok(FleetBridge::new(settings.pipeline_repo()?))
}

/// Record a fleet failure in this session's log; other errors are not fleet errors and are
/// left to the caller.
pub fn record_fleet_error(state: &AppState, err: &CoreError) {
    if let CoreError::Fleet(e) = err {
        let mut log = state.errors.lock().unwrap_or_else(|p| p.into_inner());
        log.record(e.clone());
    }
}

/// BUD-1: the budget rows as fleet last recorded them (no refresh).
#[tauri::command]
pub fn budget_rows(state: tauri::State<'_, AppState>) -> Result<Vec<PoolRow>, FaceError> {
    bridge(&state.settings_path)
        .and_then(|fleet| budget::read(&fleet))
        .map_err(|e| {
            record_fleet_error(&state, &e);
            FaceError::new("budget_rows", &e)
        })
}

/// BUD-2: save a pool edit to the pipeline clone, append its ledger row, put the Waiting on you
/// item in the local list, then return the re-read rows.
#[tauri::command]
pub fn budget_save_edit(
    edit: PoolEdit,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PoolRow>, FaceError> {
    bridge(&state.settings_path)
        .and_then(|fleet| {
            let log = state.data_dir.join("pool-edits.jsonl");
            budget::save_edit(&fleet, &edit, &log, &SystemClock)
        })
        .and_then(|saved| {
            let rows = saved.rows;
            LocalItems::new(&state.data_dir)
                .add(saved.waiting)
                .map(|()| rows)
        })
        .map_err(|e| {
            record_fleet_error(&state, &e);
            FaceError::new("budget_save_edit", &e)
        })
}

/// BUD-3: refresh every pool with a reader, then return the rows.
#[tauri::command]
pub fn budget_refresh(state: tauri::State<'_, AppState>) -> Result<Vec<PoolRow>, FaceError> {
    bridge(&state.settings_path)
        .and_then(|fleet| budget::refresh(&fleet))
        .map_err(|e| {
            record_fleet_error(&state, &e);
            FaceError::new("budget_refresh", &e)
        })
}
