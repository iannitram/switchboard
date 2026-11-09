//! AUTO-6: the satellite's script loops. `may_start` keeps it to Qwen plus one service job (R10),
//! `next_command` is `fleet run --next <lane>` (the real fleet has no `--lane` flag), `tick`
//! starts the lanes the satellite file allows and the budget allows, and `classify_failed`
//! hands each failed card of a finished loop to AUTO-9. Fakes stand in for the runner and the
//! budget; `classify_failed` runs against an inline fleet stub in a temp root.

use serde_json::json;
use std::cell::RefCell;
use std::path::PathBuf;
use switchboard_core::autopilot::classify::FailureClass;
use switchboard_core::autopilot::driver::{
    cards, classify_failed, failed_cards, load_config, may_start, next_command, run_args, tick,
    LaneRunner, SatelliteConfig, FLEET_LANES, SATELLITE_FILE,
};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::CoreError;

fn running(lanes: &[&str]) -> Vec<String> {
    lanes.iter().map(|l| l.to_string()).collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-auto6-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A runner that starts nothing real: `running` is what it says, `started` logs each start.
struct FakeRunner {
    running: Vec<String>,
    started: Vec<String>,
    asked: usize,
}

impl FakeRunner {
    fn with(lanes: &[&str]) -> Self {
        FakeRunner {
            running: running(lanes),
            started: Vec::new(),
            asked: 0,
        }
    }
}

impl LaneRunner for FakeRunner {
    fn running(&mut self) -> Vec<String> {
        self.asked += 1;
        self.running.clone()
    }
    fn start(&mut self, lane: &str) -> Result<(), CoreError> {
        self.started.push(lane.to_string());
        Ok(())
    }
}

fn on(lanes: &[&str]) -> SatelliteConfig {
    SatelliteConfig {
        satellite_on: true,
        lanes: strings(lanes),
    }
}

// ---- may_start (R10) ----

#[test]
fn nothing_running_lets_qwen_and_gemini_start() {
    assert!(may_start(&running(&[]), "qwen"));
    assert!(may_start(&running(&[]), "gemini"));
}

#[test]
fn qwen_running_lets_gemini_start_but_not_a_second_qwen() {
    let r = running(&["qwen"]);
    assert!(may_start(&r, "gemini"));
    assert!(!may_start(&r, "qwen"));
}

#[test]
fn gemini_running_lets_qwen_start_but_no_other_service_lane() {
    let r = running(&["gemini"]);
    assert!(may_start(&r, "qwen"));
    assert!(!may_start(&r, "chatgpt"));
    assert!(!may_start(&r, "claudeapi"));
    assert!(!may_start(&r, "gemini"));
}

#[test]
fn qwen_and_gemini_running_lets_no_lane_start() {
    let r = running(&["qwen", "gemini"]);
    for lane in ["qwen", "gemini", "chatgpt", "claudeapi", "deepseek"] {
        assert!(!may_start(&r, lane), "{lane} may not start");
    }
}

// ---- the command ----

#[test]
fn next_command_is_fleet_run_next_and_the_lane_with_no_lane_flag() {
    assert_eq!(
        next_command("gemini"),
        strings(&["fleet", "run", "--next", "gemini"])
    );
    assert_eq!(run_args("qwen"), strings(&["run", "--next", "qwen"]));
}

#[test]
fn the_fleet_lanes_are_the_pipelines_five() {
    assert_eq!(
        FLEET_LANES,
        ["chatgpt", "claudeapi", "deepseek", "gemini", "qwen"]
    );
}

// ---- the satellite file ----

#[test]
fn a_missing_satellite_file_is_off_with_the_default_lanes() {
    let dir = temp_dir("missing");
    assert_eq!(SATELLITE_FILE, "satellite.json");
    let config = load_config(&dir.join(SATELLITE_FILE)).unwrap();
    assert_eq!(config, SatelliteConfig::default());
    assert!(!config.satellite_on);
    assert_eq!(
        config.lanes,
        strings(&["qwen", "gemini", "chatgpt", "deepseek", "claudeapi"])
    );
}

#[test]
fn a_file_with_only_the_switch_keeps_the_default_lanes() {
    let dir = temp_dir("switch");
    let path = dir.join(SATELLITE_FILE);
    std::fs::write(&path, r#"{"satellite_on": true}"#).unwrap();
    let config = load_config(&path).unwrap();
    assert!(config.satellite_on);
    assert_eq!(config.lanes, SatelliteConfig::default().lanes);
}

#[test]
fn a_file_with_lanes_keeps_their_order() {
    let dir = temp_dir("lanes");
    let path = dir.join(SATELLITE_FILE);
    std::fs::write(
        &path,
        r#"{"satellite_on": true, "lanes": ["gemini", "qwen"]}"#,
    )
    .unwrap();
    assert_eq!(load_config(&path).unwrap(), on(&["gemini", "qwen"]));
}

#[test]
fn a_file_that_does_not_parse_is_a_parse_error_naming_it() {
    let dir = temp_dir("bad");
    let path = dir.join(SATELLITE_FILE);
    std::fs::write(&path, "{not json").unwrap();
    match load_config(&path) {
        Err(CoreError::Parse(why)) => assert!(why.contains("satellite.json"), "{why}"),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

// ---- tick ----

#[test]
fn off_starts_nothing_and_asks_nobody() {
    let mut runner = FakeRunner::with(&[]);
    let asked = RefCell::new(Vec::<String>::new());
    let spend = |lane: &str| {
        asked.borrow_mut().push(lane.to_string());
        Ok(true)
    };
    let config = SatelliteConfig::default();
    assert_eq!(tick(&config, &mut runner, &spend), Ok(vec![]));
    assert!(runner.started.is_empty());
    assert_eq!(runner.asked, 0);
    assert!(asked.borrow().is_empty());
}

#[test]
fn on_with_nothing_running_starts_qwen_and_one_service_lane() {
    let mut runner = FakeRunner::with(&[]);
    let asked = RefCell::new(Vec::<String>::new());
    let spend = |lane: &str| {
        asked.borrow_mut().push(lane.to_string());
        Ok(true)
    };
    let config = on(&["qwen", "gemini", "chatgpt", "deepseek", "claudeapi"]);
    assert_eq!(
        tick(&config, &mut runner, &spend),
        Ok(strings(&["qwen", "gemini"]))
    );
    assert_eq!(runner.started, strings(&["qwen", "gemini"]));
    // chatgpt, deepseek and claudeapi are refused by may_start, so the budget is not asked.
    assert_eq!(*asked.borrow(), strings(&["qwen", "gemini"]));
}

#[test]
fn a_braked_lane_is_skipped_and_the_next_one_starts() {
    let mut runner = FakeRunner::with(&[]);
    let spend = |lane: &str| Ok(lane != "gemini");
    let config = on(&["qwen", "gemini", "chatgpt"]);
    assert_eq!(
        tick(&config, &mut runner, &spend),
        Ok(strings(&["qwen", "chatgpt"]))
    );
}

#[test]
fn lanes_already_running_count() {
    let mut runner = FakeRunner::with(&["qwen"]);
    let spend = |_: &str| Ok(true);
    let config = on(&["qwen", "gemini", "chatgpt"]);
    assert_eq!(tick(&config, &mut runner, &spend), Ok(strings(&["gemini"])));
    assert_eq!(runner.asked, 1);

    let mut busy = FakeRunner::with(&["qwen", "deepseek"]);
    assert_eq!(tick(&config, &mut busy, &spend), Ok(vec![]));
}

#[test]
fn a_lane_the_fleet_does_not_know_is_skipped_without_asking() {
    let mut runner = FakeRunner::with(&[]);
    let asked = RefCell::new(Vec::<String>::new());
    let spend = |lane: &str| {
        asked.borrow_mut().push(lane.to_string());
        Ok(true)
    };
    let config = on(&["claude", "qwen"]);
    assert_eq!(tick(&config, &mut runner, &spend), Ok(strings(&["qwen"])));
    assert_eq!(*asked.borrow(), strings(&["qwen"]));
}

#[test]
fn a_budget_error_stops_the_tick_and_keeps_what_started() {
    let mut runner = FakeRunner::with(&[]);
    let spend = |lane: &str| {
        if lane == "gemini" {
            Err(CoreError::Refused("budget allow gemini: exit 3".into()))
        } else {
            Ok(true)
        }
    };
    let config = on(&["qwen", "gemini", "chatgpt"]);
    assert_eq!(
        tick(&config, &mut runner, &spend),
        Err(CoreError::Refused("budget allow gemini: exit 3".into()))
    );
    assert_eq!(runner.started, strings(&["qwen"]));
}

// ---- cards ----

const LOOP_OUTPUT: &str = r#"ran 4 job(s) on lane qwen: 1 done, 3 failed
{"id": "sb-247", "outcome": "done"}
{"id": "sb-248", "outcome": "failed", "failure_mode": "tests-red"}
not json at all
{"id": "sb-249", "outcome": "infra", "failure_mode": "rate-limit"}
[1, 2]
{"outcome": "timeout-partial"}
{"id": "sb-250", "outcome": "skipped"}
"#;

#[test]
fn cards_are_the_json_object_lines_in_order() {
    let all = cards(LOOP_OUTPUT);
    assert_eq!(all.len(), 5);
    assert_eq!(all[0], json!({"id": "sb-247", "outcome": "done"}));
    assert_eq!(all[4], json!({"id": "sb-250", "outcome": "skipped"}));
}

#[test]
fn failed_cards_are_failed_infra_and_timeout_partial() {
    let failed = failed_cards(LOOP_OUTPUT);
    let outcomes: Vec<&str> = failed
        .iter()
        .map(|c| c["outcome"].as_str().unwrap())
        .collect();
    assert_eq!(outcomes, ["failed", "infra", "timeout-partial"]);
    assert!(failed_cards("").is_empty());
}

/// Logs its arguments to calls.log, then answers a fleet class with the card's id.
const CLASSING_FLEET: &str = r#"import json, pathlib, sys
root = pathlib.Path(__file__).resolve().parent.parent.parent
with open(root / "calls.log", "a", encoding="utf-8") as fh:
    fh.write(" ".join(sys.argv[1:]) + "\n")
card = json.loads(pathlib.Path(sys.argv[4]).read_text(encoding="utf-8"))
print(json.dumps({"class": "fleet", "signature": card.get("id", "none")}))
"#;

#[test]
fn classify_failed_writes_each_failed_card_and_classes_it() {
    let root = temp_dir("classify");
    std::fs::create_dir_all(root.join("tools").join("fleet")).unwrap();
    std::fs::write(
        root.join("tools").join("fleet").join("fleet.py"),
        CLASSING_FLEET,
    )
    .unwrap();
    let fleet = FleetBridge::with_root(&root);
    let dir = root.join("cards");

    let classes = classify_failed(&fleet, &dir, LOOP_OUTPUT).unwrap();
    assert_eq!(
        classes,
        vec![
            (
                "sb-248".to_string(),
                FailureClass::Fleet {
                    signature: "sb-248".into()
                }
            ),
            (
                "sb-249".to_string(),
                FailureClass::Fleet {
                    signature: "sb-249".into()
                }
            ),
            (
                "card-3".to_string(),
                FailureClass::Fleet {
                    signature: "none".into()
                }
            ),
        ]
    );
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("sb-248.json")).unwrap()).unwrap();
    assert_eq!(written["failure_mode"], "tests-red");
    assert!(dir.join("card-3.json").exists());

    let log = std::fs::read_to_string(root.join("calls.log")).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(
        lines[0],
        format!(
            "errors classify --json {} --json",
            dir.join("sb-248.json").display()
        )
    );
}

#[test]
fn the_code_map_names_the_satellite_loop_present() {
    let map = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md"),
    )
    .unwrap();
    assert!(map.contains(
        "- AUTO-6 satellite loop (`tick`, `ChildRunner`, `classify_failed`; the app's satellite thread, switched on by `satellite.json`): present."
    ));
}
