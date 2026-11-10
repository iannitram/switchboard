//! CLI-1 (#64): `parse_args`, and `switchboard budget` run as the built binary
//! against the fleet stub under `crates/core/tests/fixtures/fleet-stub/`.

// The binary crate has no library target, so main.rs (and through it cli.rs and mcp.rs) is
// compiled in directly for `parse_args`; its own `#![allow(dead_code)]` covers the unused `main`.
#[path = "../src/main.rs"]
mod switchboard_main;

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;
use switchboard_core::budget::rows_from_json;
use switchboard_main::{parse_args, Args};

fn words(argv: &[&str]) -> Vec<String> {
    argv.iter().map(|w| w.to_string()).collect()
}

fn args(role: Option<&str>, json: bool, command: &[&str]) -> Args {
    Args {
        role: role.map(str::to_string),
        json,
        command: words(command),
    }
}

#[test]
fn flags_before_the_command() {
    assert_eq!(
        parse_args(&words(&["--json", "--role", "pm", "budget"])),
        Ok(args(Some("pm"), true, &["budget"]))
    );
}

#[test]
fn flags_after_the_command() {
    assert_eq!(
        parse_args(&words(&[
            "star",
            "switchboard#12",
            "--role",
            "pm",
            "--json"
        ])),
        Ok(args(Some("pm"), true, &["star", "switchboard#12"]))
    );
}

#[test]
fn flags_between_command_words() {
    assert_eq!(
        parse_args(&words(&["star", "--json", "switchboard#12"])),
        Ok(args(None, true, &["star", "switchboard#12"]))
    );
}

#[test]
fn no_flags() {
    assert_eq!(
        parse_args(&words(&["budget"])),
        Ok(args(None, false, &["budget"]))
    );
    assert_eq!(parse_args(&[]), Ok(args(None, false, &[])));
}

#[test]
fn role_without_a_value_is_an_error() {
    assert_eq!(
        parse_args(&words(&["budget", "--role"])),
        Err("--role needs a value".to_string())
    );
    assert_eq!(
        parse_args(&words(&["--role", "--json", "budget"])),
        Err("--role needs a value".to_string())
    );
}

#[test]
fn an_unknown_flag_is_an_error() {
    assert_eq!(
        parse_args(&words(&["budget", "--verbose"])),
        Err("unknown flag: --verbose".to_string())
    );
}

fn stub_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/fleet-stub")
}

/// An empty app config folder, so no `settings.json` on this machine names a pipeline (#359).
fn empty_config() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-budget-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The built binary with a clean fleet environment, an empty app config folder, and
/// `SWITCHBOARD_PIPELINE` set to `pipeline` (unset when `None`).
fn switchboard(argv: &[&str], pipeline: Option<&std::ffi::OsStr>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_switchboard"));
    cmd.args(argv)
        .env("SWITCHBOARD_CONFIG", empty_config())
        .env_remove("FLEET_ROOT")
        .env_remove("FLEET_STUB_EXIT")
        .env_remove("FLEET_STUB_STDERR")
        .env_remove("FLEET_STUB_BADJSON")
        .env_remove("SWITCHBOARD_PIPELINE");
    if let Some(root) = pipeline {
        cmd.env("SWITCHBOARD_PIPELINE", root);
    }
    cmd.output().expect("run switchboard")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn budget_prints_one_line_per_pool_in_pool_order() {
    let out = switchboard(&["budget"], Some(stub_root().as_os_str()));
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "chatgpt: 98.6% vs pace 81.4%, +17.2 pts, brake, 138 min old, stale\n\
         claude: 2% vs pace 3%, -1.0 pts, on-pace, 65 min old\n\
         claudeapi: 71.78% vs pace 74.04%, -2.3 pts, push, 138 min old\n"
    );
}

#[test]
fn budget_json_prints_the_same_rows_as_a_json_array() {
    let out = switchboard(&["budget", "--json"], Some(stub_root().as_os_str()));
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let printed: Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    let array = printed.as_array().expect("a JSON array");
    let pools: Vec<&str> = array.iter().map(|r| r["pool"].as_str().unwrap()).collect();
    assert_eq!(pools, ["chatgpt", "claude", "claudeapi"]);

    let fixture: Value = serde_json::from_str(
        &std::fs::read_to_string(stub_root().join("answers/budget.json")).unwrap(),
    )
    .unwrap();
    let expected = serde_json::to_value(rows_from_json(&fixture).unwrap()).unwrap();
    assert_eq!(printed, expected);
}

#[test]
fn budget_prints_not_recorded_where_a_number_is_missing() {
    let root = std::env::temp_dir().join(format!("sb-cli1-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tools").join("fleet")).unwrap();
    std::fs::write(
        root.join("tools").join("fleet").join("fleet.py"),
        "import sys\nsys.stdout.write('{\"solo\": {\"verdict\": \"push\", \"expected\": 40.0}}')\n",
    )
    .unwrap();
    let out = switchboard(&["budget"], Some(root.as_os_str()));
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "solo: not recorded vs pace 40%, pts not recorded, push, age not recorded\n"
    );
}

#[test]
fn budget_without_a_pipeline_exits_2() {
    for pipeline in [None, Some(std::ffi::OsStr::new(""))] {
        let out = switchboard(&["budget"], pipeline);
        assert_eq!(out.status.code(), Some(2));
        assert_eq!(stdout(&out), "");
        assert!(
            stderr(&out)
                .contains("switchboard: set SWITCHBOARD_PIPELINE to your pipeline checkout"),
            "stderr: {}",
            stderr(&out)
        );
    }
}

#[test]
fn an_empty_or_unknown_command_exits_2_with_usage() {
    for argv in [&[][..], &["frobnicate"][..], &["--json"][..]] {
        let out = switchboard(argv, Some(stub_root().as_os_str()));
        assert_eq!(out.status.code(), Some(2), "{argv:?}");
        assert!(stderr(&out).contains("usage:"), "stderr: {}", stderr(&out));
    }
}

#[test]
fn an_unknown_flag_exits_2() {
    let out = switchboard(&["budget", "--verbose"], Some(stub_root().as_os_str()));
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("unknown flag: --verbose"));
}
