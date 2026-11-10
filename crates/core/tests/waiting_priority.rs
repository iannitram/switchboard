//! #344 (WAIT-1, SKY-9): a stale priority copy in the app data folder is a Waiting on you item.

use std::path::{Path, PathBuf};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::RepoSnapshot;
use switchboard_core::waiting::{load_at, priority_items, Source};

/// 2026-11-10T00:00:00Z
const NOW: i64 = 1_794_268_800;

fn data_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-waitprio-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_copy(data: &Path, text: &str) {
    std::fs::write(data.join("priority.json"), text).unwrap();
}

#[test]
fn no_copy_is_no_item() {
    assert!(priority_items(&data_dir("none"), NOW).is_empty());
}

#[test]
fn a_fresh_copy_is_no_item() {
    let data = data_dir("fresh");
    write_copy(
        &data,
        r#"{"updated_at":"2026-11-08T00:00:00Z","order":["owner/alpha"]}"#,
    );
    assert!(priority_items(&data, NOW).is_empty());
}

#[test]
fn a_copy_forty_days_old_is_an_item_and_load_lists_it() {
    let data = data_dir("stale");
    write_copy(
        &data,
        r#"{"updated_at":"2026-10-01T00:00:00Z","order":["owner/alpha"]}"#,
    );
    let items = priority_items(&data, NOW);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].source, Source::StalePriority);
    assert_eq!(items[0].what, "Refresh the priority copy");
    assert!(items[0].why.contains("40 days old"), "{}", items[0].why);
    assert_eq!(
        items[0].link,
        data.join("priority.json").display().to_string()
    );

    SnapshotStore::new(&data)
        .save(&RepoSnapshot {
            repo: "owner/alpha".to_string(),
            ..Default::default()
        })
        .unwrap();
    let listed = load_at(&data, NOW).unwrap();
    assert_eq!(
        listed.iter().map(|i| i.source.clone()).collect::<Vec<_>>(),
        vec![Source::StalePriority]
    );
}

#[test]
fn a_copy_that_does_not_read_is_an_item_too() {
    let data = data_dir("bad");
    write_copy(&data, "{trunc");
    let items = priority_items(&data, NOW);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].source, Source::StalePriority);
    assert!(items[0].why.contains("does not read"), "{}", items[0].why);
}
