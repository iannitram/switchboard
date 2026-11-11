//! Locked tests for issue 246 (FD-12: send a held Chat request as a fleet job another machine
//! pulls). Written before the build (R28); the builder never edits this file.
//!
//! The stub `fleet.py` is written into a fresh temp root per test. It appends its argv and the
//! stdin it read to `$FLEET_STUB_CALLS` (one JSON line per call), writes `$FLEET_STUB_STDERR` to
//! stderr and exits `$FLEET_STUB_EXIT` (default 0), so the tests see exactly what fleet is given.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use switchboard_core::fleet::{FleetBridge, FleetErrorReason};
use switchboard_core::it::{held_job, send_held_job, this_machine, HELD_JOB_ARGS};
use switchboard_core::CoreError;

const THIS: &str = "machine-a";

/// A fresh folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("switchboard-fd12-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const STUB: &str = "import json, os, sys\n\
data = sys.stdin.read()\n\
with open(os.environ[\"FLEET_STUB_CALLS\"], \"a\", encoding=\"utf-8\", newline=\"\\n\") as f:\n\
\x20   f.write(json.dumps({\"argv\": sys.argv[1:], \"stdin\": data}) + \"\\n\")\n\
sys.stderr.write(os.environ.get(\"FLEET_STUB_STDERR\", \"\"))\n\
sys.exit(int(os.environ.get(\"FLEET_STUB_EXIT\", \"0\")))\n";

/// A bridge over a fresh stub root, and the file its calls go to.
fn stub(name: &str, exit: Option<&str>) -> (FleetBridge, PathBuf) {
    let root = scratch(name);
    let dir = root.join("tools").join("fleet");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("fleet.py"), STUB).unwrap();
    let calls = root.join("calls.txt");
    let mut bridge =
        FleetBridge::with_root(&root).with_env("FLEET_STUB_CALLS", calls.to_str().unwrap());
    if let Some(code) = exit {
        bridge = bridge
            .with_env("FLEET_STUB_EXIT", code)
            .with_env("FLEET_STUB_STDERR", "attach: no request mode\n");
    }
    (bridge, calls)
}

/// Each call the stub recorded, as (argv, stdin); empty when it was never run.
fn calls(path: &Path) -> Vec<(Vec<String>, String)> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let v: Value = serde_json::from_str(line).unwrap();
            let argv = v["argv"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap().to_string())
                .collect();
            (argv, v["stdin"].as_str().unwrap().to_string())
        })
        .collect()
}

fn needs(tags: &[&str]) -> Vec<String> {
    tags.iter().map(|t| t.to_string()).collect()
}

fn argv() -> Vec<String> {
    needs(&["attach", "--request", "-", "--json"])
}

/// A home folder whose machine file holds `text`, or none when `text` is None.
fn home(name: &str, text: Option<&str>) -> PathBuf {
    let home = scratch(name);
    if let Some(text) = text {
        let dir = home.join(".config").join("software-depot");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("machine"), text).unwrap();
    }
    home
}

// --- this_machine ---

#[test]
fn the_env_name_wins_trimmed() {
    let h = home("env-wins", Some("from-file\n"));
    assert_eq!(
        this_machine(Some("  machine-a \n"), &h),
        Some("machine-a".to_string())
    );
}

#[test]
fn a_blank_env_name_falls_back_to_the_machine_file() {
    let h = home("env-blank", Some("from-file\n"));
    assert_eq!(this_machine(Some("   "), &h), Some("from-file".to_string()));
    assert_eq!(this_machine(Some(""), &h), Some("from-file".to_string()));
}

#[test]
fn no_env_name_reads_the_first_line_of_the_machine_file_trimmed() {
    let h = home("file", Some("  strix-halo  \nsecond line\n"));
    assert_eq!(this_machine(None, &h), Some("strix-halo".to_string()));
}

#[test]
fn no_env_and_no_file_is_none() {
    let h = home("none", None);
    assert_eq!(this_machine(None, &h), None);
}

#[test]
fn a_blank_machine_file_is_none() {
    let h = home("blank-file", Some("   \nlater\n"));
    assert_eq!(this_machine(None, &h), None);
    assert_eq!(this_machine(Some(" "), &h), None);
}

// --- send_held_job ---

#[test]
fn the_fleet_command_is_attach_request_from_stdin() {
    assert_eq!(HELD_JOB_ARGS, ["attach", "--request", "-"]);
}

#[test]
fn a_held_request_is_one_attach_call_with_the_job_on_stdin() {
    let (fleet, calls_file) = stub("sent", None);
    let unmet = needs(&["cuda", "web"]);
    let got = send_held_job(&fleet, "web-research", "find llama-swap docs", &unmet, THIS);
    let want = held_job("web-research", "find llama-swap docs", &unmet, THIS).unwrap();
    assert_eq!(got, Ok(want.clone()));
    let seen = calls(&calls_file);
    assert_eq!(seen.len(), 1, "one fleet call");
    assert_eq!(seen[0].0, argv());
    let given: Value = serde_json::from_str(&seen[0].1).unwrap();
    assert_eq!(given, want);
}

#[test]
fn the_job_fleet_gets_carries_the_request_kind_and_nothing_else_extra() {
    let (fleet, calls_file) = stub("kind", None);
    send_held_job(
        &fleet,
        "quick-lookup",
        "what is R25",
        &needs(&["web"]),
        THIS,
    )
    .unwrap();
    let given: Value = serde_json::from_str(&calls(&calls_file)[0].1).unwrap();
    assert_eq!(
        given,
        json!({
            "kind": "quick-lookup",
            "request": "what is R25",
            "needs": ["web"],
            "never_on": [THIS],
        })
    );
}

#[test]
fn the_machine_name_is_trimmed_before_it_goes_in_never_on() {
    let (fleet, calls_file) = stub("trim", None);
    let job = send_held_job(
        &fleet,
        "quick-lookup",
        "hi",
        &needs(&["web"]),
        "  machine-a \n",
    )
    .unwrap();
    assert_eq!(job["never_on"], json!([THIS]));
    let given: Value = serde_json::from_str(&calls(&calls_file)[0].1).unwrap();
    assert_eq!(given["never_on"], json!([THIS]));
}

#[test]
fn nothing_unmet_is_refused_and_fleet_is_not_called() {
    let (fleet, calls_file) = stub("empty", None);
    assert_eq!(
        send_held_job(&fleet, "quick-lookup", "hi", &[], THIS),
        Err(CoreError::Refused(
            "fd-12: nothing is unmet, so there is no job to send".to_string()
        ))
    );
    assert!(calls(&calls_file).is_empty());
}

#[test]
fn a_machine_with_no_name_is_refused_and_fleet_is_not_called() {
    let (fleet, calls_file) = stub("noname", None);
    for name in ["", "   "] {
        assert_eq!(
            send_held_job(&fleet, "quick-lookup", "hi", &needs(&["web"]), name),
            Err(CoreError::Refused(
                "fd-12: this machine has no fleet name (FLEET_MACHINE or the machine file)"
                    .to_string()
            )),
            "{name:?}"
        );
    }
    assert!(calls(&calls_file).is_empty());
}

#[test]
fn exit_75_is_not_queued() {
    let (fleet, calls_file) = stub("75", Some("75"));
    assert_eq!(
        send_held_job(&fleet, "quick-lookup", "hi", &needs(&["web"]), THIS),
        Err(CoreError::Refused(
            "fd-12: fleet did not queue the job (exit 75)".to_string()
        ))
    );
    assert_eq!(calls(&calls_file).len(), 1);
}

#[test]
fn a_failing_fleet_is_its_fleet_error() {
    let (fleet, calls_file) = stub("fail", Some("3"));
    match send_held_job(&fleet, "quick-lookup", "hi", &needs(&["web"]), THIS) {
        Err(CoreError::Fleet(e)) => {
            assert_eq!(e.exit_code, Some(3));
            assert_eq!(e.stderr.replace("\r\n", "\n"), "attach: no request mode\n");
            assert_eq!(e.reason, FleetErrorReason::Exit);
            assert!(
                e.command.ends_with(&argv()),
                "command ends with the argv: {:?}",
                e.command
            );
        }
        other => panic!("expected a fleet error, got {other:?}"),
    }
    assert_eq!(calls(&calls_file).len(), 1);
}

// --- The shell registers the command; the code map says so ---

fn shell(file: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../app/src-tauri/src")
            .join(file),
    )
    .unwrap()
}

#[test]
fn main_registers_the_chat_send_as_job_command() {
    let main = shell("main.rs");
    let start = main
        .find("tauri::generate_handler![")
        .expect("main.rs has generate_handler!");
    let end = start + main[start..].find(']').unwrap();
    let handler = &main[start..end];
    assert!(
        handler.contains("commands::chat_send_as_job,"),
        "generate_handler! lacks commands::chat_send_as_job: {handler}"
    );
}

#[test]
fn the_command_is_a_thin_call_into_send_held_job() {
    let commands = shell("commands.rs");
    let at = commands
        .find("pub fn chat_send_as_job(")
        .expect("commands.rs has pub fn chat_send_as_job(");
    let before = &commands[..at];
    let attr = before.rfind("#[tauri::command]").expect("an attribute");
    assert!(
        !before[attr..].contains("fn "),
        "#[tauri::command] is not on chat_send_as_job"
    );
    let body_end = commands[at..]
        .find("\n}\n")
        .map_or(commands.len(), |i| at + i);
    let body = &commands[at..body_end];
    for part in [
        "kind: String",
        "request: String",
        "unmet: Vec<String>",
        // #384: the command calls the core's `ask::send_as_job`, as the CLI and MCP do.
        "ask::send_as_job(",
        "record_fleet_error(",
        "FaceError::new(\"chat_send_as_job\"",
    ] {
        assert!(body.contains(part), "missing {part}: {body}");
    }
    let ask = include_str!("../src/frontdoor/ask.rs");
    for part in ["\"FLEET_MACHINE\"", "this_machine(", "send_held_job("] {
        assert!(ask.contains(part), "ask.rs: missing {part}");
    }
}

#[test]
fn the_code_map_marks_it_present() {
    let map = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md"),
    )
    .unwrap()
    .replace("\r\n", "\n");
    let section = map
        .split("\n### it\n")
        .nth(1)
        .and_then(|rest| rest.split("\n### ").next())
        .expect("a ### it section");
    assert!(
        section.contains(
            "- FD-12 send a held request (`this_machine`, `send_held_job`, `chat_send_as_job`, `ChatHeld`): present."
        ),
        "{section}"
    );
}
