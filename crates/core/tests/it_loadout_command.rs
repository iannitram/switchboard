//! Locked tests for IT-W (the `loadout` command: this machine's loadout for the Loadout panel).
//! Written before the build (R28); the builder never edits this file. The real probe runner is
//! checked only with the `cargo` that runs these tests and a program that does not exist, so
//! the tests read nothing else on the machine and pass on all three CI runners.

use serde_json::{json, Value};
use std::sync::Mutex;
use switchboard_core::clock::UnixSeconds;
use switchboard_core::it::{
    read_loadout, read_this_machine, Loadout, ProbeResult, ProbeRunner, SystemProbeRunner,
};
use switchboard_core::platform::{current_os, loadout_probes, Probe};

const NOW: UnixSeconds = 1_800_000_000;

/// The cargo running this build: present on every runner, `--version` exits 0.
const CARGO: &str = env!("CARGO");

const CARGO_VERSION: Probe = Probe {
    name: "cargo",
    program: CARGO,
    args: &["--version"],
};

const CARGO_BAD_FLAG: Probe = Probe {
    name: "bad flag",
    program: CARGO,
    args: &["--switchboard-no-such-flag"],
};

const MISSING: Probe = Probe {
    name: "missing",
    program: "switchboard-it-w-no-such-program",
    args: &["--version"],
};

/// Answers every probe with exit 0 and the probe's name as its output; logs what it is asked.
struct Stub {
    asked: Mutex<Vec<String>>,
}

impl Stub {
    fn new() -> Self {
        Stub {
            asked: Mutex::new(Vec::new()),
        }
    }
}

impl ProbeRunner for Stub {
    fn run(&self, probe: &Probe) -> ProbeResult {
        self.asked.lock().unwrap().push(probe.name.to_string());
        ProbeResult {
            name: probe.name.to_string(),
            exit_code: Some(0),
            stdout: probe.name.to_string(),
        }
    }
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

// --- read_this_machine: this OS's probe list, through the runner it is given ---

#[test]
fn read_this_machine_runs_this_os_probe_list_once_in_order() {
    let stub = Stub::new();
    let _ = read_this_machine(&stub, NOW);
    let want: Vec<String> = loadout_probes(current_os())
        .iter()
        .map(|p| p.name.to_string())
        .collect();
    assert_eq!(*stub.asked.lock().unwrap(), want);
}

#[test]
fn read_this_machine_is_read_loadout_over_this_os_probe_list() {
    let got = read_this_machine(&Stub::new(), NOW);
    let want = read_loadout(loadout_probes(current_os()), &Stub::new(), NOW);
    assert_eq!(got, want);
    assert_eq!(got.read_at, NOW);
}

// --- SystemProbeRunner: runs the probe's program with its args ---

#[test]
fn a_probe_that_exits_0_gives_exit_0_and_its_stdout() {
    let result = SystemProbeRunner.run(&CARGO_VERSION);
    assert_eq!(result.name, "cargo");
    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.starts_with("cargo "), "{:?}", result.stdout);
}

#[test]
fn the_result_is_named_after_the_probe_not_the_program() {
    let result = SystemProbeRunner.run(&CARGO_BAD_FLAG);
    assert_eq!(result.name, "bad flag");
}

#[test]
fn a_probe_that_fails_gives_its_non_zero_exit_and_no_stderr_in_stdout() {
    let result = SystemProbeRunner.run(&CARGO_BAD_FLAG);
    let code = result.exit_code.expect("cargo exits with a code");
    assert_ne!(code, 0);
    assert!(
        !result.stdout.contains("switchboard-no-such-flag"),
        "stderr leaked into stdout: {:?}",
        result.stdout
    );
}

#[test]
fn a_missing_program_gives_no_exit_code_and_empty_stdout() {
    let result = SystemProbeRunner.run(&MISSING);
    assert_eq!(
        result,
        ProbeResult {
            name: "missing".to_string(),
            exit_code: None,
            stdout: String::new(),
        }
    );
}

#[test]
fn the_real_runner_feeds_read_loadout() {
    let loadout = read_loadout(&[CARGO_VERSION, MISSING], &SystemProbeRunner, NOW);
    assert_eq!(loadout.toolchains, ["cargo"]);
    assert_eq!(loadout.read_at, NOW);
}

// --- The JSON the Loadout panel receives: #93's Loadout, field for field ---

fn fixture() -> Loadout {
    Loadout {
        read_at: NOW,
        clis: vec![("claude".to_string(), true), ("codex".to_string(), false)],
        models_loaded: vec!["qwen3.8-27b".to_string()],
        toolchains: vec!["cargo".to_string(), "node".to_string()],
        busy: true,
        recent_errors: vec!["error 1".to_string()],
    }
}

#[test]
fn the_loadout_serialises_with_exactly_its_field_names() {
    let v = serde_json::to_value(fixture()).unwrap();
    assert_eq!(
        keys(&v),
        [
            "busy",
            "clis",
            "models_loaded",
            "read_at",
            "recent_errors",
            "toolchains"
        ]
    );
}

#[test]
fn the_loadout_serialises_field_for_field() {
    assert_eq!(
        serde_json::to_value(fixture()).unwrap(),
        json!({
            "read_at": 1_800_000_000,
            "clis": [["claude", true], ["codex", false]],
            "models_loaded": ["qwen3.8-27b"],
            "toolchains": ["cargo", "node"],
            "busy": true,
            "recent_errors": ["error 1"]
        })
    );
}

#[test]
fn an_empty_loadout_serialises_with_empty_lists() {
    let empty = Loadout {
        read_at: NOW,
        ..Loadout::default()
    };
    assert_eq!(
        serde_json::to_value(empty).unwrap(),
        json!({
            "read_at": 1_800_000_000,
            "clis": [],
            "models_loaded": [],
            "toolchains": [],
            "busy": false,
            "recent_errors": []
        })
    );
}

// --- The shell registers the command; the code map says so ---

#[test]
fn the_shell_registers_the_loadout_command() {
    let main = include_str!("../../../app/src-tauri/src/main.rs");
    let handler = main
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("generate_handler! in main.rs");
    assert!(handler.contains("commands::loadout,"), "{handler}");
}

#[test]
fn the_loadout_command_runs_off_the_main_thread_and_calls_the_core() {
    let commands = include_str!("../../../app/src-tauri/src/commands.rs");
    let at = commands
        .find("pub fn loadout(")
        .expect("commands.rs has `pub fn loadout(`");
    let before = &commands[..at];
    let attr = before
        .rfind("#[tauri::command")
        .expect("an attribute on loadout");
    assert!(
        !before[attr..].contains("fn "),
        "#[tauri::command] is not on loadout"
    );
    assert!(
        before[attr..].starts_with("#[tauri::command(async)]"),
        "loadout must be #[tauri::command(async)]: {}",
        &before[attr..]
    );
    let body_end = commands[at..]
        .find("\n}\n")
        .map_or(commands.len(), |i| at + i);
    let body = &commands[at..body_end];
    assert!(body.contains("current_loadout("), "{body}");
    assert!(body.contains("SystemProbeRunner"), "{body}");
    assert!(body.contains("SystemClock"), "{body}");
}

#[test]
fn the_code_map_lists_the_loadout_command_present() {
    let map = include_str!("../../../docs/code-map.md");
    let section: Vec<&str> = map
        .lines()
        .skip_while(|l| *l != "### it")
        .take_while(|l| !l.is_empty())
        .collect();
    assert!(
        section.contains(
            &"- IT-W loadout command (`SystemProbeRunner`, `read_this_machine`, `loadout`): present."
        ),
        "{section:?}"
    );
}

/// #339: a probe that runs past the limit is killed and has no exit code; a quick one still
/// answers. The slow program is `sleep`, so this runs on unix only.
#[cfg(unix)]
#[test]
fn a_slow_probe_is_killed_at_the_time_limit() {
    use std::time::{Duration, Instant};
    use switchboard_core::it::run_probe_within;
    const SLOW: Probe = Probe {
        name: "slow",
        program: "sleep",
        args: &["30"],
    };
    let start = Instant::now();
    let result = run_probe_within(&SLOW, Duration::from_millis(300));
    assert!(start.elapsed() < Duration::from_secs(5));
    assert_eq!(result.name, "slow");
    assert_eq!(result.exit_code, None);

    let fast = run_probe_within(&CARGO_VERSION, Duration::from_secs(30));
    assert_eq!(fast.exit_code, Some(0));
    assert!(fast.stdout.contains("cargo"));
}

/// #339: the kill reaches what the probe started, not only the probe. The probe is a shell that
/// starts a `sleep` with an odd length and waits for it; after the time limit no such `sleep`
/// is left running. Linux only: it looks for the process in `/proc`.
#[cfg(target_os = "linux")]
#[test]
fn a_killed_probe_leaves_no_child_running() {
    use std::time::Duration;
    use switchboard_core::it::run_probe_within;
    const SHELL: Probe = Probe {
        name: "shell",
        program: "sh",
        args: &["-c", "sleep 31337 & wait"],
    };
    let result = run_probe_within(&SHELL, Duration::from_millis(500));
    assert_eq!(result.exit_code, None);
    std::thread::sleep(Duration::from_millis(300));
    let running = std::fs::read_dir("/proc").unwrap().flatten().any(|entry| {
        std::fs::read(entry.path().join("cmdline"))
            .map(|c| String::from_utf8_lossy(&c).contains("sleep\x0031337"))
            .unwrap_or(false)
    });
    assert!(!running, "the probe's child is still running");
}
