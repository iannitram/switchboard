//! #379: a Project Manager job whose launcher dies at once frees the Project Manager's slot at
//! the next pass, and its failure comes back as a fleet error with the command, exit code and
//! stderr (CORE-2), so the app records it. The launcher is a stub `fleet.py` in a temp root; no
//! real fleet job runs.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use switchboard_core::autopilot::passes::{
    drift_pass, job_exit_file, pm_job, AutopilotState, FleetStarter, JobStarter,
};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::CoreError;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-pmfail-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A starter over a stub fleet whose `run` runs `script`; every lane may spend.
fn starter(root: &Path, script: &str) -> FleetStarter {
    std::fs::create_dir_all(root.join("tools").join("fleet")).unwrap();
    std::fs::write(root.join("tools").join("fleet").join("fleet.py"), script).unwrap();
    FleetStarter {
        fleet: FleetBridge::with_root(root),
        jobs_dir: root.join("jobs"),
        dev_root: root.join("dev"),
        machine: None,
        qwen_probe: Box::new(|_| false),
        retry_wait: Duration::ZERO,
    }
}

/// Asks the stub fleet nothing about spending.
struct Open(FleetStarter);

impl JobStarter for Open {
    fn may_spend(&mut self, _: &str) -> Result<bool, CoreError> {
        Ok(true)
    }
    fn start(&mut self, job: &Value, brief: &str) -> Result<(), CoreError> {
        self.0.start(job, brief)
    }
    fn finished(&mut self, job_id: &str) -> Result<bool, CoreError> {
        self.0.finished(job_id)
    }
}

const DYING: &str = r#"import sys
sys.stderr.write("fleet: no such lane\n")
sys.exit(3)
"#;

const SILENT_OK: &str = "import sys\nsys.exit(0)\n";

fn wait_for_exit(jobs: &Path, id: &str) {
    let file = job_exit_file(&jobs.join(format!("{id}.json")));
    let deadline = Instant::now() + Duration::from_secs(20);
    while !file.exists() {
        assert!(Instant::now() < deadline, "the stub launcher did not end");
        std::thread::sleep(Duration::from_millis(50));
    }
    // The exit file is written in one call; give the write a moment to land.
    std::thread::sleep(Duration::from_millis(50));
}

#[test]
fn a_launcher_that_dies_frees_the_slot_and_returns_its_error() {
    let root = temp_dir("dies");
    let mut starter = Open(starter(&root, DYING));
    let mut state = AutopilotState {
        drift_pending: vec!["o/a".to_string(), "o/b".to_string()],
        ..Default::default()
    };
    let first = drift_pass(100, &mut state, &mut starter).unwrap();
    assert_eq!(first.len(), 1);
    let id = first[0].clone();
    assert_eq!(state.pm_running.as_deref(), Some(id.as_str()));
    wait_for_exit(&root.join("jobs"), &id);

    // The next pass learns the job died: the slot is freed and the error has what CORE-2 asks.
    match drift_pass(200, &mut state, &mut starter) {
        Err(CoreError::Fleet(e)) => {
            assert_eq!(e.exit_code, Some(3));
            assert!(e.stderr.contains("no such lane"), "{e:?}");
            assert!(e.command.iter().any(|a| a == "run"), "{:?}", e.command);
        }
        other => panic!("expected a fleet error, got {other:?}"),
    }
    assert_eq!(state.pm_running, None);
    assert_eq!(state.drift_pending, ["o/b"]);

    // And the pass after that starts the next repo at once, not after the two-hour timeout.
    let next = drift_pass(300, &mut state, &mut starter).unwrap();
    assert_eq!(next.len(), 1);
    assert!(next[0].contains("-b-"), "{next:?}");
    assert!(state.drift_pending.is_empty());
}

#[test]
fn a_launcher_that_exits_0_with_no_output_is_finished() {
    let root = temp_dir("ok");
    let mut starter = starter(&root, SILENT_OK);
    let job = pm_job("o/a", "drift", 100, "claude");
    let id = job["id"].as_str().unwrap().to_string();
    starter.start(&job, "brief").unwrap();
    wait_for_exit(&root.join("jobs"), &id);
    assert_eq!(starter.finished(&id), Ok(true));
}

#[test]
fn a_launcher_still_running_is_not_finished() {
    let root = temp_dir("running");
    let mut starter = starter(&root, "import time\ntime.sleep(3)\n");
    let job = pm_job("o/a", "drift", 100, "claude");
    let id = job["id"].as_str().unwrap().to_string();
    starter.start(&job, "brief").unwrap();
    assert_eq!(starter.finished(&id), Ok(false));
}

/// A stub fleet that writes one line to `runs` beside itself each time it runs, then does what
/// `then` says.
fn counting(then: &str) -> String {
    format!(
        "import os, sys\nruns = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'runs')\n\
         with open(runs, 'a') as f:\n    f.write('run\\n')\n\
         count = len(open(runs).read().splitlines())\n{then}"
    )
}

fn runs(root: &Path) -> usize {
    std::fs::read_to_string(root.join("tools").join("fleet").join("runs"))
        .unwrap_or_default()
        .lines()
        .count()
}

#[test]
fn a_pm_launcher_that_says_not_now_twice_is_retried_once_then_fails() {
    // #432 (ruling 9): exit 75 with no output is retried once after a short wait (none here:
    // the wait is the starter's); a second 75 is a failure that says so.
    let root = temp_dir("not-now-twice");
    let mut starter = starter(
        &root,
        &counting("sys.stderr.write('busy\\n')\nsys.exit(75)\n"),
    );
    let job = pm_job("o/a", "drift", 100, "claude");
    let id = job["id"].as_str().unwrap().to_string();
    starter.start(&job, "brief").unwrap();
    wait_for_exit(&root.join("jobs"), &id);
    assert_eq!(runs(&root), 2, "retried once, not more");
    match starter.finished(&id) {
        Err(CoreError::Fleet(e)) => {
            assert_eq!(e.exit_code, Some(75));
            assert!(e.stderr.contains("twice with no output"), "{e:?}");
            assert!(e.stderr.contains("busy"), "{e:?}");
        }
        other => panic!("expected a fleet error, got {other:?}"),
    }
}

#[test]
fn a_pm_launcher_that_says_not_now_once_is_retried_and_its_second_run_counts() {
    let root = temp_dir("not-now-once");
    let script = counting("if count == 1:\n    sys.exit(75)\nprint('{}')\n");
    let mut starter = starter(&root, &script);
    let job = pm_job("o/a", "drift", 100, "claude");
    let id = job["id"].as_str().unwrap().to_string();
    starter.start(&job, "brief").unwrap();
    wait_for_exit(&root.join("jobs"), &id);
    assert_eq!(runs(&root), 2);
    assert_eq!(starter.finished(&id), Ok(true));
}

#[test]
fn a_pm_retry_waits_and_the_job_is_not_finished_while_it_does() {
    let root = temp_dir("not-now-wait");
    let mut starter = starter(&root, &counting("sys.exit(75)\n"));
    starter.retry_wait = Duration::from_secs(600);
    let job = pm_job("o/a", "drift", 100, "claude");
    let id = job["id"].as_str().unwrap().to_string();
    starter.start(&job, "brief").unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while runs(&root) < 1 {
        assert!(Instant::now() < deadline, "the stub launcher did not run");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(runs(&root), 1, "the retry waits");
    assert_eq!(
        starter.finished(&id),
        Ok(false),
        "a job waiting for its retry runs on"
    );
    assert!(!job_exit_file(&root.join("jobs").join(format!("{id}.json"))).exists());
}

#[test]
fn a_launcher_that_says_not_now_with_output_is_not_retried() {
    let root = temp_dir("not-now-output");
    let mut starter = starter(&root, &counting("print('{}')\nsys.exit(75)\n"));
    let job = pm_job("o/a", "drift", 100, "claude");
    let id = job["id"].as_str().unwrap().to_string();
    starter.start(&job, "brief").unwrap();
    wait_for_exit(&root.join("jobs"), &id);
    assert_eq!(runs(&root), 1);
    assert_eq!(starter.finished(&id), Ok(true));
}

#[test]
fn a_review_launcher_that_says_not_now_is_finished_and_not_retried() {
    // Review of #379: exit 75 is the fleet's "not now" (CORE-1), an answer and not an error.
    // #432 (ruling 9) retries only the Project Manager's launcher.
    let root = temp_dir("not-now");
    let mut starter = starter(&root, &counting("sys.exit(75)\n"));
    let job = serde_json::json!({ "id": "review-a-7", "repo": "o/a", "lane": "claude" });
    starter.start(&job, "brief").unwrap();
    wait_for_exit(&root.join("jobs"), "review-a-7");
    assert_eq!(runs(&root), 1);
    assert_eq!(starter.finished("review-a-7"), Ok(true));
}

#[test]
fn an_exit_file_still_being_written_is_not_an_error_yet() {
    let root = temp_dir("half-written");
    let mut starter = starter(&root, SILENT_OK);
    let job = pm_job("o/a", "drift", 100, "claude");
    let id = job["id"].as_str().unwrap().to_string();
    let jobs = root.join("jobs");
    std::fs::create_dir_all(&jobs).unwrap();
    let job_file = jobs.join(format!("{id}.json"));
    std::fs::write(&job_file, job.to_string()).unwrap();
    std::fs::write(job_exit_file(&job_file), "").unwrap();
    assert_eq!(starter.finished(&id), Ok(false));
}
