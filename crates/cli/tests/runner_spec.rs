use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn validate(path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(["spec", "validate"])
        .arg(path)
        .arg("--json")
        .output()
        .unwrap()
}

#[test]
fn valid_specs_print_json_and_exit_zero() {
    for name in ["roadmap-run", "bug-loop"] {
        let output = validate(&root().join("specs").join(format!("{name}.json")));
        assert!(output.status.success(), "{output:?}");
        let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(answer["errors"], serde_json::json!([]));
        assert!(output.stderr.is_empty(), "{output:?}");
    }
}

#[test]
fn invalid_specs_print_errors_and_exit_nonzero() {
    let folder = root()
        .join("target")
        .join(format!("runner-spec-cli-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("invalid.json");
    let original: Value =
        serde_json::from_str(include_str!("../../../specs/roadmap-run.json")).unwrap();
    for key in ["run", "from_state", "to_state_ok", "to_state_fail", "actor"] {
        let mut broken = original.clone();
        if key == "run" {
            broken["steps"][1].as_object_mut().unwrap().remove(key);
        } else {
            broken["steps"][1][key] = "unknown".into();
        }
        std::fs::write(&path, broken.to_string()).unwrap();
        let output = validate(&path);
        assert!(!output.status.success(), "{output:?}");
        let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
        let errors = answer["errors"].as_array().unwrap();
        assert!(!errors.is_empty(), "{answer}");
        let errors = errors.iter().map(Value::to_string).collect::<String>();
        assert!(errors.contains("build-roadmap"), "{errors}");
        assert!(errors.contains(key), "{errors}");
    }
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&folder).unwrap();
}

#[test]
fn unreadable_and_malformed_files_also_print_json_errors() {
    let output = validate(&root().join("specs/no-such-spec.json"));
    assert!(!output.status.success());
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!answer["errors"].as_array().unwrap().is_empty());
    let output = validate(&root().join("Cargo.toml"));
    assert!(!output.status.success());
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!answer["errors"].as_array().unwrap().is_empty());
}

#[test]
fn spec_validate_requires_exactly_one_file() {
    for words in [
        vec!["spec"],
        vec!["spec", "validate"],
        vec!["spec", "validate", "a", "b"],
        vec!["spec", "other", "a"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_switchboard"))
            .args(words)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}
