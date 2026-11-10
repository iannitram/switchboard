//! CORE-1 (#338): a fleet call past its time limit is killed and is a fleet error.
//! CORE-2 (#351): the error text carries the exit code and stderr.

use std::path::PathBuf;
use std::time::{Duration, Instant};
use switchboard_core::fleet::{FleetBridge, FleetErrorReason, FLEET_TIMEOUT_SECS};
use switchboard_core::CoreError;

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fleet-stub")
}

#[test]
fn the_default_limit_is_two_minutes() {
    assert_eq!(FLEET_TIMEOUT_SECS, 120);
}

#[test]
fn a_slow_fleet_call_is_killed_at_the_limit() {
    let bridge = FleetBridge::with_root(&stub())
        .with_env("FLEET_STUB_SLEEP", "30")
        .with_timeout(Duration::from_millis(500));
    let started = Instant::now();
    let err = bridge.run_json(&["budget"]).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(err.reason, FleetErrorReason::Timeout(0));
    assert_eq!(err.exit_code, None);
    assert!(err.to_string().contains("was stopped"), "{err}");
}

#[test]
fn a_status_call_also_times_out() {
    let bridge = FleetBridge::with_root(&stub())
        .with_env("FLEET_STUB_SLEEP", "30")
        .with_timeout(Duration::from_millis(300));
    let err = bridge.run_status(&["budget", "allow", "x"]).unwrap_err();
    assert!(matches!(err.reason, FleetErrorReason::Timeout(_)));
}

#[test]
fn a_fast_call_is_not_affected() {
    let bridge = FleetBridge::with_root(&stub()).with_timeout(Duration::from_secs(30));
    assert!(bridge.run_json(&["budget"]).is_ok());
}

#[test]
fn the_error_text_has_the_exit_code_and_stderr() {
    let bridge = FleetBridge::with_root(&stub())
        .with_env("FLEET_STUB_EXIT", "3")
        .with_env("FLEET_STUB_STDERR", "boom from fleet\nsecond line\n");
    let text = CoreError::Fleet(bridge.run_json(&["budget"]).unwrap_err()).to_string();
    assert!(text.starts_with("fleet error: "), "{text}");
    assert!(text.contains("exited 3"), "{text}");
    assert!(text.contains("boom from fleet | second line"), "{text}");
}

#[test]
fn the_error_text_says_when_the_output_was_not_json() {
    let bridge = FleetBridge::with_root(&stub()).with_env("FLEET_STUB_BADJSON", "1");
    let text = CoreError::Fleet(bridge.run_json(&["budget"]).unwrap_err()).to_string();
    assert!(text.contains("not JSON"), "{text}");
}

#[test]
fn a_missing_script_shows_the_interpreters_complaint() {
    let bridge = FleetBridge::with_root(&std::env::temp_dir().join("sb-no-such-pipeline"));
    let text = CoreError::Fleet(bridge.run_json(&["budget"]).unwrap_err()).to_string();
    assert!(
        text.contains("exited 2") && text.contains("fleet.py"),
        "{text}"
    );
}

#[test]
fn a_program_that_cannot_start_says_so() {
    let bridge = FleetBridge::with_root(&stub()).with_python("/no/such/python", &[]);
    let text = CoreError::Fleet(bridge.run_json(&["budget"]).unwrap_err()).to_string();
    assert!(text.contains("could not start"), "{text}");
}
