//! Tauri commands: the GUI face's only door into the core. Each returns JSON the React face
//! renders, or the core's error as `{ "error": { "command", "lines" } }` (SHELL-3).

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use switchboard_core::fleet::{FleetError, FleetErrorLog};
use switchboard_core::{settings, CoreError};

/// What the commands share: the settings file's path and this session's fleet error log.
pub struct AppState {
    pub settings_path: PathBuf,
    pub errors: Mutex<FleetErrorLog>,
}

impl AppState {
    pub fn new(settings_path: PathBuf) -> Self {
        AppState {
            settings_path,
            errors: Mutex::new(FleetErrorLog::default()),
        }
    }
}

/// SHELL-3: a core error as the face shows it, the command that failed and the first lines.
#[derive(Debug, Serialize)]
pub struct FaceError {
    pub command: String,
    pub lines: Vec<String>,
}

impl FaceError {
    fn from_core(command: &str, err: &CoreError) -> Self {
        FaceError {
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

/// CORE-2 as the face reads it: one fleet error, without its `reason`.
#[derive(Debug, Serialize)]
pub struct FleetErrorRow {
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub stderr: String,
}

impl From<&FleetError> for FleetErrorRow {
    fn from(err: &FleetError) -> Self {
        FleetErrorRow {
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
        .map_err(|e| FaceError::from_core("settings_missing", &e))
}

/// CORE-2: the fleet errors this session recorded, newest first (the log's order).
#[tauri::command]
pub fn fleet_errors(state: tauri::State<'_, AppState>) -> Vec<FleetErrorRow> {
    state
        .errors
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .errors
        .iter()
        .map(FleetErrorRow::from)
        .collect()
}
