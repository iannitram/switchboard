// #375 (CORE-2, IT-2): each fleet error the app records is also kept in the app data folder
// (`switchboard_core::fleet_errors`), and the app's list reads it back through the same core
// code the CLI's `errors` uses. A keep that fails leaves the action and the session log as
// they were. Every test uses its own temp folder; none sets a real app data folder.
use super::*;
use switchboard_core::fleet::FleetErrorReason;

/// A fresh folder for one test, under the system temp folder.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-errdisk-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fleet_error(n: u32) -> CoreError {
    CoreError::Fleet(FleetError {
        command: vec!["python3".into(), "fleet.py".into(), format!("call-{n}")],
        exit_code: Some(3),
        stderr: format!("boom {n}\n"),
        reason: FleetErrorReason::Exit,
        undo_note: None,
    })
}

/// An app state whose settings file is missing (so IT filing fails fast and says so) and
/// whose errors are kept in `data`, when given.
fn state(dir: &Path, data: Option<PathBuf>) -> AppState {
    let state = AppState::new(dir.join("settings.json"));
    if let Some(data) = data {
        state.keep_errors_in(data);
    }
    state
}

/// Record `err` as the app does and wait for its IT filing thread.
fn record(state: &AppState, err: &CoreError) {
    if let Some(filing) = record_fleet_error(state, err) {
        let _ = filing.join();
    }
}

fn commands(rows: &[FleetErrorRow]) -> Vec<String> {
    rows.iter().map(|r| r.command[2].clone()).collect()
}

#[test]
fn a_recorded_error_is_kept_on_disk_and_listed_newest_first() {
    let dir = scratch("kept");
    let data = dir.join("data");
    let app = state(&dir, Some(data.clone()));
    record(&app, &fleet_error(1));
    record(&app, &fleet_error(2));
    let text = std::fs::read_to_string(data.join("fleet-errors.jsonl")).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert_eq!(commands(&fleet_error_rows(&app)), ["call-2", "call-1"]);
}

#[test]
fn the_list_outlives_a_restart() {
    let dir = scratch("restart");
    let data = dir.join("data");
    record(&state(&dir, Some(data.clone())), &fleet_error(1));
    let restarted = state(&dir, Some(data));
    assert!(restarted.errors.lock().unwrap().errors.is_empty());
    let rows = fleet_error_rows(&restarted);
    assert_eq!(commands(&rows), ["call-1"]);
    assert_eq!(rows[0].exit_code, Some(3));
    assert_eq!(rows[0].stderr, "boom 1\n");
}

#[test]
fn the_app_list_is_what_the_core_load_reads() {
    let dir = scratch("agree");
    let data = dir.join("data");
    let app = state(&dir, Some(data.clone()));
    for n in 0..3 {
        record(&app, &fleet_error(n));
    }
    let core: Vec<String> = switchboard_core::fleet_errors::load(&data)
        .unwrap()
        .errors
        .iter()
        .map(|e| e.command[2].clone())
        .collect();
    assert_eq!(commands(&fleet_error_rows(&app)), core);
}

#[test]
fn a_damaged_line_is_skipped_not_a_panic() {
    let dir = scratch("damaged");
    let data = dir.join("data");
    let app = state(&dir, Some(data.clone()));
    record(&app, &fleet_error(1));
    let file = data.join("fleet-errors.jsonl");
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str("{not a record\n");
    std::fs::write(&file, text).unwrap();
    assert_eq!(commands(&fleet_error_rows(&app)), ["call-1"]);
}

#[test]
fn a_keep_that_fails_leaves_the_session_log_and_the_list_has_the_error() {
    let dir = scratch("unwritable");
    let data = dir.join("data");
    std::fs::write(&data, "a file where the data folder should be").unwrap();
    let app = state(&dir, Some(data));
    record(&app, &fleet_error(1));
    record(&app, &fleet_error(2));
    assert_eq!(app.errors.lock().unwrap().errors.len(), 2);
    assert_eq!(commands(&fleet_error_rows(&app)), ["call-2", "call-1"]);
}

#[test]
fn without_a_data_folder_nothing_is_written_and_the_session_log_is_listed_newest_first() {
    let dir = scratch("nofolder");
    let app = state(&dir, None);
    record(&app, &fleet_error(1));
    record(&app, &fleet_error(2));
    assert_eq!(commands(&fleet_error_rows(&app)), ["call-2", "call-1"]);
    let names: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert!(names.is_empty(), "wrote {names:?}");
}

#[test]
fn a_non_fleet_error_is_not_kept() {
    let dir = scratch("other");
    let data = dir.join("data");
    let app = state(&dir, Some(data.clone()));
    assert!(record_fleet_error(&app, &CoreError::Io("disk".into())).is_none());
    assert!(!data.join("fleet-errors.jsonl").exists());
    assert!(fleet_error_rows(&app).is_empty());
}
