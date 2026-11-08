//! Locked tests for issue 1 (CORE-1: the fleet bridge runs `fleet.py <cmd> --json`). Qwen never
//! edits this file. The stub `fleet.py` is `tests/fixtures/fleet-stub/`; no test sets
//! `FLEET_ROOT` in this process (tests run in parallel), so the root is given with `with_root`.

use serde_json::Value;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use switchboard_core::fleet::{FleetBridge, FleetErrorReason};
use switchboard_core::platform::{current_os, python_launcher};

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fleet-stub")
}

fn answer(name: &str) -> Value {
    let text = std::fs::read_to_string(stub().join("answers").join(name)).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// A fresh folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("switchboard-fleet-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A root whose `fleet.py` prints back its arguments and its FLEET_ROOT as JSON.
fn echo_root() -> PathBuf {
    let root = scratch("echo");
    let dir = root.join("tools").join("fleet");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("fleet.py"),
        "import json, os, sys\n\
         print(json.dumps({\"argv\": sys.argv[1:], \"root\": os.environ.get(\"FLEET_ROOT\")}))\n",
    )
    .unwrap();
    root
}

fn unix(s: &str) -> String {
    s.replace("\r\n", "\n")
}

#[test]
fn a_non_empty_fleet_root_wins_over_the_pipeline_repo() {
    let p = Path::new("/srv/pipeline");
    assert_eq!(
        FleetBridge::resolve_root(p, None),
        PathBuf::from("/srv/pipeline")
    );
    assert_eq!(
        FleetBridge::resolve_root(p, Some(OsStr::new(""))),
        PathBuf::from("/srv/pipeline")
    );
    assert_eq!(
        FleetBridge::resolve_root(p, Some(OsStr::new("/srv/other"))),
        PathBuf::from("/srv/other")
    );
}

#[test]
fn with_root_uses_the_root_it_is_given() {
    let b = FleetBridge::with_root(Path::new("/srv/pipeline"));
    assert_eq!(b.root(), Path::new("/srv/pipeline"));
}

#[test]
fn the_command_line_is_the_launcher_then_the_script_then_the_args_then_json() {
    let b = FleetBridge::with_root(&stub());
    let (program, lead) = python_launcher(current_os());
    let mut want: Vec<String> = vec![program.to_string()];
    want.extend(lead.iter().map(|a| a.to_string()));
    want.push(b.script().to_string_lossy().into_owned());
    want.push("ledger".into());
    want.push("--since".into());
    want.push("7d".into());
    want.push("--json".into());
    assert_eq!(b.command_line(&["ledger", "--since", "7d"]), want);
}

#[test]
fn with_python_replaces_the_launcher() {
    let b = FleetBridge::with_root(&stub()).with_python("py-test", &["-X"]);
    let script = b.script().to_string_lossy().into_owned();
    assert_eq!(
        b.command_line(&["budget"]),
        vec!["py-test", "-X", &script, "budget", "--json"]
    );
}

#[test]
fn run_json_parses_what_the_command_prints() {
    let b = FleetBridge::with_root(&stub());
    assert_eq!(b.run_json(&["budget"]).unwrap(), answer("budget.json"));
    assert_eq!(b.run_json(&["actions"]).unwrap(), answer("actions.json"));
}

#[test]
fn run_json_passes_the_args_with_json_last_and_fleet_root_set_to_the_root() {
    let root = echo_root();
    let b = FleetBridge::with_root(&root);
    let v = b.run_json(&["ledger", "--since", "7d"]).unwrap();
    assert_eq!(
        v["argv"],
        serde_json::json!(["ledger", "--since", "7d", "--json"])
    );
    assert_eq!(
        v["root"],
        Value::String(root.to_string_lossy().into_owned())
    );
}

#[test]
fn a_non_zero_exit_is_a_fleet_error_with_command_code_and_stderr() {
    let b = FleetBridge::with_root(&stub())
        .with_env("FLEET_STUB_EXIT", "3")
        .with_env("FLEET_STUB_STDERR", "disk full\nline two\n");
    let e = b.run_json(&["budget"]).unwrap_err();
    assert_eq!(e.command, b.command_line(&["budget"]));
    assert_eq!(e.exit_code, Some(3));
    assert_eq!(unix(&e.stderr), "disk full\nline two\n");
    assert_eq!(e.reason, FleetErrorReason::Exit);
}

#[test]
fn json_that_does_not_parse_is_a_bad_json_fleet_error() {
    let b = FleetBridge::with_root(&stub()).with_env("FLEET_STUB_BADJSON", "1");
    let e = b.run_json(&["budget"]).unwrap_err();
    assert_eq!(e.exit_code, Some(0));
    assert!(
        matches!(e.reason, FleetErrorReason::BadJson(_)),
        "{:?}",
        e.reason
    );
    assert_eq!(e.command, b.command_line(&["budget"]));
}

#[test]
fn a_missing_fleet_py_is_an_exit_error_from_python() {
    let b = FleetBridge::with_root(&scratch("empty-root"));
    let e = b.run_json(&["budget"]).unwrap_err();
    assert_eq!(e.reason, FleetErrorReason::Exit);
    assert_eq!(e.exit_code, Some(2));
    assert!(!e.stderr.trim().is_empty());
}

#[test]
fn a_launcher_that_cannot_start_is_a_spawn_error() {
    let b = FleetBridge::with_root(&stub()).with_python("switchboard-no-such-python", &[]);
    let e = b.run_json(&["budget"]).unwrap_err();
    assert_eq!(e.exit_code, None);
    assert!(
        matches!(e.reason, FleetErrorReason::Spawn(_)),
        "{:?}",
        e.reason
    );
    assert_eq!(e.command[0], "switchboard-no-such-python");
    let e = b.run_status(&["budget", "allow", "qwen"]).unwrap_err();
    assert!(
        matches!(e.reason, FleetErrorReason::Spawn(_)),
        "{:?}",
        e.reason
    );
}

#[test]
fn run_status_answers_0_for_go() {
    let b = FleetBridge::with_root(&stub());
    assert_eq!(b.run_status(&["budget", "allow", "qwen"]), Ok(0));
}

#[test]
fn run_status_answers_75_for_not_now() {
    let b = FleetBridge::with_root(&stub()).with_env("FLEET_STUB_EXIT", "75");
    assert_eq!(b.run_status(&["budget", "allow", "qwen"]), Ok(75));
}

#[test]
fn run_status_turns_any_other_code_into_a_fleet_error() {
    let b = FleetBridge::with_root(&stub())
        .with_env("FLEET_STUB_EXIT", "3")
        .with_env("FLEET_STUB_STDERR", "lock timeout\n");
    let e = b.run_status(&["budget", "allow", "qwen"]).unwrap_err();
    assert_eq!(e.exit_code, Some(3));
    assert_eq!(e.reason, FleetErrorReason::Exit);
    assert_eq!(unix(&e.stderr), "lock timeout\n");
    assert_eq!(e.command, b.command_line(&["budget", "allow", "qwen"]));
}
