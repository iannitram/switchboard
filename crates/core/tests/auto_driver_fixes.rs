//! AUTO-9 fix: a failed card's id never names a file outside the cards folder (#382). The
//! classing runs a stub `fleet.py` in a temp root; no real fleet job runs.

use std::path::{Path, PathBuf};
use switchboard_core::autopilot::driver::{card_file_name, classify_failed};
use switchboard_core::fleet::FleetBridge;

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
