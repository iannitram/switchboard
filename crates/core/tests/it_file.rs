//! Locked tests for issue 103 (IT-2: `file_error_tickets` hands each recorded fleet error to
//! `fleet errors file --records -`). The stub `fleet.py` is written into a fresh temp root per
//! test; it appends its argv and the stdin it read to a calls file, so the tests check what
//! fleet is given: the argv and each record.

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

/// A root whose `fleet.py` appends `{"argv": [...], "stdin": "..."}` (JSON, one line per call)
/// to `$FLEET_STUB_CALLS`, writes `$FLEET_STUB_STDERR` to stderr and exits with
/// `$FLEET_STUB_EXIT` (default 0).
fn stub_root(name: &str) -> PathBuf {
    let root = scratch(name);
    let dir = root.join("tools").join("fleet");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("fleet.py"),
        "import json, os, sys\n\
         data = sys.stdin.read()\n\
         with open(os.environ[\"FLEET_STUB_CALLS\"], \"a\") as f:\n\
         \x20   f.write(json.dumps({\"argv\": sys.argv[1:], \"stdin\": data}) + \"\\n\")\n\
         sys.stderr.write(os.environ.get(\"FLEET_STUB_STDERR\", \"\"))\n\
         sys.exit(int(os.environ.get(\"FLEET_STUB_EXIT\", \"0\")))\n",
    )
    .unwrap();
    root
}

/// One call the stub recorded: its argv and the stdin it read.
struct Call {
    argv: Vec<String>,
    stdin: String,
}

/// Each call the stub recorded (empty when it was never run).
fn calls(path: &Path) -> Vec<Call> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .map(|line| {
            let v: Value = serde_json::from_str(line).unwrap();
            Call {
                argv: v["argv"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap().to_string())
                    .collect(),
                stdin: v["stdin"].as_str().unwrap().to_string(),
            }
        })
        .collect()
}

/// The argv `file_error_tickets` must give the stub: the records mode, reading stdin.
fn records_argv() -> Vec<String> {
    ["errors", "file", "--records", "-", "--json"]
        .iter()
        .map(|a| a.to_string())
        .collect()
}

/// The record `fleet errors file --records` reads for `fleet_error(stderr)`.
fn record(stderr: &str) -> Value {
    serde_json::json!({
        "command": ["python3", "-I", "/srv/pipeline/tools/fleet/fleet.py", "budget", "--json"],
        "exit_code": 3,
        "stderr": stderr,
    })
}

/// `s` with Windows line ends made Unix (Python writes `\r\n` to a text stream there).
fn unix(s: &str) -> String {
    s.replace("\r\n", "\n")
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
    assert_eq!(got.len(), 1, "one errors file call for the whole log");
    assert_eq!(got[0].argv, records_argv());
    // Both records reach fleet whole and identical (same command, exit code and stderr), so
    // fleet builds the same signature for both: the first files the ticket and the second is
    // a comment on it, never a second ticket (`fleet errors file --records`).
    let given: Value = serde_json::from_str(&got[0].stdin).unwrap();
    assert_eq!(
        given,
        serde_json::json!([record("same\n"), record("same\n")])
    );
}

#[test]
fn each_record_is_handed_over_as_it_was_recorded_oldest_first() {
    let root = stub_root("order");
    let calls_file = root.join("calls.txt");
    let bridge =
        FleetBridge::with_root(&root).with_env("FLEET_STUB_CALLS", calls_file.to_str().unwrap());
    let mut log = FleetErrorLog::default();
    log.record(fleet_error("first\nline two\n"));
    let mut spawn = fleet_error("");
    spawn.exit_code = None;
    spawn.reason = FleetErrorReason::Spawn("not found".into());
    log.record(spawn);
    assert_eq!(file_error_tickets(&bridge, &log), Ok(2));
    let got = calls(&calls_file);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].argv, records_argv());
    let given: Value = serde_json::from_str(&got[0].stdin).unwrap();
    let mut second = record("");
    second["exit_code"] = Value::Null;
    assert_eq!(
        given,
        serde_json::json!([record("first\nline two\n"), second])
    );
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
    assert_eq!(unix(&e.stderr), "lock timeout\n");
    let got = calls(&calls_file);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].argv, records_argv());
    let given: Value = serde_json::from_str(&got[0].stdin).unwrap();
    assert_eq!(
        given,
        serde_json::json!([record("first\n"), record("second\n")])
    );
    assert_eq!(log.errors, before);
}
