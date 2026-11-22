//! `bug-loop-live` (#411): wire `specs/bug-loop.json` through `Engine` and `OwnerSteps`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde_json::json;
use switchboard_core::clock::FixedClock;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::runner::bug_loop::{drive_bug, run_end_to_end, BugEnv, BugOutcome};
use switchboard_core::runner::engine::{Checker, Outcome, StepRunner};
use switchboard_core::runner::spec::{self, Check, Step};
use switchboard_core::runner::state::StateStore;
use switchboard_core::runner::waiting::RunnerItems;
use switchboard_core::CoreError;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-bug-loop-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tools").join("fleet")).unwrap();
    std::fs::write(
        dir.join("tools").join("fleet").join("fleet.py"),
        "print('{\"ok\": true}')\n",
    )
    .unwrap();
    dir
}

struct AllPass;
impl Checker for AllPass {
    fn holds(&self, _: &Check, outcome: &Outcome) -> bool {
        matches!(outcome, Outcome::Exit { code: 0, .. })
    }
}

struct StubRunner {
    has_releases: bool,
    released: AtomicBool,
    steps: Mutex<Vec<String>>,
}

impl StepRunner for StubRunner {
    fn run(&self, step: &Step, unit: &str) -> Result<Outcome, CoreError> {
        self.steps
            .lock()
            .unwrap()
            .push(format!("{unit}:{}", step.id));
        if step.id == "check-release" && self.has_releases && !self.released.load(Ordering::SeqCst)
        {
            return Ok(Outcome::NotNow);
        }
        Ok(Outcome::Exit {
            code: 0,
            json: Some(json!({"ok": true})),
        })
    }
}

#[test]
fn report_runs_from_intake_to_closed_issue_without_releases() {
    let dir = tmp("no-releases");
    let spec_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../specs/bug-loop.json");
    let spec = spec::load(&spec_path).unwrap();
    let bridge = FleetBridge::new(&dir);
    let runner = StubRunner {
        has_releases: false,
        released: AtomicBool::new(false),
        steps: Mutex::new(Vec::new()),
    };
    let store = StateStore::new(&dir);
    let waiting = RunnerItems::new(&dir);
    let clock = FixedClock::at(1_000);
    let env = BugEnv {
        bridge: &bridge,
        runner: &runner,
        checks: &AllPass,
        store: &store,
        waiting: &waiting,
        clock: &clock,
    };

    let (intake, fix) = run_end_to_end(&spec, &env, "report-1", "bug-1", 60).unwrap();
    assert!(
        matches!(intake, BugOutcome::Done { final_state, .. } if final_state == "report-archived")
    );
    assert!(matches!(fix, BugOutcome::Done { final_state, .. } if final_state == "closed"));
    assert!(waiting.list().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn app_with_releases_stays_open_until_release_tag_contains_fix() {
    let dir = tmp("with-releases");
    let spec_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../specs/bug-loop.json");
    let spec = spec::load(&spec_path).unwrap();
    let bridge = FleetBridge::new(&dir);
    let runner = StubRunner {
        has_releases: true,
        released: AtomicBool::new(false),
        steps: Mutex::new(Vec::new()),
    };
    let store = StateStore::new(&dir);
    let waiting = RunnerItems::new(&dir);
    let clock = FixedClock::at(1_000);
    let env = BugEnv {
        bridge: &bridge,
        runner: &runner,
        checks: &AllPass,
        store: &store,
        waiting: &waiting,
        clock: &clock,
    };

    let (_, fix) = run_end_to_end(&spec, &env, "report-2", "bug-2", 60).unwrap();
    assert!(matches!(fix, BugOutcome::WaitingRelease { state, .. } if state == "swept"));

    // Once a release tag contains the fix, resuming closes the issue.
    runner.released.store(true, Ordering::SeqCst);
    let resumed = drive_bug(&spec, &env, "bug-2", None, 20).unwrap();
    assert!(matches!(resumed, BugOutcome::Done { final_state, .. } if final_state == "closed"));
    let _ = std::fs::remove_dir_all(&dir);
}
