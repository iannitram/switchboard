// Locked tests for IT-1 (#244): the app keeps the newest loadout read for the panel and the
// router (FD-12). The builder never edits this file (R28). `commands.rs` takes it in with
// `#[cfg(test)] #[path = "loadout_tests.rs"] mod loadout_tests;`.
//
// The probes are this file's own fixed list and the runner a stub, so nothing on this machine
// is run and the answers are the same on every machine.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use switchboard_core::it::{Loadout, ProbeResult, ProbeRunner};
use switchboard_core::platform::Probe;

/// Three probes: a CLI, a toolchain and `fleet status`.
const PROBES: &[Probe] = &[
    Probe {
        name: "claude",
        program: "claude",
        args: &["auth", "status"],
    },
    Probe {
        name: "cargo",
        program: "cargo",
        args: &["--version"],
    },
    Probe {
        name: "fleet status",
        program: "python3",
        args: &["fleet.py", "status"],
    },
];

/// Every probe exits 0; `fleet status` prints `busy_out`. Counts the probes it ran.
struct Stub {
    busy_out: &'static str,
    ran: AtomicUsize,
}

impl Stub {
    fn new(busy_out: &'static str) -> Self {
        Self {
            busy_out,
            ran: AtomicUsize::new(0),
        }
    }
    fn ran(&self) -> usize {
        self.ran.load(Ordering::SeqCst)
    }
}

impl ProbeRunner for Stub {
    fn run(&self, probe: &Probe) -> ProbeResult {
        self.ran.fetch_add(1, Ordering::SeqCst);
        ProbeResult {
            name: probe.name.to_string(),
            timed_out: false,
            exit_code: Some(0),
            stdout: if probe.name == "fleet status" {
                self.busy_out.to_string()
            } else {
                String::new()
            },
        }
    }
}

/// What `read_loadout` gives for `PROBES` at `read_at`.
fn expected(read_at: i64, busy: bool) -> Loadout {
    Loadout {
        read_at,
        timed_out: Vec::new(),
        clis: vec![("claude".to_string(), true)],
        models_loaded: vec![],
        toolchains: vec!["cargo".to_string()],
        busy,
        recent_errors: vec![],
    }
}

fn state() -> AppState {
    AppState::new(std::env::temp_dir().join("switchboard-it1-unused-settings.json"))
}

#[test]
fn a_new_state_has_no_loadout() {
    assert_eq!(latest_loadout(&state()), None);
}

#[test]
fn a_read_runs_each_probe_once_stores_it_and_returns_it() {
    let state = state();
    let stub = Stub::new("DOING #12");
    let got = read_and_store(&state, PROBES, &stub, 1_000);
    assert_eq!(stub.ran(), 3);
    assert_eq!(got, expected(1_000, true));
    assert_eq!(latest_loadout(&state), Some(expected(1_000, true)));
}

#[test]
fn a_later_read_replaces_the_stored_one() {
    let state = state();
    read_and_store(&state, PROBES, &Stub::new("DOING #12"), 1_000);
    let got = read_and_store(&state, PROBES, &Stub::new("idle"), 1_060);
    assert_eq!(got, expected(1_060, false));
    assert_eq!(latest_loadout(&state), Some(expected(1_060, false)));
}

#[test]
fn a_read_taken_at_the_same_second_replaces_the_stored_one() {
    let state = state();
    read_and_store(&state, PROBES, &Stub::new("DOING #12"), 1_000);
    read_and_store(&state, PROBES, &Stub::new("idle"), 1_000);
    assert_eq!(latest_loadout(&state), Some(expected(1_000, false)));
}

#[test]
fn an_older_read_that_lands_late_does_not_replace_a_newer_one() {
    let state = state();
    read_and_store(&state, PROBES, &Stub::new("idle"), 1_060);
    let got = read_and_store(&state, PROBES, &Stub::new("DOING #12"), 1_000);
    assert_eq!(got, expected(1_060, false), "the answer is what is stored");
    assert_eq!(latest_loadout(&state), Some(expected(1_060, false)));
}

#[test]
fn store_loadout_keeps_the_newest_by_read_at() {
    let state = state();
    assert_eq!(store_loadout(&state, expected(5, true)), expected(5, true));
    assert_eq!(store_loadout(&state, expected(4, false)), expected(5, true));
    assert_eq!(
        store_loadout(&state, expected(6, false)),
        expected(6, false)
    );
    assert_eq!(latest_loadout(&state), Some(expected(6, false)));
}

#[test]
fn the_panel_gets_the_stored_loadout_without_running_a_probe() {
    let state = state();
    read_and_store(&state, PROBES, &Stub::new("DOING #12"), 1_000);
    let stub = Stub::new("idle");
    assert_eq!(
        current_loadout(&state, PROBES, &stub, 1_030),
        expected(1_000, true)
    );
    assert_eq!(stub.ran(), 0, "no probe runs while a loadout is stored");
}

#[test]
fn before_the_first_read_the_panel_reads_once_and_stores_it() {
    let state = state();
    let stub = Stub::new("idle");
    assert_eq!(
        current_loadout(&state, PROBES, &stub, 1_000),
        expected(1_000, false)
    );
    assert_eq!(stub.ran(), 3);
    assert_eq!(latest_loadout(&state), Some(expected(1_000, false)));
}

#[test]
fn the_loadout_event_and_its_period_are_named() {
    assert_eq!(LOADOUT_EVENT, "loadout");
    assert_eq!(LOADOUT_EVERY_SECS, 60);
}

#[test]
fn the_code_map_marks_it_present() {
    let code_map = include_str!("../../../docs/code-map.md");
    let it = code_map
        .split("\n### ")
        .find(|section| section.starts_with("it\n"))
        .expect("the code map has an `### it` section");
    assert!(
        it.lines().any(|line| line
            == "- IT-1 loadout every minute (`read_and_store`, `latest_loadout`, `current_loadout`, the `loadout` event): present."),
        "the it section lists IT-1's timer as present"
    );
}
