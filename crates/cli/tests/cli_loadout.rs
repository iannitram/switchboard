//! Locked tests for #241 (CLI-1: `switchboard loadout`). Qwen never edits this file (R28).
//! The text and the JSON are checked through `cli::loadout_text` and `cli::loadout_command`
//! over fixed `Loadout` values, so they do not depend on this OS's probe list. Two tests run
//! the built binary, which reads this machine's real probes.

// The binary crate has no library target, so the sources are compiled in directly.
#[path = "../src/main.rs"]
mod app;
// `cli.rs` reads `super::parse_args`, so it sits at this root beside the parser's import.
#[path = "../src/cli.rs"]
#[allow(dead_code, clippy::duplicate_mod)]
mod cli;

#[allow(unused_imports)]
use app::{parse_args, Args};

use std::process::{Command, ExitCode};

use serde_json::Value;
use switchboard_core::it::Loadout;

/// Every probe answered: two CLIs signed in, three toolchains found, two models, busy, two errors.
fn full() -> Loadout {
    Loadout {
        read_at: 1_700_000_000,
        clis: vec![
            ("claude".to_string(), true),
            ("codex".to_string(), false),
            ("gh".to_string(), true),
        ],
        models_loaded: vec!["qwen3-coder-30b".to_string(), "gpt-oss-20b".to_string()],
        toolchains: vec![
            "cargo".to_string(),
            "python3".to_string(),
            "gemini".to_string(),
        ],
        busy: true,
        recent_errors: vec![
            "fleet ghq: rate limited".to_string(),
            "merge_pr.py: behind main".to_string(),
        ],
    }
}

/// Every Linux probe failed: the CLIs are listed as not signed in and nothing else is set.
fn all_failed() -> Loadout {
    Loadout {
        read_at: 1_700_000_000,
        clis: ["claude", "codex", "gh"]
            .iter()
            .map(|name| (name.to_string(), false))
            .collect(),
        ..Loadout::default()
    }
}

const FULL_TEXT: &str = "\
claude  signed in
codex  not signed in
gh  signed in
cargo  found
node  not found
python3  found
gemini  found
llama-swap  2 loaded  qwen3-coder-30b, gpt-oss-20b
fleet status  busy
fleet errors  2 recent  fleet ghq: rate limited; merge_pr.py: behind main
";

const FAILED_TEXT: &str = "\
claude  not signed in
codex  not signed in
gh  not signed in
cargo  not found
node  not found
python3  not found
gemini  not found
llama-swap  no models loaded
fleet status  idle
fleet errors  none
";

#[test]
fn text_has_one_line_per_probe_in_the_panel_order() {
    let loadout = full();
    assert_eq!(cli::loadout_text(&loadout), FULL_TEXT);
    assert_eq!(loadout, full(), "the loadout is only read");
}

#[test]
fn text_for_failed_probes_names_each_state_and_has_no_detail() {
    assert_eq!(cli::loadout_text(&all_failed()), FAILED_TEXT);
}

#[test]
fn a_toolchain_outside_the_four_comes_after_them() {
    let loadout = Loadout {
        toolchains: vec!["go".to_string(), "cargo".to_string()],
        ..all_failed()
    };
    let text = cli::loadout_text(&loadout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[3..8],
        [
            "cargo  found",
            "node  not found",
            "python3  not found",
            "gemini  not found",
            "go  found"
        ]
    );
}

#[test]
fn no_probes_prints_the_no_probes_line() {
    assert_eq!(
        cli::loadout_text(&Loadout::default()),
        "no loadout probes on this OS\n"
    );
}

#[test]
fn busy_alone_still_counts_as_a_probe() {
    let loadout = Loadout {
        busy: true,
        ..Loadout::default()
    };
    let text = cli::loadout_text(&loadout);
    assert!(text.contains("fleet status  busy\n"), "got {text}");
}

#[test]
fn text_command_writes_the_text_and_exits_0() {
    let mut out: Vec<u8> = Vec::new();
    let code = cli::loadout_command(&full(), false, &mut out);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(String::from_utf8(out).unwrap(), FULL_TEXT);
}

#[test]
fn json_is_the_loadout_as_the_tauri_command_serializes_it() {
    let mut out: Vec<u8> = Vec::new();
    let code = cli::loadout_command(&full(), true, &mut out);
    assert_eq!(code, ExitCode::SUCCESS);
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.ends_with("}\n"),
        "one JSON object and a newline: {text}"
    );
    assert!(text.contains("\n  \""), "pretty-printed: {text}");
    let printed: Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(printed, serde_json::to_value(full()).unwrap());
    assert_eq!(printed["clis"][0], serde_json::json!(["claude", true]));
    assert_eq!(printed["read_at"], 1_700_000_000);
}

/// The built binary in a plain temp folder.
fn switchboard(argv: &[&str]) -> std::process::Output {
    let cwd = std::env::temp_dir().join(format!("sb-loadout-{}", std::process::id()));
    std::fs::create_dir_all(&cwd).unwrap();
    Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(argv)
        .current_dir(&cwd)
        .output()
        .expect("run switchboard")
}

#[test]
fn the_binary_prints_this_machines_loadout_as_json_and_exits_0() {
    let out = switchboard(&["loadout", "--json"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
    let printed: Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    for key in [
        "read_at",
        "clis",
        "models_loaded",
        "toolchains",
        "busy",
        "recent_errors",
    ] {
        assert!(printed.get(key).is_some(), "{key} missing in {printed}");
    }
}

#[test]
fn the_binary_prints_this_machines_loadout_as_text_and_exits_0() {
    let out = switchboard(&["loadout"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
    assert_eq!(stderr, "");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(!text.is_empty());
    for line in text.lines() {
        assert!(
            line == "no loadout probes on this OS" || line.contains("  "),
            "not a probe line: {line}"
        );
    }
}
