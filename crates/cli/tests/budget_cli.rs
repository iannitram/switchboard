//! CLI-1 (#64): `switchboard` parses its flags anywhere and answers `budget` from the fleet stub
//! under `crates/core/tests/fixtures/fleet-stub/`, as text or `--json`; `loadout` and the other
//! unbuilt commands exit 2. The binary has no library target, so every case runs the built
//! binary.

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/fleet-stub")
}

fn switchboard(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(args)
        .env("SWITCHBOARD_PIPELINE", stub())
        .env_remove("FLEET_ROOT")
        .env_remove("FLEET_STUB_EXIT")
        .env_remove("FLEET_STUB_BADJSON")
        .output()
        .expect("run switchboard")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

/// The stub's three pools, sorted by pool name as `budget::read` returns them.
const BUDGET_TEXT: &str = "chatgpt: 98.6% vs pace 81.4%, brake\n\
claude: 2% vs pace 3%, on-pace\n\
claudeapi: 71.78% vs pace 74.04%, push\n";

#[test]
fn budget_prints_one_line_per_pool_in_pool_order() {
    let out = switchboard(&["budget"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), BUDGET_TEXT);
}

#[test]
fn budget_json_is_an_array_of_the_same_rows() {
    for args in [["--json", "budget"], ["budget", "--json"]] {
        let out = switchboard(&args);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        let rows: Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
        let rows = rows.as_array().expect("a JSON array");
        let pools: Vec<&str> = rows.iter().map(|r| r["pool"].as_str().unwrap()).collect();
        assert_eq!(pools, ["chatgpt", "claude", "claudeapi"]);
        assert_eq!(rows[0]["pct"], 98.6);
        assert_eq!(rows[0]["pace"], 81.4);
        assert_eq!(rows[0]["verdict"], "brake");
        assert_eq!(rows[0]["stale"], true);
        assert_eq!(rows[1]["reading_age_min"], 65);
        assert_eq!(rows[2]["verdict"], "push");
    }
}

#[test]
fn role_with_a_value_before_or_after_the_command() {
    for args in [
        &["--role", "satellite", "budget"][..],
        &["budget", "--role", "satellite"][..],
    ] {
        let out = switchboard(args);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        assert_eq!(stdout(&out), BUDGET_TEXT);
    }
}

#[test]
fn role_without_a_value_exits_2() {
    let out = switchboard(&["budget", "--role"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("--role needs a value"),
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), "");
}

#[test]
fn unknown_flag_exits_2() {
    let out = switchboard(&["--yaml", "budget"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("unknown flag: --yaml"),
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), "");
}

#[test]
fn loadout_exits_2_naming_it_1() {
    let out = switchboard(&["loadout"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("IT-1"), "{}", stderr(&out));
}

#[test]
fn unbuilt_commands_exit_2() {
    for command in ["sky", "waiting", "reports", "star"] {
        let out = switchboard(&[command]);
        assert_eq!(out.status.code(), Some(2), "{command}");
        assert!(
            stderr(&out).contains(&format!("switchboard: {command} is not built yet")),
            "{}",
            stderr(&out)
        );
    }
}

#[test]
fn empty_or_unknown_command_exits_2_with_usage() {
    for args in [&[][..], &["frobnicate"][..]] {
        let out = switchboard(args);
        assert_eq!(out.status.code(), Some(2));
        assert!(
            stderr(&out).contains("usage: switchboard"),
            "{}",
            stderr(&out)
        );
    }
}

#[test]
fn code_map_lists_cli_1() {
    let map = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md"),
    )
    .unwrap();
    let cli = map.split("### cli").nth(1).unwrap();
    let cli = cli.split("\n### ").next().unwrap();
    assert!(cli.contains(
        "- CLI-1 parse arguments, budget: present; sky, waiting, reports, star, loadout: planned."
    ));
}
