//! #319: the lane `qwen` may spend only while the local model server answers.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use switchboard_core::abilities::FleetModels;
use switchboard_core::autopilot::passes::{
    drift_pass, AutopilotState, FleetStarter, JobStarter, QwenProbe,
};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::CoreError;

fn config() -> Value {
    json!({"machines": {
        "box": {"qwen_model": "m", "qwen_server_url": "http://127.0.0.1:1/v1"},
        "bare": {"qwen_model": "m"}
    }})
}

#[test]
fn the_url_is_read_from_the_machine_entry() {
    let c = config();
    assert_eq!(
        FleetModels::qwen_server_url(&c, Some("box")).as_deref(),
        Some("http://127.0.0.1:1/v1")
    );
    assert_eq!(FleetModels::qwen_server_url(&c, Some("bare")), None);
    assert_eq!(FleetModels::qwen_server_url(&c, Some("other")), None);
    assert_eq!(FleetModels::qwen_server_url(&c, None), None);
}

/// A fleet root whose `fleet.py` is the shared stub (`budget allow` exits `budget_exit`) and
/// whose config is `config`, when given.
fn root(name: &str, budget_exit: &str, config: Option<&Value>) -> FleetBridge {
    let stub = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fleet-stub/tools/fleet/fleet.py");
    let dir: PathBuf =
        std::env::temp_dir().join(format!("sb-qwenlane-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fleet_dir = dir.join("tools").join("fleet");
    std::fs::create_dir_all(&fleet_dir).unwrap();
    std::fs::copy(stub, fleet_dir.join("fleet.py")).unwrap();
    if let Some(config) = config {
        std::fs::write(fleet_dir.join("config.json"), config.to_string()).unwrap();
    }
    FleetBridge::with_root(&dir).with_env("FLEET_STUB_EXIT", budget_exit)
}

fn starter(fleet: FleetBridge, up: bool, asked: Arc<AtomicUsize>) -> FleetStarter {
    let probe: QwenProbe = Box::new(move |_url| {
        asked.fetch_add(1, Ordering::SeqCst);
        up
    });
    FleetStarter {
        fleet,
        jobs_dir: std::env::temp_dir(),
        dev_root: std::env::temp_dir(),
        machine: Some("box".to_string()),
        qwen_probe: probe,
    }
}

#[test]
fn qwen_may_spend_only_when_the_server_answers() {
    let c = config();
    let asked = Arc::new(AtomicUsize::new(0));
    let mut up = starter(root("up", "0", Some(&c)), true, asked.clone());
    assert_eq!(up.may_spend("qwen"), Ok(true));
    let mut down = starter(root("down", "0", Some(&c)), false, asked.clone());
    assert_eq!(down.may_spend("qwen"), Ok(false));
    assert_eq!(asked.load(Ordering::SeqCst), 2);
}

#[test]
fn a_missing_url_or_config_counts_as_down_and_is_never_probed() {
    let asked = Arc::new(AtomicUsize::new(0));
    let mut no_config = starter(root("noconf", "0", None), true, asked.clone());
    assert_eq!(no_config.may_spend("qwen"), Ok(false));
    let c = config();
    let mut bare = starter(root("bare", "0", Some(&c)), true, asked.clone());
    bare.machine = Some("bare".to_string());
    assert_eq!(bare.may_spend("qwen"), Ok(false));
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}

#[test]
fn a_braked_budget_says_no_without_a_probe_and_other_lanes_are_not_probed() {
    let c = config();
    let asked = Arc::new(AtomicUsize::new(0));
    let mut braked = starter(root("braked", "75", Some(&c)), true, asked.clone());
    assert_eq!(braked.may_spend("qwen"), Ok(false));
    let mut open = starter(root("other", "0", Some(&c)), false, asked.clone());
    assert_eq!(open.may_spend("gemini"), Ok(true));
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}

/// Asks a real `FleetStarter` about lanes and records the jobs it is told to start.
struct Wrapper {
    inner: FleetStarter,
    lanes: Vec<String>,
}

impl JobStarter for Wrapper {
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError> {
        self.inner.may_spend(lane)
    }
    fn start(&mut self, job: &Value, _: &str) -> Result<(), CoreError> {
        self.lanes.push(job["lane"].as_str().unwrap().to_string());
        Ok(())
    }
    fn finished(&mut self, _: &str) -> Result<bool, CoreError> {
        Ok(true)
    }
}

fn first_lane(name: &str, up: bool) -> String {
    let c = config();
    let mut w = Wrapper {
        inner: starter(root(name, "0", Some(&c)), up, Arc::new(AtomicUsize::new(0))),
        lanes: Vec::new(),
    };
    let mut state = AutopilotState {
        drift_pending: vec!["o/a".to_string()],
        ..Default::default()
    };
    drift_pass(100, &mut state, &mut w).unwrap();
    w.lanes.remove(0)
}

#[test]
fn the_drift_job_goes_to_qwen_when_it_is_up_and_to_the_next_lane_when_it_is_down() {
    assert_eq!(first_lane("lane-up", true), "qwen");
    assert_eq!(first_lane("lane-down", false), "gemini");
}
