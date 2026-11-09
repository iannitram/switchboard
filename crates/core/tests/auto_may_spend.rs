//! AUTO-1: `may_spend` asks `fleet budget allow <lane>` and obeys it: exit 0 is `true`, exit 75
//! is `false`, any other exit is a `FleetError` and never `true`. An empty lane is an error
//! without a call. The fixture stub answers with `FLEET_STUB_EXIT`; the empty-lane test uses
//! an inline logging stub in a temp root, as auto_classify does, to prove nothing was called.

use std::path::{Path, PathBuf};
use switchboard_core::autopilot::may_spend;
use switchboard_core::fleet::{FleetBridge, FleetErrorReason};
use switchboard_core::CoreError;

/// Logs its arguments to calls.log, then exits 0.
const LOGGING_FLEET: &str = r#"import pathlib, sys
root = pathlib.Path(__file__).resolve().parent.parent.parent
with open(root / "calls.log", "a", encoding="utf-8") as fh:
    fh.write(" ".join(sys.argv[1:]) + "\n")
"#;

fn fixture_stub() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fleet-stub")
}

fn logging_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sb-auto1-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tools").join("fleet")).unwrap();
    std::fs::write(
        root.join("tools").join("fleet").join("fleet.py"),
        LOGGING_FLEET,
    )
    .unwrap();
    root
}

fn stub_says(code: &str) -> FleetBridge {
    let b = FleetBridge::with_root(&fixture_stub());
    if code == "0" {
        b
    } else {
        b.with_env("FLEET_STUB_EXIT", code)
    }
}

#[test]
fn a_stub_that_answers_0_may_spend() {
    assert_eq!(may_spend(&stub_says("0"), "claude"), Ok(true));
}

#[test]
fn a_stub_that_answers_75_may_not_spend() {
    assert_eq!(may_spend(&stub_says("75"), "claude"), Ok(false));
}

#[test]
fn a_stub_that_exits_1_is_an_error_never_true() {
    match may_spend(&stub_says("1"), "claude") {
        Err(CoreError::Fleet(err)) => {
            assert_eq!(err.exit_code, Some(1));
            assert_eq!(err.reason, FleetErrorReason::Exit);
            assert!(err.command.contains(&"budget".to_string()), "{err:?}");
            assert!(err.command.contains(&"claude".to_string()), "{err:?}");
        }
        other => panic!("expected a fleet error, got {other:?}"),
    }
}

#[test]
fn an_empty_lane_is_an_error_and_the_stub_is_not_called() {
    let root = logging_root("empty");
    let result = may_spend(&FleetBridge::with_root(&root), "");
    assert!(matches!(result, Err(CoreError::Refused(_))), "{result:?}");
    assert!(
        !root.join("calls.log").exists(),
        "the stub was called for an empty lane"
    );
}

#[test]
fn the_lane_is_passed_to_budget_allow() {
    let root = logging_root("lane");
    assert_eq!(
        may_spend(&FleetBridge::with_root(&root), "claude"),
        Ok(true)
    );
    let calls = std::fs::read_to_string(root.join("calls.log")).unwrap();
    assert!(calls.contains("budget allow claude"), "{calls}");
}
