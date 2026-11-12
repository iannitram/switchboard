//! The Tauri shell (SHELL-1). One window; the React face (`app/src/`) draws the Sky, Waiting
//! on you, Reports, Budget and Chat views. Every command here is a thin call into
//! `switchboard_core`; no rule lives in this crate (SPEC.md, Implementation decisions).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;
use switchboard_core::autopilot::weekly::local_utc_offset;
use switchboard_core::autopilot::{passes, run};
use switchboard_core::budget::{RefreshTimer, REFRESH_EVERY_SECS};
use switchboard_core::clock::{Clock, SystemClock};
use switchboard_core::github::live;
use switchboard_core::it::SystemProbeRunner;
use switchboard_core::platform::{current_os, loadout_probes};
use switchboard_core::CoreError;
use tauri::{Emitter, Manager};

mod commands;

/// BUD-3: how often the refresh timer is checked. The timer itself decides what is due.
const TICK_EVERY: Duration = Duration::from_secs(60);

/// AUTO-6: how often the satellite's lanes are checked. A loop that ended frees its lane.
const SATELLITE_EVERY: Duration = Duration::from_secs(60);

/// AUTO-6: the satellite thread. Each pass is one tick of the core's satellite session
/// (`run::SatelliteSession::tick`, the tick `switchboard autopilot satellite` runs): it reads
/// `satellite.json` (off when missing), records a fleet error for each loop that ended with an
/// error (CORE-2), classes the failed cards of loops that ended (AUTO-9), and starts the lanes
/// `driver::tick` allows. #432: the fleet settings are read again at each tick, so a changed
/// pipeline is used at once; the tick holds the satellite lock only while it runs, and the
/// loops and holds every satellite on this machine counts are on disk (`loops.json`). Being
/// held out by another run is said when it starts and when it ends (`run::SatelliteLog`).
fn satellite_loop(handle: tauri::AppHandle, config_dir: std::path::PathBuf) {
    let data = handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| config_dir.clone());
    let mut session: Option<run::SatelliteSession> = None;
    let mut log = run::SatelliteLog::default();
    loop {
        let state = handle.state::<commands::AppState>();
        // Settings not set yet start nothing, as before; the session keeps its last bridge.
        if let Ok(fleet) = commands::bridge(&state.settings_path) {
            match session.as_mut() {
                Some(session) => session.set_fleet(fleet),
                None => session = Some(run::SatelliteSession::new(fleet, &data)),
            }
        }
        if let Some(session) = session.as_mut() {
            let report = session.tick(&config_dir, Duration::ZERO);
            for e in &report.raised {
                commands::record_fleet_error(&state, e);
            }
            for line in log.lines(&report) {
                eprintln!("{line}");
            }
        }
        std::thread::sleep(SATELLITE_EVERY);
    }
}

/// A pass's report on stderr, as the window has always printed it, and its fleet errors
/// recorded (CORE-2).
fn print_report(state: &commands::AppState, report: &run::PassReport) {
    for e in &report.raised {
        commands::record_fleet_error(state, e);
    }
    for line in report.lines() {
        eprintln!("{line}");
    }
}

/// AUTO-3: the autopilot's passes after each poll (`run::run_poll_passes`, the passes
/// `switchboard autopilot <pass>` runs), holding the GitHub lock so no poll writes a snapshot
/// while they read. Jobs go to `<data>/autopilot/jobs/`; clones are found beside the pipeline
/// repo. A setting that is not set starts nothing; no machine name starts no review. #385: the
/// passes hold the autopilot lock; when a headless run holds it this round is skipped.
fn autopilot_after_poll(
    handle: &tauri::AppHandle,
    data: &std::path::Path,
    transport: &dyn switchboard_core::github::transport::GitHubTransport,
) {
    let state = handle.state::<commands::AppState>();
    let settings = match switchboard_core::settings::load(&state.settings_path) {
        Ok(settings) => settings,
        Err(e) => return eprintln!("autopilot: {e}"),
    };
    let pipeline_root = match settings.pipeline_repo() {
        Ok(repo) => repo.to_path_buf(),
        Err(_) => return,
    };
    let dev_root = pipeline_root
        .parent()
        .unwrap_or(&pipeline_root)
        .to_path_buf();
    let fleet = match commands::bridge(&state.settings_path) {
        Ok(fleet) => fleet,
        Err(e) => return eprintln!("autopilot: {e}"),
    };
    let home = handle.path().home_dir().unwrap_or_default();
    let env = std::env::var("FLEET_MACHINE").ok();
    let machine = passes::this_machine(env.as_deref(), &home);
    let mut starter = passes::FleetStarter {
        fleet,
        jobs_dir: data.join("autopilot").join("jobs"),
        dev_root: dev_root.clone(),
        machine: machine.clone(),
        qwen_probe: Box::new(passes::http_qwen_probe),
        retry_wait: passes::PM_RETRY_WAIT,
    };
    let env = run::RunEnv {
        data_dir: data,
        pipeline_root: &pipeline_root,
        dev_root: &dev_root,
        machine: machine.as_deref(),
        clock: &SystemClock,
        utc_offset_secs: local_utc_offset(),
        transport,
        dry_run: false,
        lock_wait: Duration::ZERO,
        // AUTO-5, #432 (ruling 8): the weekly backup runs inside the passes lock, so no other
        // run's passes start while it works. It holds this poll thread while it runs.
        backup: &|repos: &[String]| switchboard_core::backup::backup_all(&settings, repos),
    };
    let reports = {
        let _held = state.github.lock().unwrap_or_else(|p| p.into_inner());
        run::run_poll_passes(&env, &run::Pass::AFTER_POLL, &mut starter)
    };
    let reports = match reports {
        Ok(reports) => reports,
        Err(e) => return eprintln!("autopilot: {e}"),
    };
    for report in reports {
        print_report(&state, &report);
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            app.manage(commands::AppState::new(dir.join("settings.json")));
            // #375: recorded fleet errors are kept in the app data folder.
            if let Ok(data) = app.path().app_data_dir() {
                app.state::<commands::AppState>().keep_errors_in(data);
            }
            let satellite = app.handle().clone();
            std::thread::spawn(move || satellite_loop(satellite, dir));
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut timer = RefreshTimer::new(REFRESH_EVERY_SECS);
                loop {
                    let state = handle.state::<commands::AppState>();
                    if let Ok(fleet) = commands::bridge(&state.settings_path) {
                        match timer.tick(&fleet, &SystemClock) {
                            Some(Ok(rows)) => {
                                let _ = handle.emit("budget-rows", rows);
                            }
                            Some(Err(e)) => {
                                commands::record_fleet_error(&state, &e);
                                if !matches!(e, CoreError::Fleet(_)) {
                                    eprintln!("budget refresh: {e}");
                                }
                            }
                            None => {}
                        }
                    }
                    std::thread::sleep(TICK_EVERY);
                }
            });
            let poll = app.handle().clone();
            std::thread::spawn(move || {
                let mut transport = None;
                loop {
                    let state = poll.state::<commands::AppState>();
                    if transport.is_none() {
                        match commands::github_transport() {
                            Ok(t) => transport = Some(t),
                            Err(e) => eprintln!("github poll: {e}"),
                        }
                    }
                    if let (Some(t), Ok(data)) = (&transport, poll.path().app_data_dir()) {
                        let read = {
                            let _held = state.github.lock().unwrap_or_else(|p| p.into_inner());
                            live::tick(t, &data, &SystemClock)
                        };
                        let read = read.unwrap_or_else(|e| {
                            eprintln!("github poll: {e}");
                            Vec::new()
                        });
                        let _ = poll.emit(live::SNAPSHOT_EVENT, read);
                        autopilot_after_poll(&poll, &data, t);
                    }
                    std::thread::sleep(Duration::from_secs(live::TICK_EVERY_SECS));
                }
            });
            // IT-1: read the loadout now, then every minute, and push each read to the panel.
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                let state = handle.state::<commands::AppState>();
                let loadout = commands::read_and_store(
                    &state,
                    loadout_probes(current_os()),
                    &SystemProbeRunner,
                    SystemClock.now(),
                );
                let _ = handle.emit(commands::LOADOUT_EVENT, loadout);
                std::thread::sleep(Duration::from_secs(commands::LOADOUT_EVERY_SECS));
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::views,
            commands::settings_missing,
            commands::settings_get,
            commands::settings_save,
            commands::fleet_errors,
            commands::budget_rows,
            commands::budget_refresh,
            commands::budget_save_edit,
            commands::reports,
            commands::waiting,
            commands::sky,
            commands::chat_rate,
            commands::loadout,
            commands::chat_implicit,
            commands::refresh_all,
            commands::backup_repo,
            commands::backup_all,
            commands::chat_route,
            commands::chat_send_as_job,
            commands::confirm_guarded,
            commands::drop_guarded,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Switchboard");
}
