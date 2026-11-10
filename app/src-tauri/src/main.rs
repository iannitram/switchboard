//! The Tauri shell (SHELL-1). One window; the React face (`app/src/`) draws the Sky, Waiting
//! on you, Reports, Budget and Chat views. Every command here is a thin call into
//! `switchboard_core`; no rule lives in this crate (SPEC.md, Implementation decisions).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;
use switchboard_core::autopilot::{driver, may_spend, passes};
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

/// AUTO-6: the satellite thread. Each pass reads `satellite.json` (off when missing), starts
/// the lanes `driver::tick` allows, and classes the failed cards of loops that ended (AUTO-9).
fn satellite_loop(handle: tauri::AppHandle, config_dir: std::path::PathBuf) {
    let data = handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| config_dir.clone());
    let mut runner: Option<driver::ChildRunner> = None;
    loop {
        let state = handle.state::<commands::AppState>();
        match driver::load_config(&config_dir.join(driver::SATELLITE_FILE)) {
            Ok(config) => {
                if let Ok(fleet) = commands::bridge(&state.settings_path) {
                    let runner = runner.get_or_insert_with(|| {
                        driver::ChildRunner::new(fleet.clone(), &data.join("satellite"))
                    });
                    for (lane, stdout) in runner.take_finished() {
                        let dir = data.join("satellite").join("cards").join(&lane);
                        match driver::classify_failed(&fleet, &dir, &stdout) {
                            Ok(classes) => {
                                for (id, class) in classes {
                                    eprintln!("satellite {lane}: {id} {class:?}");
                                }
                            }
                            Err(e) => {
                                let _ = commands::record_fleet_error(&state, &e);
                            }
                        }
                    }
                    let spend = |lane: &str| may_spend(&fleet, lane);
                    if let Err(e) = driver::tick(&config, runner, &spend) {
                        commands::record_fleet_error(&state, &e);
                        eprintln!("satellite: {e}");
                    }
                }
            }
            Err(e) => eprintln!("satellite: {e}"),
        }
        std::thread::sleep(SATELLITE_EVERY);
    }
}

/// AUTO-3: the autopilot's passes after each poll (`autopilot::after_poll`), holding the
/// GitHub lock so no poll writes a snapshot while they read. Jobs go to
/// `<data>/autopilot/jobs/`; clones are found beside the pipeline repo. A setting that is not
/// set starts nothing; no machine name starts no review.
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
        dev_root,
        machine: machine.clone(),
        qwen_probe: Box::new(passes::http_qwen_probe),
    };
    let result = {
        let _held = state.github.lock().unwrap_or_else(|p| p.into_inner());
        let started =
            switchboard_core::autopilot::after_poll(&mut switchboard_core::autopilot::AfterPoll {
                data_dir: data,
                machine: machine.as_deref(),
                starter: &mut starter,
            });
        // AUTO-4: then merge what passed, each merge followed by a read of its repo (GH-3).
        let store = switchboard_core::github::snapshot::SnapshotStore::new(data);
        let poller = switchboard_core::github::poll::Poller {
            transport,
            store: &store,
            clock: &SystemClock,
            repos: Vec::new(),
        };
        let merged = store.load_all().and_then(|snaps| {
            passes::merge_pass(&snaps, &poller, &pipeline_root, &starter.dev_root)
        });
        match merged {
            Ok(merged) => {
                for (repo, prs, issues) in merged {
                    eprintln!("autopilot: merged {repo} {prs:?}, closing {issues:?}");
                }
            }
            Err(e) => eprintln!("autopilot: merge: {e}"),
        }
        // AUTO-5: the weekly drift clear and backup, once Sunday 6am local has passed.
        if let Some(offset) = local_utc_offset() {
            match passes::weekly_run(data, SystemClock.now(), offset, &mut starter) {
                Ok(run) => {
                    for id in &run.drift_started {
                        eprintln!("autopilot: started {id}");
                    }
                    if let Some(e) = &run.drift_error {
                        eprintln!("autopilot: drift clear: {e}");
                    }
                    if !run.backup.is_empty() {
                        let settings = settings.clone();
                        std::thread::spawn(move || {
                            match switchboard_core::backup::backup_all(&settings, &run.backup) {
                                Ok(outcomes) => {
                                    use switchboard_core::backup::BackupOutcome;
                                    for (repo, outcome) in outcomes {
                                        match outcome {
                                            Ok(BackupOutcome::Written(_)) => {}
                                            Ok(BackupOutcome::Refused(why)) => {
                                                eprintln!("weekly backup: {repo} refused: {why}");
                                            }
                                            Err(e) => eprintln!("weekly backup: {e}"),
                                        }
                                    }
                                }
                                Err(e) => eprintln!("weekly backup: {e}"),
                            }
                        });
                    }
                }
                Err(e) => eprintln!("autopilot: weekly: {e}"),
            }
        }
        started
    };
    match result {
        Ok(ids) => {
            for id in ids {
                eprintln!("autopilot: started {id}");
            }
        }
        Err(e) => {
            commands::record_fleet_error(&state, &e);
            eprintln!("autopilot: {e}");
        }
    }
}

/// AUTO-5: this computer's UTC offset in seconds: `date +%z` on Linux and macOS, PowerShell's
/// `zzz` on Windows. `None` when neither answers; the weekly run then waits.
fn local_utc_offset() -> Option<i64> {
    let out = if cfg!(windows) {
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-Date).ToString('zzz').Replace(':','')",
            ])
            .output()
    } else {
        std::process::Command::new("date").arg("+%z").output()
    }
    .ok()?;
    switchboard_core::autopilot::weekly::parse_utc_offset(&String::from_utf8_lossy(&out.stdout))
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            app.manage(commands::AppState::new(dir.join("settings.json")));
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Switchboard");
}
