use serde_json::Value;
use std::path::PathBuf;
use switchboard_core::runner::spec::{self, Actor};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn document() -> Value {
    serde_json::from_str(include_str!("../../../specs/roadmap-run.json")).unwrap()
}

#[test]
fn both_repo_specs_load_into_typed_records() {
    for name in ["roadmap-run", "bug-loop"] {
        let loaded = spec::load(&root().join("specs").join(format!("{name}.json"))).unwrap();
        assert_eq!(loaded.id, name);
        assert!(!loaded.records[0].fields[0].name.is_empty());
        assert!(!loaded.states[0].terminal);
        assert!(!loaded.scorers[0].labels.is_empty());
        assert!(!loaded.invariants[0].statement.is_empty());
        assert_eq!(loaded.validate(), Vec::<String>::new());
        let encoded = serde_json::to_value(&loaded).unwrap();
        let source: Value = serde_json::from_str(
            &std::fs::read_to_string(root().join("specs").join(format!("{name}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(encoded, source);
    }
    let loaded = spec::parse(&document().to_string()).unwrap();
    assert_eq!(loaded.steps[0].actor, Actor::Human);
}

#[test]
fn every_required_step_key_is_refused_with_the_step_id_and_key() {
    let original = document();
    for key in original["steps"][1].as_object().unwrap().keys() {
        let mut broken = original.clone();
        broken["steps"][1].as_object_mut().unwrap().remove(key);
        let error = spec::parse(&broken.to_string()).unwrap_err().to_string();
        assert!(error.contains(key), "{key}: {error}");
        // A missing id is identified by its position instead.
        let step = if key == "id" {
            "steps[1]"
        } else {
            "build-roadmap"
        };
        assert!(error.contains(step), "{key}: {error}");
    }
}

#[test]
fn each_unknown_state_reference_names_the_step_and_key() {
    for key in ["from_state", "to_state_ok", "to_state_fail"] {
        let mut broken = document();
        broken["steps"][1][key] = "no-such-state".into();
        let error = spec::parse(&broken.to_string()).unwrap_err().to_string();
        for expected in ["build-roadmap", key, "no-such-state"] {
            assert!(error.contains(expected), "{error}");
        }
    }
}

#[test]
fn an_unknown_actor_names_the_step_and_key() {
    let mut broken = document();
    broken["steps"][1]["actor"] = "no-such-actor".into();
    let error = spec::parse(&broken.to_string()).unwrap_err().to_string();
    for expected in ["build-roadmap", "actor", "no-such-actor"] {
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn malformed_nested_step_data_keeps_step_context() {
    let mut broken = document();
    broken["steps"][1]["run"]["exists"] = "yes".into();
    let error = spec::parse(&broken.to_string()).unwrap_err().to_string();
    assert!(error.contains("build-roadmap"), "{error}");
    assert!(error.contains("run"), "{error}");
}

#[test]
fn bad_json_and_unreadable_files_are_core_errors() {
    assert!(spec::parse("{").is_err());
    assert!(spec::parse("{}").is_err());
    assert!(spec::load(&root().join("specs/no-such-spec.json")).is_err());
}
