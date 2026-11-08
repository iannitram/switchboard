//! Tauri commands: the GUI face's only door into the core. Each returns JSON the React face
//! renders, or the core's error as `{ "error": { "command", "lines" } }` (SHELL-3).

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use switchboard_core::fleet::{FleetError, FleetErrorLog};
use switchboard_core::settings::Settings;
use switchboard_core::{settings, CoreError};
use tauri::Manager;

/// What the shell holds between commands: the settings file's path and this session's fleet
/// error log.
pub struct AppState {
    pub settings_path: PathBuf,
    pub errors: Mutex<FleetErrorLog>,
}

impl AppState {
    pub fn new(settings_path: PathBuf) -> Self {
        Self {
            settings_path,
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
        Self::from_text(command, &err.to_string())
    }

    fn from_text(command: &str, text: &str) -> Self {
        Self {
            command: command.to_string(),
            lines: text.lines().take(5).map(str::to_string).collect(),
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
    vec![
        "Sky",
        "Waiting on you",
        "Reports",
        "Budget",
        "Chat",
        "Settings",
    ]
}

/// CORE-5: the names of the settings not set, in the core's order.
#[tauri::command]
pub fn settings_missing(state: tauri::State<'_, AppState>) -> Result<Vec<&'static str>, FaceError> {
    settings::load(&state.settings_path)
        .map(|s| s.missing())
        .map_err(|e| FaceError::new("settings_missing", &e))
}

/// CORE-5 as the Settings view reads it: the settings and the names of those not set.
#[derive(Debug, Serialize)]
pub struct SettingsView {
    pub settings: Settings,
    pub missing: Vec<&'static str>,
}

impl From<Settings> for SettingsView {
    fn from(settings: Settings) -> Self {
        let missing = settings.missing();
        Self { settings, missing }
    }
}

/// The settings file: `app_config_dir()/settings.json`.
fn settings_file(app: &tauri::AppHandle, command: &str) -> Result<PathBuf, FaceError> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join("settings.json"))
        .map_err(|e| FaceError::from_text(command, &e.to_string()))
}

/// CORE-5: the settings file's paths and which are not set.
#[tauri::command]
pub fn settings_get(app: tauri::AppHandle) -> Result<SettingsView, FaceError> {
    let path = settings_file(&app, "settings_get")?;
    settings::load(&path)
        .map(SettingsView::from)
        .map_err(|e| FaceError::new("settings_get", &e))
}

/// CORE-5: save the settings, then answer like `settings_get` from the saved file.
#[tauri::command]
pub fn settings_save(app: tauri::AppHandle, settings: Settings) -> Result<SettingsView, FaceError> {
    let path = settings_file(&app, "settings_save")?;
    settings::save(&path, &settings)
        .and_then(|()| settings::load(&path))
        .map(SettingsView::from)
        .map_err(|e| FaceError::new("settings_save", &e))
}

/// CORE-2: the fleet errors this session recorded, newest first.
#[tauri::command]
pub fn fleet_errors(state: tauri::State<'_, AppState>) -> Vec<FleetErrorRow> {
    let log = state.errors.lock().unwrap_or_else(|p| p.into_inner());
    log.errors.iter().map(FleetErrorRow::from).collect()
}
