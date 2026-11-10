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

#[test]
fn a_child_that_fills_both_pipes_and_exits_does_not_deadlock() {
    // 4 MB each on stderr and stdout, far past a pipe's buffer; the call must finish and say
    // the output is not JSON (the flood on stdout), not hang.
    let bridge = FleetBridge::with_root(&stub())
        .with_env("FLEET_STUB_FLOOD", "4000000")
        .with_env("FLEET_STUB_FLOOD_STDOUT", "1")
        .with_timeout(Duration::from_secs(60));
    let started = Instant::now();
    let err = bridge.run_json(&["budget"]).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(matches!(err.reason, FleetErrorReason::BadJson(_)), "{err}");
}

#[test]
fn a_child_that_fills_the_pipe_and_then_hangs_is_killed_with_what_it_printed() {
    let bridge = FleetBridge::with_root(&stub())
        .with_env("FLEET_STUB_FLOOD", "4000000")
        .with_env("FLEET_STUB_SLEEP", "30")
        .with_timeout(Duration::from_millis(1500));
    let started = Instant::now();
    let err = bridge.run_json(&["budget"]).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(matches!(err.reason, FleetErrorReason::Timeout(_)), "{err}");
    assert!(err.stderr.len() >= 3_000_000, "{}", err.stderr.len());
}

#[cfg(unix)]
#[test]
fn the_timeout_also_kills_what_the_child_started() {
    let pidfile = std::env::temp_dir().join(format!("sb-fleet-grandchild-{}", std::process::id()));
    let _ = std::fs::remove_file(&pidfile);
    let bridge = FleetBridge::with_root(&stub())
        .with_env("FLEET_STUB_GRANDCHILD", pidfile.to_str().unwrap())
        .with_env("FLEET_STUB_SLEEP", "30")
        .with_timeout(Duration::from_secs(2));
    let err = bridge.run_json(&["budget"]).unwrap_err();
    assert!(matches!(err.reason, FleetErrorReason::Timeout(_)), "{err}");
    let pid = std::fs::read_to_string(&pidfile).unwrap();
    let _ = std::fs::remove_file(&pidfile);
    // Gone, or a zombie waiting for its parent (not running).
    std::thread::sleep(Duration::from_millis(300));
    let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid.trim()));
    let running = stat.map(|s| !s.contains(") Z")).unwrap_or(false);
    assert!(!running, "the grandchild {pid} is still running");
}

#[cfg(unix)]
#[test]
fn a_grandchild_holding_the_pipe_after_the_child_exits_does_not_hang_the_call() {
    let pidfile = std::env::temp_dir().join(format!("sb-fleet-holder-{}", std::process::id()));
    let _ = std::fs::remove_file(&pidfile);
    let bridge = FleetBridge::with_root(&stub())
        .with_env("FLEET_STUB_GRANDCHILD", pidfile.to_str().unwrap())
        .with_timeout(Duration::from_secs(2));
    let started = Instant::now();
    let _ = bridge.run_json(&["budget"]);
    assert!(started.elapsed() < Duration::from_secs(15));
    if let Ok(pid) = std::fs::read_to_string(&pidfile) {
        let _ = std::process::Command::new("kill").arg(pid.trim()).status();
    }
    let _ = std::fs::remove_file(&pidfile);
}
