//! The Tauri shell (SHELL-1). One window; the React face (`app/src/`) draws the Sky, Waiting
//! on you, Reports, Budget and Chat views. Every command here is a thin call into
//! `switchboard_core`; no rule lives in this crate (SPEC.md, Implementation decisions).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::views])
        .run(tauri::generate_context!())
        .expect("error while running Switchboard");
}
