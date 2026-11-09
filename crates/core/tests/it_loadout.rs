//! Locked tests for #93 (IT-1: the loadout from the platform's probe list). The builder never
//! edits this file (R28). Every probe goes through a stub `ProbeRunner`: nothing runs a real
//! command, touches the network or reads this machine.

use std::collections::HashMap;
use std::sync::Mutex;
use switchboard_core::clock::UnixSeconds;
use switchboard_core::it::{read_loadout, Loadout, ProbeResult, ProbeRunner};
use switchboard_core::platform::{loadout_probes, Os, Probe};

const NOW: UnixSeconds = 1_800_000_000;

const CLIS: [&str; 4] = ["claude", "codex", "gemini", "gh"];

/// Answers each probe by name from `answers`; a probe it has no answer for gets no exit code
/// (the program is missing). It logs every probe name it is asked.
struct Stub {
    answers: HashMap<&'static str, (Option<i32>, String)>,
    asked: Mutex<Vec<String>>,
}

impl Stub {
    fn new(answers: &[(&'static str, Option<i32>, &str)]) -> Self {
        Stub {
            answers: answers
                .iter()
                .map(|(name, code, out)| (*name, (*code, out.to_string())))
                .collect(),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl ProbeRunner for Stub {
    fn run(&self, probe: &Probe) -> ProbeResult {
        self.asked.lock().unwrap().push(probe.name.to_string());
        let (exit_code, stdout) = self
            .answers
            .get(probe.name)
            .cloned()
            .unwrap_or((None, String::new()));
        ProbeResult {
            name: probe.name.to_string(),
            exit_code,
            stdout,
        }
    }
}

fn linux() -> &'static [Probe] {
    loadout_probes(Os::Linux)
}

fn read(answers: &[(&'static str, Option<i32>, &str)]) -> Loadout {
    read_loadout(linux(), &Stub::new(answers), NOW)
}

/// The signed-in flag the loadout holds for `cli`, or None when the CLI is not listed.
fn signed_in(loadout: &Loadout, cli: &str) -> Option<bool> {
    loadout
        .clis
        .iter()
        .find(|(name, _)| name == cli)
        .map(|(_, signed)| *signed)
}

fn errors_out(n: usize) -> String {
    (1..=n).map(|i| format!("error {i}\n")).collect()
}

#[test]
fn the_linux_probe_list_names_the_ten_probes_in_order() {
    let names: Vec<&str> = linux().iter().map(|p| p.name).collect();
    assert_eq!(
        names,
        [
            "claude",
            "codex",
            "gemini",
            "gh",
            "llama-swap",
            "cargo",
            "node",
            "python3",
            "fleet status",
            "fleet errors",
        ]
    );
    assert!(linux().iter().all(|p| !p.program.is_empty()));
}

#[test]
fn the_mac_and_windows_probe_lists_are_empty() {
    assert!(loadout_probes(Os::Mac).is_empty());
    assert!(loadout_probes(Os::Windows).is_empty());
}

#[test]
fn every_probe_is_run_once_through_the_runner() {
    let stub = Stub::new(&[]);
    let _ = read_loadout(linux(), &stub, NOW);
    let mut asked = stub.asked();
    asked.sort();
    let mut want: Vec<String> = linux().iter().map(|p| p.name.to_string()).collect();
    want.sort();
    assert_eq!(asked, want);
}

#[test]
fn claude_answering_0_is_signed_in_and_codex_answering_1_is_not() {
    let loadout = read(&[("claude", Some(0), ""), ("codex", Some(1), "")]);
    assert_eq!(signed_in(&loadout, "claude"), Some(true));
    assert_eq!(signed_in(&loadout, "codex"), Some(false));
}

#[test]
fn a_cli_with_no_exit_code_is_listed_as_not_signed_in() {
    let loadout = read(&[("claude", Some(0), ""), ("gh", Some(0), "")]);
    assert_eq!(loadout.clis.len(), CLIS.len());
    for cli in CLIS {
        assert!(signed_in(&loadout, cli).is_some(), "{cli} is not listed");
    }
    assert_eq!(signed_in(&loadout, "claude"), Some(true));
    assert_eq!(signed_in(&loadout, "codex"), Some(false));
    assert_eq!(signed_in(&loadout, "gemini"), Some(false));
    assert_eq!(signed_in(&loadout, "gh"), Some(true));
}

#[test]
fn only_the_four_clis_are_listed_as_clis() {
    let all_ok: Vec<(&'static str, Option<i32>, &str)> =
        linux().iter().map(|p| (p.name, Some(0), "")).collect();
    let loadout = read(&all_ok);
    let mut names: Vec<&str> = loadout.clis.iter().map(|(n, _)| n.as_str()).collect();
    names.sort();
    assert_eq!(names, ["claude", "codex", "gemini", "gh"]);
}

#[test]
fn a_loaded_model_list_fills_models_loaded() {
    let running = r#"{"running":[{"model":"qwen3.8-27b","state":"ready"},{"model":"labeller","state":"ready"}]}"#;
    let loadout = read(&[("llama-swap", Some(0), running)]);
    assert_eq!(loadout.models_loaded, ["qwen3.8-27b", "labeller"]);
}

#[test]
fn no_running_models_or_no_llama_swap_gives_no_models() {
    let empty = read(&[("llama-swap", Some(0), r#"{"running":[]}"#)]);
    assert!(empty.models_loaded.is_empty());
    let down = read(&[("llama-swap", Some(7), "")]);
    assert!(down.models_loaded.is_empty());
    let missing = read(&[]);
    assert!(missing.models_loaded.is_empty());
}

#[test]
fn an_unreadable_model_answer_gives_no_models_and_no_panic() {
    let loadout = read(&[("llama-swap", Some(0), "<html>not json</html>")]);
    assert!(loadout.models_loaded.is_empty());
    assert_eq!(loadout.read_at, NOW);
}

#[test]
fn a_missing_toolchain_is_left_out() {
    let loadout = read(&[
        ("cargo", Some(0), "cargo 1.91.0\n"),
        ("node", None, ""),
        ("python3", Some(127), ""),
    ]);
    assert_eq!(loadout.toolchains, ["cargo"]);
}

#[test]
fn every_present_toolchain_is_listed() {
    let loadout = read(&[
        ("cargo", Some(0), "cargo 1.91.0\n"),
        ("node", Some(0), "v22.11.0\n"),
        ("python3", Some(0), "Python 3.13.1\n"),
    ]);
    let mut toolchains = loadout.toolchains.clone();
    toolchains.sort();
    assert_eq!(toolchains, ["cargo", "node", "python3"]);
}

#[test]
fn busy_follows_the_fleet_status_probe() {
    let doing = "TOKEN       STATE  AGE  SPEC            NEEDS\nstrix-halo  DOING  3m   switchboard#93  -\n";
    let idle = "TOKEN       STATE  AGE  SPEC  NEEDS\nstrix-halo  IDLE   12m  -     -\n";
    assert!(read(&[("fleet status", Some(0), doing)]).busy);
    assert!(!read(&[("fleet status", Some(0), idle)]).busy);
    assert!(!read(&[("fleet status", Some(1), doing)]).busy);
    assert!(!read(&[]).busy);
}

#[test]
fn more_than_five_errors_give_the_newest_five() {
    let out = errors_out(7);
    let loadout = read(&[("fleet errors", Some(0), out.as_str())]);
    assert_eq!(
        loadout.recent_errors,
        ["error 3", "error 4", "error 5", "error 6", "error 7"]
    );
}

#[test]
fn five_or_fewer_errors_are_all_kept_and_blank_lines_are_not_errors() {
    let out = "error 1\n\nerror 2\n";
    let loadout = read(&[("fleet errors", Some(0), out)]);
    assert_eq!(loadout.recent_errors, ["error 1", "error 2"]);
    let none = read(&[("fleet errors", Some(0), "")]);
    assert!(none.recent_errors.is_empty());
}

#[test]
fn an_empty_probe_list_gives_an_empty_loadout_read_now() {
    let stub = Stub::new(&[("claude", Some(0), "")]);
    let loadout = read_loadout(&[], &stub, NOW);
    assert_eq!(
        loadout,
        Loadout {
            read_at: NOW,
            ..Loadout::default()
        }
    );
    assert!(stub.asked().is_empty());
}

#[test]
fn every_probe_failing_never_fails_the_read() {
    let all_missing = read(&[]);
    assert_eq!(all_missing.read_at, NOW);
    assert!(all_missing.clis.iter().all(|(_, signed)| !signed));
    assert!(all_missing.models_loaded.is_empty());
    assert!(all_missing.toolchains.is_empty());
    assert!(!all_missing.busy);
    assert!(all_missing.recent_errors.is_empty());
}
