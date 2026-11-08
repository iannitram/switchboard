//! AUTO-9: a job's failure is classed by the fleet's answer to `fleet errors classify --json`,
//! never by the exit code. Tests use inline fleet stubs in temp roots (as budget_rows does).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use switchboard_core::autopilot::classify::{classify, from_json, FailureClass};
use switchboard_core::fleet::{FleetBridge, FleetError, FleetErrorReason};
use switchboard_core::CoreError;

/// Logs its arguments to calls.log, then prints answers/errors.json.
const ANSWERING_FLEET: &str = r#"import pathlib, sys
root = pathlib.Path(__file__).resolve().parent.parent.parent
with open(root / "calls.log", "a", encoding="utf-8") as fh:
    fh.write(" ".join(sys.argv[1:]) + "\n")
if sys.argv[1:2] != ["errors"]:
    sys.stderr.write("stub: only errors\n")
    sys.exit(2)
sys.stdout.write((root / "answers" / "errors.json").read_text(encoding="utf-8"))
"#;

/// Exits 3 with a message on stderr, as a fleet run that failed.
const FAILING_FLEET: &str = r#"import sys
sys.stderr.write("classify broke\n")
sys.exit(3)
"#;

fn stub_root(name: &str, script: &str, answer: Option<&Value>) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sb-auto9-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tools").join("fleet")).unwrap();
    std::fs::create_dir_all(root.join("answers")).unwrap();
    std::fs::write(root.join("tools").join("fleet").join("fleet.py"), script).unwrap();
    if let Some(answer) = answer {
        std::fs::write(root.join("answers").join("errors.json"), answer.to_string()).unwrap();
    }
    root
}

fn card() -> &'static Path {
    Path::new("/srv/pipeline/cards/done/42.json")
}

#[test]
fn a_fleet_answer_with_a_signature_is_fleet() {
    let v = json!({"class": "fleet", "mode": "rate-limit", "why": "infra card: rate-limit", "signature": "rate-limit@-@-"});
    assert_eq!(
        from_json(&v),
        Ok(FailureClass::Fleet {
            signature: "rate-limit@-@-".into()
        })
    );
}

#[test]
fn none_is_passed_whatever_the_mode() {
    assert_eq!(
        from_json(&json!({"class": "none", "mode": null, "why": "outcome done"})),
        Ok(FailureClass::Passed)
    );
}

#[test]
fn coding_and_author_map_to_their_classes() {
    assert_eq!(
        from_json(&json!({"class": "coding", "mode": "tests-red", "why": "x"})),
        Ok(FailureClass::Coding)
    );
    assert_eq!(
        from_json(&json!({"class": "author", "mode": null, "why": "x"})),
        Ok(FailureClass::Author)
    );
}

#[test]
fn a_fleet_answer_without_a_signature_is_an_error() {
    let v = json!({"class": "fleet", "mode": "rate-limit", "why": "x"});
    assert!(matches!(from_json(&v), Err(CoreError::Parse(_))));
}

#[test]
fn a_fleet_answer_whose_signature_is_not_a_string_is_an_error() {
    let v = json!({"class": "fleet", "mode": "rate-limit", "why": "x", "signature": 7});
    assert!(matches!(from_json(&v), Err(CoreError::Parse(_))));
}

#[test]
fn an_unknown_class_is_an_error() {
    let v = json!({"class": "maybe", "mode": null, "why": "x"});
    assert!(matches!(from_json(&v), Err(CoreError::Parse(_))));
}

#[test]
fn a_missing_class_is_an_error() {
    assert!(matches!(
        from_json(&json!({"mode": null, "why": "x"})),
        Err(CoreError::Parse(_))
    ));
}

#[test]
fn a_value_that_is_not_an_object_is_an_error() {
    assert!(matches!(
        from_json(&json!("fleet")),
        Err(CoreError::Parse(_))
    ));
    assert!(matches!(
        from_json(&json!(["none"])),
        Err(CoreError::Parse(_))
    ));
    assert!(matches!(from_json(&Value::Null), Err(CoreError::Parse(_))));
}

#[test]
fn classify_returns_the_class_the_fleet_stub_answers() {
    let answer = json!({"class": "fleet", "mode": "rate-limit", "why": "infra card: rate-limit", "signature": "rate-limit@-@-"});
    let root = stub_root("answer", ANSWERING_FLEET, Some(&answer));
    let fleet = FleetBridge::with_root(&root);
    assert_eq!(
        classify(&fleet, card()),
        Ok(FailureClass::Fleet {
            signature: "rate-limit@-@-".into()
        })
    );
    let calls = std::fs::read_to_string(root.join("calls.log")).unwrap();
    assert!(calls.contains("errors classify"), "{calls}");
    assert!(
        calls.contains("/srv/pipeline/cards/done/42.json"),
        "{calls}"
    );
}

#[test]
fn classify_passes_none_as_passed() {
    let answer = json!({"class": "none", "mode": null, "why": "outcome done"});
    let root = stub_root("none", ANSWERING_FLEET, Some(&answer));
    assert_eq!(
        classify(&FleetBridge::with_root(&root), card()),
        Ok(FailureClass::Passed)
    );
}

#[test]
fn classify_with_a_stub_that_exits_non_zero_is_a_fleet_error() {
    let root = stub_root("exit", FAILING_FLEET, None);
    let result = classify(&FleetBridge::with_root(&root), card());
    match result {
        Err(CoreError::Fleet(FleetError {
            exit_code,
            stderr,
            reason,
            ..
        })) => {
            assert_eq!(exit_code, Some(3));
            assert_eq!(stderr, "classify broke\n");
            assert_eq!(reason, FleetErrorReason::Exit);
        }
        other => panic!("expected a fleet error, got {other:?}"),
    }
}

#[test]
fn classify_with_a_bad_answer_is_an_error_not_a_class() {
    let answer = json!({"class": "fleet", "mode": "rate-limit", "why": "x"});
    let root = stub_root("nosig", ANSWERING_FLEET, Some(&answer));
    assert!(matches!(
        classify(&FleetBridge::with_root(&root), card()),
        Err(CoreError::Parse(_))
    ));
}
