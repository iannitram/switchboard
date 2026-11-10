//! Tauri commands: the GUI face's only door into the core. Each returns JSON the React face
//! renders, or the core's error as `{ "error": { "command", "lines" } }` (SHELL-3).

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use switchboard_core::budget::{self, PoolEdit, PoolRow};
use switchboard_core::clock::{Clock, SystemClock, UnixSeconds};
use switchboard_core::fleet::{FleetBridge, FleetError, FleetErrorLog};
use switchboard_core::frontdoor::implicit_log::log_implicit;
use switchboard_core::it::{self, Loadout, ProbeRunner};
use switchboard_core::platform::{current_os, loadout_probes, Probe};
use switchboard_core::reports::{self, thread_rows, Filter, Report, FLEET_STATE_DIR_ENV};
use switchboard_core::settings::{self, Settings};
use switchboard_core::CoreError;
use tauri::Manager;

/// What the shell holds between commands: the settings file's path, this session's fleet
/// error log, and the newest loadout read.
pub struct AppState {
    pub settings_path: PathBuf,
    pub errors: Mutex<FleetErrorLog>,
    /// GH-1: held while the poll thread or the Refresh button reads GitHub, so two never write
    /// the snapshot store at once.
    pub github: Mutex<()>,
    /// IT-1: the newest loadout read, for the panel and the router (FD-12); None before the first read.
    pub loadout: Mutex<Option<Loadout>>,
}

impl AppState {
    pub fn new(settings_path: PathBuf) -> Self {
        Self {
            settings_path,
            errors: Mutex::new(FleetErrorLog::default()),
            github: Mutex::new(()),
            loadout: Mutex::new(None),
        }
    }
}

/// IT-1: the event the minute read pushes its loadout on.
pub const LOADOUT_EVENT: &str = "loadout";
/// IT-1: how often the core reads the loadout while the app is open.
pub const LOADOUT_EVERY_SECS: u64 = 60;

/// IT-1: keep `loadout` as the stored one unless the stored one was read later; answer what is
/// stored. A read at the same second replaces the stored one.
pub fn store_loadout(state: &AppState, loadout: Loadout) -> Loadout {
    let mut kept = state.loadout.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(newest) = kept.as_ref().filter(|k| k.read_at > loadout.read_at) {
        return newest.clone();
    }
    *kept = Some(loadout.clone());
    loadout
}

/// IT-1: the newest loadout read so far, None before the first read.
pub fn latest_loadout(state: &AppState) -> Option<Loadout> {
    state
        .loadout
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

/// IT-1: read the loadout now through `probes` and store it; answer the stored loadout.
pub fn read_and_store(
    state: &AppState,
    probes: &[Probe],
    runner: &dyn ProbeRunner,
    now: UnixSeconds,
) -> Loadout {
    store_loadout(state, it::read_loadout(probes, runner, now))
}

/// IT-1: the stored loadout, with no probe run; the first read only when none is stored yet.
pub fn current_loadout(
    state: &AppState,
    probes: &[Probe],
    runner: &dyn ProbeRunner,
    now: UnixSeconds,
) -> Loadout {
    latest_loadout(state).unwrap_or_else(|| read_and_store(state, probes, runner, now))
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

/// SKY-W: the sky the Sky view paints (`sky_at` in the app data folder, now). A ledger or
/// settings file that does not read is named on stderr; the sky shows without jobs.
#[tauri::command]
pub fn sky(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<switchboard_core::sky::model::SkyModel, FaceError> {
    let env = std::env::var_os(FLEET_STATE_DIR_ENV);
    app.path()
        .app_data_dir()
        .map_err(|e| CoreError::Io(format!("app data dir: {e}")))
        .and_then(|data| {
            sky_at(
                &state.settings_path,
                env.as_deref(),
                &data,
                SystemClock.now(),
            )
        })
        .map(|(model, jobs_error)| {
            if let Some(e) = jobs_error {
                eprintln!("sky: the jobs in flight could not be read, so none are shown: {e}");
            }
            model
        })
        .map_err(|e| FaceError::new("sky", &e))
}

/// SKY-W, SKY-4 (#343): the sky from the snapshot in `data`, with the jobs in flight from the
/// fleet ledger (`ledger_events`). A ledger or settings file that does not read leaves the sky
/// without jobs, and its error comes back beside the sky; only the sky's own read fails it.
pub fn sky_at(
    settings_path: &Path,
    env: Option<&std::ffi::OsStr>,
    data: &Path,
    now: UnixSeconds,
) -> Result<(switchboard_core::sky::model::SkyModel, Option<CoreError>), CoreError> {
    let (events, jobs_error) = match ledger_events(settings_path, env) {
        Ok(events) => (events, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let model = switchboard_core::sky::model::load_with_ledger(data, &events, now)?;
    Ok((model, jobs_error))
}

/// SKY-4 (#343): the fleet ledger's events, from the fleet state `reports` reads; none while
/// the settings name no pipeline repo, as the CLI has none without its pipeline checkout.
pub fn ledger_events(
    settings_path: &Path,
    env: Option<&std::ffi::OsStr>,
) -> Result<Vec<serde_json::Value>, CoreError> {
    let settings = settings::load(settings_path)?;
    let Ok(root) = settings.pipeline_repo() else {
        return Ok(Vec::new());
    };
    let state = reports::state_dir(root, env);
    switchboard_core::joblog::read_ledger(&switchboard_core::joblog::ledger_files(root, &state))
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

/// IT-W / IT-1: the stored loadout (the Loadout panel). The core's minute read keeps it; only
/// before that first read does this run the probes. It runs off the main thread.
#[tauri::command(async)]
pub fn loadout(state: tauri::State<'_, AppState>) -> Result<Loadout, FaceError> {
    Ok(current_loadout(
        &state,
        loadout_probes(current_os()),
        &it::SystemProbeRunner,
        SystemClock.now(),
    ))
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
            save_pool_edit_rows(&fleet, &edit, &data, &SystemClock)
        })
        .map_err(|e| {
            record_fleet_error(&state, &e);
            FaceError::new("budget_save_edit", &e)
        })
}

/// BUD-2 (#340): the Budget panel's save through `budget::save_pool_edit`, the same as the CLI
/// and MCP: file, ledger row and Waiting item all or nothing; the rows read again.
pub fn save_pool_edit_rows(
    fleet: &FleetBridge,
    edit: &PoolEdit,
    data: &Path,
    clock: &dyn Clock,
) -> Result<Vec<PoolRow>, CoreError> {
    budget::save_pool_edit(fleet, edit, data, clock).map(|saved| saved.rows)
}

#[cfg(test)]
#[path = "pool_edit_tests.rs"]
mod pool_edit_tests;

/// REP-1, REP-3: the reports, newest first, narrowed by `filter` when the face sends one.
/// The chat threads join from the thread store in the app data folder (REP follow-up, #286).
#[tauri::command]
pub fn reports(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    filter: Option<Filter>,
) -> Result<Vec<Report>, FaceError> {
    bridge(&state.settings_path)
        .and_then(|fleet| {
            let env = std::env::var_os(FLEET_STATE_DIR_ENV);
            let data = app
                .path()
                .app_data_dir()
                .map_err(|e| CoreError::Io(format!("app data dir: {e}")))?;
            let threads = thread_rows(&data);
            reports::load(&fleet, env.as_deref(), &threads, filter.as_ref())
        })
        .map_err(|e| {
            record_fleet_error(&state, &e);
            FaceError::new("reports", &e)
        })
}

#[cfg(test)]
#[path = "reports_threads_tests.rs"]
mod reports_threads_tests;

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

/// FD-12: send a held Chat request as a fleet job another machine pulls (the Chat view's
/// "Send as a fleet job"). This machine's name comes from `FLEET_MACHINE` or the machine file.
#[tauri::command]
pub fn chat_send_as_job(
    state: tauri::State<'_, AppState>,
    kind: String,
    request: String,
    unmet: Vec<String>,
) -> Result<serde_json::Value, FaceError> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let env_name = std::env::var("FLEET_MACHINE").ok();
    let machine = switchboard_core::it::this_machine(env_name.as_deref(), &home);
    let machine = machine.unwrap_or_default();
    bridge(&state.settings_path)
        .and_then(|fleet| {
            switchboard_core::it::send_held_job(&fleet, &kind, &request, &unmet, &machine)
        })
        .map_err(|e| {
            record_fleet_error(&state, &e);
            FaceError::new("chat_send_as_job", &e)
        })
}

/// CORE-3, BAK-3 (#347): confirm a delete or restore request that waits in Waiting on you.
/// `confirmed` is true only when I pressed the confirm button on that request; the argument
/// has no default, and without it the guard refuses. `cwd` is the folder the app runs in (a
/// job worktree refuses everything). A refusal changes nothing.
pub fn confirm_guarded_at(
    settings_path: &Path,
    data: &Path,
    text: &str,
    confirmed: bool,
    cwd: &Path,
) -> Result<switchboard_core::confirm::Done, CoreError> {
    use switchboard_core::rules::{self, Caller, Face};
    let caller = Caller {
        face: Face::Gui,
        role: None,
        in_job_worktree: rules::is_job_worktree(cwd),
        confirmed_in_app: confirmed,
    };
    let settings = settings::load(settings_path)?;
    switchboard_core::confirm::confirm(&settings, data, text, &caller)
}

/// CORE-3, BAK-3 (#347): the Waiting on you view's confirm on a guarded request. It runs off
/// the main thread: a restore pushes to GitHub.
#[tauri::command(async)]
pub fn confirm_guarded(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    text: String,
    confirmed: bool,
) -> Result<switchboard_core::confirm::Done, FaceError> {
    app.path()
        .app_data_dir()
        .map_err(|e| CoreError::Io(format!("app data dir: {e}")))
        .and_then(|data| {
            let cwd = std::env::current_dir()
                .map_err(|e| CoreError::Io(format!("working folder: {e}")))?;
            confirm_guarded_at(&state.settings_path, &data, &text, confirmed, &cwd)
        })
        .map_err(|e| FaceError::new("confirm_guarded", &e))
}

#[cfg(test)]
#[path = "confirm_guarded_tests.rs"]
mod confirm_guarded_tests;

#[cfg(test)]
#[path = "it_tickets_tests.rs"]
mod it_tickets_tests;

#[cfg(test)]
#[path = "loadout_tests.rs"]
mod loadout_tests;
#[cfg(test)]
#[path = "sky_ledger_tests.rs"]
mod sky_ledger_tests;
#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;

/// FD-1, FD-11: route one Chat message (the composer's Send, #177), then answer it: a service
/// pick is sent through its adapter and both turns are stored in the thread (`handoff_id` names
/// an open thread; none starts one). A Claude handoff is not answered. The reply is the route
/// (`step`, `kind`, `pick`) with `text`, `service`, `model`, `handoff` and `handoff_id` when
/// answered. It runs off the main thread: it calls a model.
#[tauri::command(async)]
pub fn chat_route(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    message: String,
    handoff_id: Option<String>,
) -> Result<switchboard_core::frontdoor::answer::ChatReply, FaceError> {
    use switchboard_core::frontdoor::{answer, chat};
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| CoreError::Io(format!("app data dir: {e}")));
    bridge(&state.settings_path)
        .and_then(|fleet| {
            let routed =
                chat::route_message(&fleet, &message, chat::estimate_complexity(&message))?;
            let machine = switchboard_core::abilities::FleetModels::local_machine();
            let qwen = answer::qwen_base_url(&fleet, machine.as_deref());
            let env = |name: &str| std::env::var(name).ok();
            let transport = || -> Box<dyn switchboard_core::frontdoor::adapters::ChatTransport> {
                Box::new(switchboard_core::frontdoor::adapters::HttpChatTransport::new())
            };
            let data = data?;
            let ctx = answer::AnswerContext {
                data_dir: &data,
                qwen_base_url: qwen.as_deref(),
                env: &env,
                transport: &transport,
            };
            answer::answer_route(routed, &message, handoff_id.as_deref(), &ctx)
        })
        .map_err(|e| {
            record_fleet_error(&state, &e);
            FaceError::new("chat_route", &e)
        })
}
