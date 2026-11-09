//! The Tauri shell (SHELL-1). One window; the React face (`app/src/`) draws the Sky, Waiting
//! on you, Reports, Budget and Chat views. Every command here is a thin call into
//! `switchboard_core`; no rule lives in this crate (SPEC.md, Implementation decisions).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;
use switchboard_core::budget::{RefreshTimer, REFRESH_EVERY_SECS};
use switchboard_core::clock::SystemClock;
use switchboard_core::CoreError;
use tauri::{Emitter, Manager};

mod commands;

/// BUD-3: how often the refresh timer is checked. The timer itself decides what is due.
const TICK_EVERY: Duration = Duration::from_secs(60);

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            app.manage(commands::AppState::new(dir.join("settings.json")));
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::views,
            commands::settings_missing,
            commands::fleet_errors,
            commands::budget_rows,
            commands::budget_refresh,
            commands::budget_save_edit,
            commands::waiting,
            commands::sky,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Switchboard");
}
