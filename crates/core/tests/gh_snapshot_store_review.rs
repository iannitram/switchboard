//! R25 review gap test for #14: only a missing snapshot folder is an empty load_all.
//! Kept apart from the locked file gh_snapshot_store.rs (R28).

use std::path::PathBuf;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::CoreError;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-review-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn load_all_is_an_io_error_when_the_snapshot_path_is_not_a_folder() {
    let data = temp_dir("notdir");
    std::fs::write(data.join("snapshot"), "x").unwrap();
    let store = SnapshotStore::new(&data);
    assert!(matches!(store.load_all(), Err(CoreError::Io(_))));
}
