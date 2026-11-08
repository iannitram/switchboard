//! BUD-3 (#27): refresh the budget on a timer from the injected clock. Locked tests (R28): the
//! builder never edits this file.

use serde_json::Value;
use std::path::{Path, PathBuf};
use switchboard_core::budget::{refresh, rows_from_json, RefreshTimer, REFRESH_EVERY_SECS};
use switchboard_core::clock::FixedClock;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::CoreError;

const T0: i64 = 1_794_146_400;

/// A stub fleet.py that logs its arguments to calls.log, exits 3 while a file named `fail`
/// exists, and otherwise prints the budget fixture.
const STUB: &str = r#"import pathlib, sys
root = pathlib.Path(__file__).resolve().parent.parent.parent
with open(root / "calls.log", "a", encoding="utf-8") as fh:
    fh.write(" ".join(sys.argv[1:]) + "\n")
if (root / "fail").exists():
    sys.stderr.write("reader failed\n")
    sys.exit(3)
sys.stdout.write((root / "answers" / "budget.json").read_text(encoding="utf-8"))
"#;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_rows() -> Vec<switchboard_core::budget::PoolRow> {
    let p = manifest().join("tests/fixtures/fleet-stub/answers/budget.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    rows_from_json(&v).unwrap()
}

fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sb-bud3-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tools").join("fleet")).unwrap();
    std::fs::create_dir_all(root.join("answers")).unwrap();
    std::fs::write(root.join("tools").join("fleet").join("fleet.py"), STUB).unwrap();
    std::fs::copy(
        manifest().join("tests/fixtures/fleet-stub/answers/budget.json"),
        root.join("answers").join("budget.json"),
    )
    .unwrap();
    root
}

fn calls(root: &Path) -> Vec<String> {
    match std::fs::read_to_string(root.join("calls.log")) {
        Ok(t) => t.lines().map(String::from).collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn the_timer_runs_every_thirty_minutes() {
    assert_eq!(REFRESH_EVERY_SECS, 1800);
}

#[test]
fn refresh_runs_budget_refresh_json_and_parses_the_rows() {
    let r = root("refresh");
    let rows = refresh(&FleetBridge::new(&r)).unwrap();
    assert_eq!(rows, fixture_rows());
    assert_eq!(calls(&r), ["budget --refresh --json"]);
}

#[test]
fn a_failing_reader_is_a_fleet_error() {
    let r = root("failing");
    std::fs::write(r.join("fail"), "").unwrap();
    match refresh(&FleetBridge::new(&r)) {
        Err(CoreError::Fleet(e)) => {
            assert_eq!(e.exit_code, Some(3));
            assert!(e.stderr.contains("reader failed"));
        }
        other => panic!("expected a fleet error, got {other:?}"),
    }
}

#[test]
fn a_new_timer_is_due_at_once() {
    let t = RefreshTimer::new(REFRESH_EVERY_SECS);
    assert!(t.due(T0));
}

#[test]
fn the_first_tick_refreshes_and_the_next_waits_the_full_interval() {
    let r = root("tick");
    let fleet = FleetBridge::new(&r);
    let clock = FixedClock::at(T0);
    let mut t = RefreshTimer::new(REFRESH_EVERY_SECS);
    assert_eq!(t.tick(&fleet, &clock), Some(Ok(fixture_rows())));
    assert_eq!(calls(&r).len(), 1);
    assert_eq!(t.tick(&fleet, &clock), None);
    clock.advance(1799);
    assert!(!t.due(T0 + 1799));
    assert_eq!(t.tick(&fleet, &clock), None);
    assert_eq!(calls(&r).len(), 1);
    clock.advance(1);
    assert!(t.due(T0 + 1800));
    assert_eq!(t.tick(&fleet, &clock), Some(Ok(fixture_rows())));
    assert_eq!(
        calls(&r),
        ["budget --refresh --json", "budget --refresh --json"]
    );
}

#[test]
fn a_failed_tick_still_waits_the_full_interval() {
    let r = root("failtick");
    std::fs::write(r.join("fail"), "").unwrap();
    let fleet = FleetBridge::new(&r);
    let clock = FixedClock::at(T0);
    let mut t = RefreshTimer::new(REFRESH_EVERY_SECS);
    assert!(matches!(
        t.tick(&fleet, &clock),
        Some(Err(CoreError::Fleet(_)))
    ));
    std::fs::remove_file(r.join("fail")).unwrap();
    clock.advance(600);
    assert_eq!(t.tick(&fleet, &clock), None);
    clock.advance(1200);
    assert_eq!(t.tick(&fleet, &clock), Some(Ok(fixture_rows())));
    assert_eq!(calls(&r).len(), 2);
}

#[test]
fn a_clock_that_moves_back_is_not_due() {
    let r = root("back");
    let fleet = FleetBridge::new(&r);
    let clock = FixedClock::at(T0);
    let mut t = RefreshTimer::new(REFRESH_EVERY_SECS);
    assert!(t.tick(&fleet, &clock).is_some());
    assert!(!t.due(T0 - 5000));
}

#[test]
fn the_code_map_marks_the_refresh_present() {
    let map = std::fs::read_to_string(manifest().join("../../docs/code-map.md")).unwrap();
    assert!(map.contains(
        "- BUD-3 refresh (`refresh`, `RefreshTimer`, `budget_rows`, `budget_refresh`): present."
    ));
}
