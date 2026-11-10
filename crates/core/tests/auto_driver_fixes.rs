//! AUTO-6, AUTO-9 fixes: a failed card's id never names a file outside the cards folder (#382),
//! and a satellite loop that ends with an error is recorded with its exit code and stderr and
//! holds its lane instead of being started again every tick (#383). The loops run a stub
//! `fleet.py` in a temp root; no real fleet job runs.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use switchboard_core::autopilot::driver::{
    card_file_name, classify_failed, tick, ChildRunner, LaneRunner, LoopEnd, SatelliteConfig,
};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::CoreError;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-auto6-fix-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn stub_fleet(root: &Path, script: &str) -> FleetBridge {
    std::fs::create_dir_all(root.join("tools").join("fleet")).unwrap();
    std::fs::write(root.join("tools").join("fleet").join("fleet.py"), script).unwrap();
    FleetBridge::with_root(root)
}

/// Answers a fleet class with the card's id, from the card file it is given.
const CLASSING_FLEET: &str = r#"import json, pathlib, sys
card = json.loads(pathlib.Path(sys.argv[4]).read_text(encoding="utf-8"))
print(json.dumps({"class": "fleet", "signature": card.get("id", "none")}))
"#;

// --- #382: the card's file name ---

#[test]
fn a_card_id_becomes_one_file_name() {
    assert_eq!(card_file_name(Some("sb-248"), 1), "sb-248.json");
    assert_eq!(card_file_name(Some("../escaped"), 1), "..-escaped.json");
    assert_eq!(card_file_name(Some("/tmp/abs"), 2), "-tmp-abs.json");
    assert_eq!(card_file_name(Some("a/b"), 3), "a-b.json");
    assert_eq!(card_file_name(Some("a\\b"), 3), "a-b.json");
    assert_eq!(card_file_name(Some(".."), 4), "card-4.json");
    assert_eq!(card_file_name(Some(""), 5), "card-5.json");
    assert_eq!(card_file_name(None, 6), "card-6.json");
}

#[test]
fn an_id_with_a_path_in_it_is_written_inside_the_cards_folder() {
    let root = temp_dir("escape");
    let fleet = stub_fleet(&root, CLASSING_FLEET);
    let dir = root.join("cards");
    let abs = root.join("abs-target");
    let output = format!(
        "{}\n{}\n{}\n",
        r#"{"id":"../escaped","outcome":"failed"}"#,
        serde_json::json!({"id": abs.display().to_string(), "outcome": "failed"}),
        r#"{"id":"a/b","outcome":"failed"}"#,
    );
    let classes = classify_failed(&fleet, &dir, &output).unwrap();
    // Every card is classed (a/b no longer stops the ones after it), under its own id.
    let ids: Vec<&str> = classes.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["../escaped", abs.to_str().unwrap(), "a/b"],
        "{classes:?}"
    );
    assert!(!root.join("escaped.json").exists());
    assert!(!root.join("abs-target.json").exists());
    assert!(!abs.with_extension("json").exists());
    let mut written: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    written.sort();
    assert_eq!(written.len(), 3, "{written:?}");
    assert!(written.contains(&"..-escaped.json".to_string()));
    assert!(written.contains(&"a-b.json".to_string()));
}

// --- #383: a loop that ends with an error ---

/// `fleet run --next <lane>` that says why on stderr and exits 1 at once.
const DYING_FLEET: &str = r#"import sys
sys.stderr.write("no queue for lane " + sys.argv[sys.argv.index("--next") + 1] + "\n")
sys.exit(1)
"#;

/// `fleet run --next <lane>` that prints one card and exits 0.
const QUIET_FLEET: &str = r#"import json
print(json.dumps({"id": "sb-1", "outcome": "done"}))
"#;

fn gemini_on() -> SatelliteConfig {
    SatelliteConfig {
        satellite_on: true,
        lanes: vec!["gemini".to_string()],
    }
}

/// Wait (up to 20 s) until a loop has ended, and answer what ended.
fn wait_for_end(runner: &mut ChildRunner) -> Vec<LoopEnd> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if runner.running().is_empty() {
            return runner.take_finished();
        }
        assert!(Instant::now() < deadline, "the stub loop did not end");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_loop_that_dies_is_recorded_and_not_started_again_at_once() {
    let root = temp_dir("dying");
    let fleet = stub_fleet(&root, DYING_FLEET);
    let mut runner = ChildRunner::new(fleet, &root.join("satellite"));
    let spend = |_: &str| -> Result<bool, CoreError> { Ok(true) };

    assert_eq!(tick(&gemini_on(), &mut runner, &spend).unwrap(), ["gemini"]);
    let ended = wait_for_end(&mut runner);
    assert_eq!(ended.len(), 1);
    let end = &ended[0];
    assert_eq!(end.lane, "gemini");
    assert_eq!(end.exit_code, Some(1));
    assert!(end.stderr.contains("no queue for lane gemini"), "{end:?}");
    assert!(end.failed());
    match end.fleet_error() {
        Some(CoreError::Fleet(e)) => {
            assert_eq!(e.exit_code, Some(1));
            assert!(e.stderr.contains("no queue for lane gemini"));
            assert!(e.command.iter().any(|a| a == "--next"), "{:?}", e.command);
            assert!(e.command.iter().any(|a| a == "gemini"), "{:?}", e.command);
        }
        other => panic!("expected a fleet error, got {other:?}"),
    }

    // The next two ticks start nothing: the lane is held.
    assert_eq!(runner.held(), ["gemini"]);
    for _ in 0..2 {
        assert!(tick(&gemini_on(), &mut runner, &spend).unwrap().is_empty());
    }
}

#[test]
fn a_held_lane_starts_again_once_the_hold_is_over() {
    let root = temp_dir("hold-over");
    let fleet = stub_fleet(&root, DYING_FLEET);
    let mut runner = ChildRunner::with_hold(fleet, &root.join("satellite"), Duration::ZERO);
    let spend = |_: &str| -> Result<bool, CoreError> { Ok(true) };
    assert_eq!(tick(&gemini_on(), &mut runner, &spend).unwrap(), ["gemini"]);
    wait_for_end(&mut runner);
    assert!(runner.held().is_empty());
    assert_eq!(tick(&gemini_on(), &mut runner, &spend).unwrap(), ["gemini"]);
    wait_for_end(&mut runner);
}

#[test]
fn a_loop_that_exits_0_is_no_error_and_holds_nothing() {
    let root = temp_dir("quiet");
    let fleet = stub_fleet(&root, QUIET_FLEET);
    let mut runner = ChildRunner::new(fleet, &root.join("satellite"));
    let spend = |_: &str| -> Result<bool, CoreError> { Ok(true) };
    assert_eq!(tick(&gemini_on(), &mut runner, &spend).unwrap(), ["gemini"]);
    let ended = wait_for_end(&mut runner);
    assert_eq!(ended[0].exit_code, Some(0));
    assert!(ended[0].stdout.contains("sb-1"));
    assert_eq!(ended[0].fleet_error(), None);
    assert!(runner.held().is_empty());
    assert_eq!(tick(&gemini_on(), &mut runner, &spend).unwrap(), ["gemini"]);
    wait_for_end(&mut runner);
}
