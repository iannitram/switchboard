//! Locked tests for issue 103 (IT-2: `file_error_tickets` hands each recorded fleet error to
//! `fleet errors file`). The stub `fleet.py` is written into a fresh temp root per test; it
//! appends its argv to a calls file so the tests can count the calls.

use serde_json::Value;
use std::path::{Path, PathBuf};
use switchboard_core::fleet::{FleetBridge, FleetError, FleetErrorLog, FleetErrorReason};
use switchboard_core::it::file_error_tickets;

/// A fresh folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("switchboard-it-file-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A root whose `fleet.py` appends its argv (JSON, one line per call) to `$FLEET_STUB_CALLS`,
/// writes `$FLEET_STUB_STDERR` to stderr and exits with `$FLEET_STUB_EXIT` (default 0).
fn stub_root(name: &str) -> PathBuf {
    let root = scratch(name);
    let dir = root.join("tools").join("fleet");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("fleet.py"),
        "import json, os, sys\n\
         with open(os.environ[\"FLEET_STUB_CALLS\"], \"a\") as f:\n\
         \x20   f.write(json.dumps(sys.argv[1:]) + \"\\n\")\n\
         sys.stderr.write(os.environ.get(\"FLEET_STUB_STDERR\", \"\"))\n\
         sys.exit(int(os.environ.get(\"FLEET_STUB_EXIT\", \"0\")))\n",
    )
    .unwrap();
    root
}

/// The argv of each call the stub recorded (empty when it was never run).
fn calls(path: &Path) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .map(|line| {
            let v: Value = serde_json::from_str(line).unwrap();
            v.as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap().to_string())
                .collect()
        })
        .collect()
}

fn fleet_error(stderr: &str) -> FleetError {
    FleetError {
        command: vec![
            "python3".into(),
            "-I".into(),
            "/srv/pipeline/tools/fleet/fleet.py".into(),
            "budget".into(),
            "--json".into(),
        ],
        exit_code: Some(3),
        stderr: stderr.into(),
        reason: FleetErrorReason::Exit,
    }
}

#[test]
fn an_empty_log_runs_nothing_and_returns_zero() {
    let root = stub_root("empty");
    let calls_file = root.join("calls.txt");
    let bridge =
        FleetBridge::with_root(&root).with_env("FLEET_STUB_CALLS", calls_file.to_str().unwrap());
    let log = FleetErrorLog::default();
    assert_eq!(file_error_tickets(&bridge, &log), Ok(0));
    assert!(calls(&calls_file).is_empty());
}

#[test]
fn two_identical_errors_make_one_errors_file_call_and_return_two() {
    let root = stub_root("two");
    let calls_file = root.join("calls.txt");
    let bridge =
        FleetBridge::with_root(&root).with_env("FLEET_STUB_CALLS", calls_file.to_str().unwrap());
    let mut log = FleetErrorLog::default();
    log.record(fleet_error("same\n"));
    log.record(fleet_error("same\n"));
    assert_eq!(file_error_tickets(&bridge, &log), Ok(2));
    let got = calls(&calls_file);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains(&"errors".to_string()), "{got:?}");
    assert!(got[0].contains(&"file".to_string()), "{got:?}");
}

#[test]
fn a_failing_fleet_returns_its_error_and_leaves_the_log_unchanged() {
    let root = stub_root("fail");
    let calls_file = root.join("calls.txt");
    let bridge = FleetBridge::with_root(&root)
        .with_env("FLEET_STUB_CALLS", calls_file.to_str().unwrap())
        .with_env("FLEET_STUB_EXIT", "3")
        .with_env("FLEET_STUB_STDERR", "lock timeout\n");
    let mut log = FleetErrorLog::default();
    log.record(fleet_error("first\n"));
    log.record(fleet_error("second\n"));
    let before = log.errors.clone();
    let e = file_error_tickets(&bridge, &log).unwrap_err();
    assert_eq!(e.exit_code, Some(3));
    assert_eq!(e.reason, FleetErrorReason::Exit);
    assert_eq!(e.stderr, "lock timeout\n");
    assert_eq!(calls(&calls_file).len(), 1);
    assert_eq!(log.errors, before);
}
