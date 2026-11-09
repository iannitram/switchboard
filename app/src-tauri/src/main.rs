//! The Tauri shell (SHELL-1). One window; the React face (`app/src/`) draws the Sky, Waiting
//! on you, Reports, Budget and Chat views. Every command here is a thin call into
//! `switchboard_core`; no rule lives in this crate (SPEC.md, Implementation decisions).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;
use switchboard_core::autopilot::{driver, may_spend};
use switchboard_core::budget::{RefreshTimer, REFRESH_EVERY_SECS};
use switchboard_core::clock::SystemClock;
use switchboard_core::github::live;
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
    let data = handle.path().app_data_dir().unwrap_or_else(|_| config_dir.clone());
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
                            Err(e) => commands::record_fleet_error(&state, &e),
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
                    }
                    std::thread::sleep(Duration::from_secs(live::TICK_EVERY_SECS));
                }
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
            commands::waiting,
            commands::sky,
            commands::chat_rate,
            commands::loadout,
            commands::chat_implicit,
            commands::refresh_all,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Switchboard");
}
