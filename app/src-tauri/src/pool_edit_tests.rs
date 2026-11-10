// #340 on the app face: the Budget panel's save is all or nothing, Waiting item included.
use super::*;
use switchboard_core::clock::FixedClock;
use switchboard_core::waiting::LocalItems;

const CONFIG: &str = r#"{
  "version": 1,
  "pools": {
    "deepseek": { "label": "DeepSeek", "kind": "fixed-period", "cap_usd": 10 }
  }
}
"#;

/// A pipeline over the core's fleet stub and an empty data folder, under a fresh folder.
fn dirs(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("switchboard-340-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("pipeline");
    let stub =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/core/tests/fixtures/fleet-stub");
    for sub in ["tools/fleet", "answers"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    for file in ["tools/fleet/fleet.py", "answers/budget.json"] {
        std::fs::copy(stub.join(file), root.join(file)).unwrap();
    }
    std::fs::write(root.join("tools/fleet/config.json"), CONFIG).unwrap();
    (root, dir.join("data"))
}

fn cap(cap: f64) -> PoolEdit {
    PoolEdit {
        pool: "deepseek".to_string(),
        cap: Some(cap),
        ..PoolEdit::default()
    }
}

#[test]
fn a_save_lands_the_file_the_ledger_row_and_the_waiting_item() {
    let (root, data) = dirs("saved");
    let rows = save_pool_edit_rows(
        &FleetBridge::with_root(&root),
        &cap(25.0),
        &data,
        &FixedClock::at(7),
    )
    .unwrap();
    assert!(!rows.is_empty());
    let config = std::fs::read_to_string(root.join("tools/fleet/config.json")).unwrap();
    assert!(config.contains("\"cap_usd\": 25.0"), "{config}");
    assert!(data.join(budget::POOL_EDITS_LOG).is_file());
    assert_eq!(LocalItems::new(&data).load().unwrap().len(), 1);
}

#[test]
fn a_waiting_item_that_cannot_be_added_leaves_file_and_ledger_as_they_were() {
    let (root, data) = dirs("no-waiting");
    // `waiting/local.json` is a folder, so the item cannot be written.
    std::fs::create_dir_all(LocalItems::new(&data).path()).unwrap();
    let err = save_pool_edit_rows(
        &FleetBridge::with_root(&root),
        &cap(25.0),
        &data,
        &FixedClock::at(7),
    );
    assert!(err.is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("tools/fleet/config.json")).unwrap(),
        CONFIG
    );
    assert!(!data.join(budget::POOL_EDITS_LOG).exists());
}
