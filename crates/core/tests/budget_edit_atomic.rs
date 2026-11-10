//! BUD-2 (#340, #362): an edit is all or nothing, and a cap is only set where a cap exists and
//! in range.

use serde_json::json;
use std::path::{Path, PathBuf};
use switchboard_core::budget::{save_edit, PoolEdit};
use switchboard_core::clock::FixedClock;
use switchboard_core::fleet::FleetBridge;

const CONFIG: &str = r#"{
  "pools": {
    "claude": { "kind": "weekly", "pace": { "reset_weekday": "sun" } },
    "local": { "kind": "local" },
    "deepseek": { "kind": "fixed-period", "cap_usd": 10 }
  }
}
"#;

fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sb-bud2-atomic-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let fleet = root.join("tools").join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::create_dir_all(root.join("answers")).unwrap();
    let stub = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fleet-stub");
    std::fs::copy(stub.join("tools/fleet/fleet.py"), fleet.join("fleet.py")).unwrap();
    std::fs::copy(
        stub.join("answers/budget.json"),
        root.join("answers/budget.json"),
    )
    .unwrap();
    std::fs::write(fleet.join("config.json"), CONFIG).unwrap();
    root
}

fn config_text(root: &Path) -> String {
    std::fs::read_to_string(root.join("tools/fleet/config.json")).unwrap()
}

fn cap(pool: &str, cap: f64) -> PoolEdit {
    PoolEdit {
        pool: pool.into(),
        cap: Some(cap),
        ..PoolEdit::default()
    }
}

#[test]
fn a_failed_reread_leaves_config_and_ledger_as_they_were() {
    let r = root("reread");
    let log = r.join("pool-edits.jsonl");
    std::fs::write(&log, "{\"old\":1}\n").unwrap();
    // The re-read of `budget` fails: no answer file for it.
    std::fs::remove_file(r.join("answers/budget.json")).unwrap();
    let err = save_edit(
        &FleetBridge::new(&r),
        &cap("deepseek", 7.0),
        &log,
        &FixedClock::at(1),
    )
    .unwrap_err();
    assert!(err.to_string().contains("fleet error"), "{err}");
    assert_eq!(config_text(&r), CONFIG);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "{\"old\":1}\n");
}

#[test]
fn a_failed_reread_with_no_ledger_yet_leaves_no_ledger() {
    let r = root("reread-new");
    let log = r.join("pool-edits.jsonl");
    std::fs::remove_file(r.join("answers/budget.json")).unwrap();
    save_edit(
        &FleetBridge::new(&r),
        &cap("deepseek", 7.0),
        &log,
        &FixedClock::at(1),
    )
    .unwrap_err();
    assert_eq!(config_text(&r), CONFIG);
    assert!(!log.exists());
}

#[test]
fn an_unwritable_ledger_leaves_config_unchanged() {
    let r = root("ledgerdir");
    let log = r.join("pool-edits.jsonl");
    std::fs::create_dir(&log).unwrap();
    save_edit(
        &FleetBridge::new(&r),
        &cap("deepseek", 7.0),
        &log,
        &FixedClock::at(1),
    )
    .unwrap_err();
    assert_eq!(config_text(&r), CONFIG);
}

#[test]
fn a_cap_is_refused_on_a_pool_with_no_dollar_cap() {
    for pool in ["claude", "local"] {
        let r = root(pool);
        let log = r.join("pool-edits.jsonl");
        let err = save_edit(
            &FleetBridge::new(&r),
            &cap(pool, 5.0),
            &log,
            &FixedClock::at(1),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no dollar cap"), "{err}");
        assert_eq!(config_text(&r), CONFIG, "{pool}");
        assert!(!log.exists(), "{pool}");
    }
}

#[test]
fn a_cap_out_of_range_is_refused() {
    for bad in [0.0, 1e300, 1.8e19, 100_000.5, f64::INFINITY] {
        let r = root("range");
        let log = r.join("pool-edits.jsonl");
        save_edit(
            &FleetBridge::new(&r),
            &cap("deepseek", bad),
            &log,
            &FixedClock::at(1),
        )
        .unwrap_err();
        assert_eq!(config_text(&r), CONFIG, "{bad}");
        assert!(!log.exists(), "{bad}");
    }
}

#[test]
fn the_largest_cap_is_accepted() {
    let r = root("max");
    let saved = save_edit(
        &FleetBridge::new(&r),
        &cap("deepseek", 100_000.0),
        &r.join("e.jsonl"),
        &FixedClock::at(1),
    )
    .unwrap();
    assert_eq!(saved.ledger_row["set"], json!({"cap_usd": 100000.0}));
}

/// A failure in the undo is reported with the answer, not swallowed.
#[cfg(unix)]
#[test]
fn an_undo_that_fails_is_reported() {
    use std::os::unix::fs::PermissionsExt;
    let r = root("undo-fails");
    let log = r.join("pool-edits.jsonl");
    std::fs::write(&log, "{\"old\":1}\n").unwrap();
    // The re-read fails, and as it does the script makes the ledger read-only, so the ledger
    // cannot be cut back.
    std::fs::write(
        r.join("tools/fleet/fleet.py"),
        "import os, sys\nos.chmod(os.environ['LEDGER'], 0o444)\nsys.exit(1)\n",
    )
    .unwrap();
    let bridge = FleetBridge::new(&r).with_env("LEDGER", log.to_str().unwrap());
    let err = save_edit(&bridge, &cap("deepseek", 7.0), &log, &FixedClock::at(1)).unwrap_err();
    // Still a fleet error (the app logs those), with the undo note on it.
    match &err {
        switchboard_core::CoreError::Fleet(e) => {
            let note = e.undo_note.as_deref().unwrap_or("");
            assert!(note.contains("pool-edits.jsonl"), "{note}");
        }
        other => panic!("expected a fleet error, got {other:?}"),
    }
    let text = err.to_string();
    assert!(text.contains("fleet error"), "{text}");
    assert!(text.contains("could not be fully taken back"), "{text}");
    assert!(text.contains("pool-edits.jsonl"), "{text}");
    // The config was put back all the same.
    assert_eq!(config_text(&r), CONFIG);
    std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o644)).unwrap();
}
