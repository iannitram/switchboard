//! Locked tests for issue 3 (CORE-2: the fleet error log the banner and IT read). Qwen never
//! edits this file.

use switchboard_core::fleet::{FleetError, FleetErrorLog, FleetErrorReason, FLEET_ERROR_LOG_CAP};

fn err(code: i32, stderr: &str) -> FleetError {
    FleetError {
        command: vec![
            "python3".into(),
            "-I".into(),
            "/srv/pipeline/tools/fleet/fleet.py".into(),
            "budget".into(),
            "--json".into(),
        ],
        exit_code: Some(code),
        stderr: stderr.into(),
        reason: FleetErrorReason::Exit,
        undo_note: None,
    }
}

#[test]
fn a_new_log_is_empty_and_has_no_latest() {
    let log = FleetErrorLog::default();
    assert!(log.errors.is_empty());
    assert_eq!(log.latest(), None);
}

#[test]
fn record_keeps_the_error_exactly_with_all_of_its_stderr() {
    let mut log = FleetErrorLog::default();
    let e = err(
        3,
        "line one\nline two\nline three\nline four\nline five\nline six\n",
    );
    log.record(e.clone());
    assert_eq!(log.errors, vec![e]);
}

#[test]
fn errors_are_kept_newest_last_and_latest_is_the_last_recorded() {
    let mut log = FleetErrorLog::default();
    log.record(err(1, "first\n"));
    log.record(err(2, "second\n"));
    assert_eq!(log.errors, vec![err(1, "first\n"), err(2, "second\n")]);
    assert_eq!(log.latest(), Some(&err(2, "second\n")));
}

#[test]
fn the_same_error_twice_is_kept_twice() {
    let mut log = FleetErrorLog::default();
    log.record(err(3, "same\n"));
    log.record(err(3, "same\n"));
    assert_eq!(log.errors.len(), 2);
}

#[test]
fn the_log_keeps_at_most_50_and_drops_the_oldest() {
    assert_eq!(FLEET_ERROR_LOG_CAP, 50);
    let mut log = FleetErrorLog::default();
    for code in 1..=55 {
        log.record(err(code, "x\n"));
    }
    assert_eq!(log.errors.len(), 50);
    assert_eq!(log.errors[0].exit_code, Some(6));
    assert_eq!(log.errors[49].exit_code, Some(55));
    assert_eq!(log.latest().unwrap().exit_code, Some(55));
}

#[test]
fn the_code_map_shows_core_fleet_present() {
    let map = include_str!("../../../docs/code-map.md");
    let line = map
        .lines()
        .skip_while(|l| *l != "### core-fleet")
        .nth(1)
        .unwrap();
    assert!(line.contains("· present."), "{line}");
}
