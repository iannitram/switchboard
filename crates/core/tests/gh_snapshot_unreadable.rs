//! #350 (GH-1): one snapshot file that does not read hides no other repo, is named, and the
//! next read replaces it.

use serde_json::json;
use std::path::{Path, PathBuf};
use switchboard_core::clock::FixedClock;
use switchboard_core::github::live::{refresh_all, repos, tick};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::Recorded;
use switchboard_core::github::RepoSnapshot;
use switchboard_core::sky::model;

const T0: i64 = 1_800_000_000;

fn data_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!(
        "sb-unreadable-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A good `owner/alpha` and an `owner/beta` file holding `{trunc`.
fn seeded(tag: &str) -> (PathBuf, PathBuf) {
    let data = data_dir(tag);
    let store = SnapshotStore::new(&data);
    store
        .save(&RepoSnapshot {
            repo: "owner/alpha".to_string(),
            read_at: T0,
            ..Default::default()
        })
        .unwrap();
    let bad = store.path_for("owner/beta");
    std::fs::write(&bad, "{trunc").unwrap();
    (data, bad)
}

fn repo_names(snaps: &[RepoSnapshot]) -> Vec<&str> {
    snaps.iter().map(|s| s.repo.as_str()).collect()
}

#[test]
fn load_all_keeps_the_good_snapshots() {
    let (data, _) = seeded("all");
    let snaps = SnapshotStore::new(&data).load_all().unwrap();
    assert_eq!(repo_names(&snaps), vec!["owner/alpha"]);
}

#[test]
fn load_each_names_the_file_that_does_not_read() {
    let (data, bad) = seeded("each");
    let (snaps, unreadable) = SnapshotStore::new(&data).load_each().unwrap();
    assert_eq!(repo_names(&snaps), vec!["owner/alpha"]);
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].repo, "owner/beta");
    assert_eq!(unreadable[0].path, bad);
    assert!(
        unreadable[0].error.contains(&bad.display().to_string()),
        "{}",
        unreadable[0].error
    );
}

#[test]
fn the_sky_shows_the_good_repo() {
    let (data, _) = seeded("sky");
    let sky = model::load(&data, T0).unwrap();
    let names: Vec<&str> = sky.constellations.iter().map(|c| c.repo.as_str()).collect();
    assert_eq!(names, vec!["owner/alpha"]);
}

#[test]
fn the_unreadable_repo_is_still_one_to_read() {
    let (data, _) = seeded("repos");
    assert_eq!(repos(&data).unwrap(), vec!["owner/alpha", "owner/beta"]);
}

fn readable(data: &Path, repo: &str) -> RepoSnapshot {
    SnapshotStore::new(data).load(repo).unwrap().unwrap()
}

#[test]
fn refresh_replaces_the_file_that_does_not_read() {
    let (data, _) = seeded("refresh");
    let answer = json!({ "repository": { "labels": { "nodes": [ { "name": "fresh" } ] } } });
    let t = Recorded::new(vec![answer.clone(), answer], vec![]);
    let read = refresh_all(&t, &data, &FixedClock::at(T0 + 60)).unwrap();
    assert_eq!(read, vec!["owner/alpha", "owner/beta"]);
    assert_eq!(readable(&data, "owner/beta").labels, vec!["fresh"]);
}

#[test]
fn the_poll_reads_an_unreadable_snapshot_as_missing() {
    let (data, _) = seeded("tick");
    let answer = json!({ "repository": { "labels": { "nodes": [ { "name": "fresh" } ] } } });
    let t = Recorded::new(vec![answer], vec![]);
    // alpha was read at T0, so only beta is due.
    let read = tick(&t, &data, &FixedClock::at(T0 + 60)).unwrap();
    assert_eq!(read, vec!["owner/beta"]);
    assert_eq!(readable(&data, "owner/beta").labels, vec!["fresh"]);
}
