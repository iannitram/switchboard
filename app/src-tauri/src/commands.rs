//! Tauri commands: the GUI face's only door into the core. Each returns JSON the React face
//! renders, or the core's error as `{ "error": { "command", "lines" } }` (SHELL-3).

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use switchboard_core::budget::{self, PoolEdit, PoolRow};
use switchboard_core::clock::{Clock, SystemClock};
use switchboard_core::fleet::{FleetBridge, FleetError, FleetErrorLog};
use switchboard_core::frontdoor::implicit_log::log_implicit;
use switchboard_core::settings::{self, Settings};
use switchboard_core::waiting::LocalItems;
use switchboard_core::CoreError;
use tauri::Manager;

/// What the shell holds between commands: the settings file's path and this session's fleet
/// error log.
pub struct AppState {
    pub settings_path: PathBuf,
    pub errors: Mutex<FleetErrorLog>,
    /// GH-1: held while the poll thread or the Refresh button reads GitHub, so two never write
    /// the snapshot store at once.
    pub github: Mutex<()>,
}

impl AppState {
    pub fn new(settings_path: PathBuf) -> Self {
        Self {
            settings_path,
            errors: Mutex::new(FleetErrorLog::default()),
            github: Mutex::new(()),
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

/// CORE-5 as the Settings view reads it: the three paths and the names of those not set.
#[derive(Debug, Serialize)]
pub struct SettingsView {
    pub settings: Settings,
    pub missing: Vec<&'static str>,
}

/// The settings file at `path` and the names not set. A missing file is every path unset.
pub fn settings_view(path: &Path) -> Result<SettingsView, CoreError> {
    let settings = settings::load(path)?;
    let missing = settings.missing();
    Ok(SettingsView { settings, missing })
}

/// Write `settings` to `path`, then read it back, so the answer is what is on disk.
pub fn save_settings(path: &Path, settings: &Settings) -> Result<SettingsView, CoreError> {
    settings::save(path, settings)?;
    settings_view(path)
}

/// CORE-5: the settings file and the names not set, for the Settings view.
#[tauri::command]
pub fn settings_get(state: tauri::State<'_, AppState>) -> Result<SettingsView, FaceError> {
    settings_view(&state.settings_path).map_err(|e| FaceError::new("settings_get", &e))
}

/// CORE-5: save the Settings view's paths and answer as `settings_get` does.
#[tauri::command]
pub fn settings_save(
    state: tauri::State<'_, AppState>,
    settings: Settings,
) -> Result<SettingsView, FaceError> {
    save_settings(&state.settings_path, &settings).map_err(|e| FaceError::new("settings_save", &e))
}

/// CORE-2: the fleet errors this session recorded, newest first.
#[tauri::command]
pub fn fleet_errors(state: tauri::State<'_, AppState>) -> Vec<FleetErrorRow> {
    let log = state.errors.lock().unwrap_or_else(|p| p.into_inner());
    log.errors.iter().map(FleetErrorRow::from).collect()
}

/// SKY-W: the sky the Sky view paints, from the snapshot in the app data folder.
#[tauri::command]
pub fn sky(app: tauri::AppHandle) -> Result<switchboard_core::sky::model::SkyModel, FaceError> {
    app.path()
        .app_data_dir()
        .map_err(|e| CoreError::Io(format!("app data dir: {e}")))
        .and_then(|data| switchboard_core::sky::model::load(&data, SystemClock.now()))
        .map_err(|e| FaceError::new("sky", &e))
}

/// The fleet bridge over the pipeline repo the settings file names.
pub fn bridge(settings_path: &Path) -> Result<FleetBridge, CoreError> {
    let settings = settings::load(settings_path)?;
    Ok(FleetBridge::new(settings.pipeline_repo()?))
}

/// IT-2: one filing at a time, so two errors close together never hand fleet the log at once.
static FILING: Mutex<()> = Mutex::new(());

/// IT-2: hand `log` to `fleet errors file` through the pipeline repo the settings name; the count handed over.
pub fn file_tickets(settings_path: &Path, log: &FleetErrorLog) -> Result<usize, CoreError> {
    let fleet = bridge(settings_path)?;
    let _held = FILING.lock().unwrap_or_else(|p| p.into_inner());
    switchboard_core::it::file_error_tickets(&fleet, log).map_err(CoreError::Fleet)
}

/// Record a fleet failure in this session's log; other errors are not fleet errors and are
/// left to the caller. IT-2: then file the whole log on its own thread. None for any other error.
pub fn record_fleet_error(
    state: &AppState,
    err: &CoreError,
) -> Option<std::thread::JoinHandle<Result<usize, CoreError>>> {
    let CoreError::Fleet(e) = err else {
        return None;
    };
    let copy = {
        let mut log = state.errors.lock().unwrap_or_else(|p| p.into_inner());
        log.record(e.clone());
        FleetErrorLog {
            errors: log.errors.clone(),
        }
    };
    let settings_path = state.settings_path.clone();
    Some(std::thread::spawn(move || {
        let filed = file_tickets(&settings_path, &copy);
        if let Err(e) = &filed {
            eprintln!("IT ticket not filed: {e}");
        }
        filed
    }))
}

/// FD-9: log a thumbs press on one answer (#179) as that turn's rating row in the chat's
/// thread, in the thread store in the app data folder.
#[tauri::command]
pub fn chat_rate(
    app: tauri::AppHandle,
    handoff_id: String,
    turn_index: u32,
    rating: String,
    service: String,
    model: String,
    kind: Option<String>,
) -> Result<(), FaceError> {
    app.path()
        .app_data_dir()
        .map_err(|e| CoreError::Io(format!("app data dir: {e}")))
        .and_then(|data| {
            switchboard_core::frontdoor::rating_log::log_rating(
                &data,
                &handoff_id,
                turn_index,
                &rating,
                &service,
                &model,
                kind.as_deref(),
            )
        })
        .map(|_| ())
        .map_err(|e| FaceError::new("chat_rate", &e))
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

/// IT-W: this machine's loadout, read now through this OS's probe list (the Loadout panel).
/// It runs off the main thread: the probes start ten programs.
#[tauri::command(async)]
pub fn loadout() -> switchboard_core::it::Loadout {
    switchboard_core::it::read_this_machine(
        &switchboard_core::it::SystemProbeRunner,
        switchboard_core::clock::Clock::now(&SystemClock),
    )
}

/// WAIT-W: the Waiting on you items, newest first: the snapshot's and the local ones.
#[tauri::command]
pub fn waiting(app: tauri::AppHandle) -> Result<Vec<switchboard_core::waiting::Item>, FaceError> {
    app.path()
        .app_data_dir()
        .map_err(|e| CoreError::Io(format!("app data dir: {e}")))
        .and_then(|data| switchboard_core::waiting::load(&data))
        .map_err(|e| FaceError::new("waiting", &e))
}

/// BUD-2: save a pool edit to the pipeline clone's config.json, log it in
/// `<app data dir>/pool-edits.jsonl`, put its Waiting on you item with the local items, and
/// return the rows read again.
#[tauri::command]
pub fn budget_save_edit(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    edit: PoolEdit,
) -> Result<Vec<PoolRow>, FaceError> {
    app.path()
        .app_data_dir()
        .map_err(|e| CoreError::Io(format!("app data dir: {e}")))
        .and_then(|data| {
            let fleet = bridge(&state.settings_path)?;
            let saved =
                budget::save_edit(&fleet, &edit, &data.join("pool-edits.jsonl"), &SystemClock)?;
            LocalItems::new(&data).add(saved.waiting)?;
            Ok(saved.rows)
        })
        .map_err(|e| {
            record_fleet_error(&state, &e);
            FaceError::new("budget_save_edit", &e)
        })
}

/// FD-9: log an implicit chat signal (FD-8's press sends `opened-in-claude`) as its own row in
/// the chat's thread, in the thread store in the app data folder.
#[tauri::command]
pub fn chat_implicit(
    app: tauri::AppHandle,
    handoff_id: String,
    signal: String,
    service: Option<String>,
    model: Option<String>,
    kind: Option<String>,
) -> Result<(), FaceError> {
    app.path()
        .app_data_dir()
        .map_err(|e| CoreError::Io(format!("app data dir: {e}")))
        .and_then(|data| {
            log_implicit(
                &data,
                &handoff_id,
                &signal,
                service.as_deref(),
                model.as_deref(),
                kind.as_deref(),
            )
        })
        .map(|_| ())
        .map_err(|e| FaceError::new("chat_implicit", &e))
}

/// GH-1: the GitHub transport, with the token `gh auth token` prints.
pub fn github_transport() -> Result<switchboard_core::github::transport::HttpTransport, CoreError> {
    switchboard_core::github::transport::HttpTransport::new(
        &switchboard_core::platform::GhAuthToken::default(),
    )
}

/// GH-1: the Refresh button: read every repo now, whatever its age. Answers the repos read.
/// It runs off the main thread: it reads GitHub.
#[tauri::command(async)]
pub fn refresh_all(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, FaceError> {
    let _held = state.github.lock().unwrap_or_else(|p| p.into_inner());
    app.path()
        .app_data_dir()
        .map_err(|e| CoreError::Io(format!("app data dir: {e}")))
        .and_then(|data| {
            let transport = github_transport()?;
            switchboard_core::github::live::refresh_all(&transport, &data, &SystemClock)
        })
        .map_err(|e| FaceError::new("refresh_all", &e))
}

#[cfg(test)]
#[path = "it_tickets_tests.rs"]
mod it_tickets_tests;

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;

/// BAK-1, BAK-2: "back up" on one constellation; a refused backup becomes its Waiting on you
/// item.
#[tauri::command]
pub fn backup_repo(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    repo: String,
) -> Result<switchboard_core::backup_run::BackupRow, FaceError> {
    app.path()
        .app_data_dir()
        .map_err(|e| CoreError::Io(format!("app data dir: {e}")))
        .and_then(|data| {
            let settings = settings::load(&state.settings_path)?;
            let now = switchboard_core::clock::Clock::now(&SystemClock);
            switchboard_core::backup_run::run_one(&settings, &data, &repo, now)
        })
        .map_err(|e| FaceError::new("backup_repo", &e))
}

/// BAK-1, BAK-2: "back up all", every repo in the sky.
#[tauri::command]
pub fn backup_all(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<switchboard_core::backup_run::BackupRow>, FaceError> {
    app.path()
        .app_data_dir()
        .map_err(|e| CoreError::Io(format!("app data dir: {e}")))
        .and_then(|data| {
            let settings = settings::load(&state.settings_path)?;
            let now = switchboard_core::clock::Clock::now(&SystemClock);
            switchboard_core::backup_run::run_all(&settings, &data, now)
        })
        .map_err(|e| FaceError::new("backup_all", &e))
}
