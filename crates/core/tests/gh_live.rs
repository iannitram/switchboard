//! Locked tests for #240 (GH-1, GH-3 in the app: the poll's repos, the tick, the Refresh
//! button and refresh-on-action over the app data folder). Qwen never edits this file (R28).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use switchboard_core::clock::FixedClock;
use switchboard_core::github::live::{
    after_write, refresh_all, repos, tick, SNAPSHOT_EVENT, TICK_EVERY_SECS,
};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{Recorded, RestAnswer};
use switchboard_core::github::RepoSnapshot;
use switchboard_core::CoreError;

const T0: i64 = 1_800_000_000;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-live-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The priority copy in the app data folder, with this order.
fn write_priority(data: &Path, order: &[&str]) {
    let text = json!({ "updated_at": "2026-11-09T12:00:00Z", "order": order }).to_string();
    std::fs::write(data.join("priority.json"), text).unwrap();
}

/// A stored snapshot of `repo`, read at `read_at`.
fn store_snapshot(data: &Path, repo: &str, read_at: i64) {
    SnapshotStore::new(data)
        .save(&RepoSnapshot {
            repo: repo.to_string(),
            read_at,
            ..Default::default()
        })
        .unwrap();
}

/// `n` recorded GraphQL answers, each a tiny repository whose one label is `read-<i>`.
fn answers(n: usize) -> Vec<Value> {
    (1..=n)
        .map(|i| json!({ "repository": { "labels": { "nodes": [ { "name": format!("read-{i}") } ] } } }))
        .collect()
}

fn changed(etag: &str) -> RestAnswer {
    RestAnswer {
        status: 200,
        etag: Some(etag.to_string()),
        body: json!([]),
    }
}

fn log(t: &Recorded) -> Vec<String> {
    t.log.lock().unwrap().clone()
}

fn load(data: &Path, repo: &str) -> RepoSnapshot {
    SnapshotStore::new(data).load(repo).unwrap().unwrap()
}

#[test]
fn the_event_is_snapshot_and_the_thread_ticks_every_minute() {
    assert_eq!(SNAPSHOT_EVENT, "snapshot");
    assert_eq!(TICK_EVERY_SECS, 60);
}

#[test]
fn no_priority_copy_and_no_snapshot_is_no_repos() {
    let data = temp_dir("none");
    assert_eq!(repos(&data).unwrap(), Vec::<String>::new());
}

#[test]
fn repos_are_the_priority_order_then_the_rest_of_the_store_by_name() {
    let data = temp_dir("order");
    write_priority(&data, &["o/b", "o/a", "o/b"]);
    store_snapshot(&data, "o/d", T0);
    store_snapshot(&data, "o/a", T0);
    store_snapshot(&data, "o/c", T0);
    assert_eq!(repos(&data).unwrap(), vec!["o/b", "o/a", "o/c", "o/d"]);
}

#[test]
fn with_no_priority_copy_the_repos_are_the_store_by_name() {
    let data = temp_dir("store");
    store_snapshot(&data, "o/c", T0);
    store_snapshot(&data, "o/a", T0);
    assert_eq!(repos(&data).unwrap(), vec!["o/a", "o/c"]);
}

#[test]
fn a_priority_copy_that_does_not_parse_is_a_parse_error() {
    let data = temp_dir("badcopy");
    std::fs::write(data.join("priority.json"), "{ not json").unwrap();
    assert!(matches!(repos(&data), Err(CoreError::Parse(_))));
    let t = Recorded::new(answers(1), vec![]);
    assert!(matches!(
        tick(&t, &data, &FixedClock::at(T0)),
        Err(CoreError::Parse(_))
    ));
    assert!(log(&t).is_empty());
}

#[test]
fn a_tick_at_0_and_10_minutes_reads_each_repo_once_and_one_at_5_minutes_reads_none() {
    let data = temp_dir("tick");
    write_priority(&data, &["o/a", "o/b"]);
    let t = Recorded::new(answers(4), vec![changed("\"e1\""), changed("\"e2\"")]);
    let clock = FixedClock::at(T0);

    assert_eq!(tick(&t, &data, &clock).unwrap(), vec!["o/a", "o/b"]);
    assert_eq!(log(&t), vec!["graphql", "graphql"]);
    assert_eq!(load(&data, "o/a").read_at, T0);
    assert_eq!(load(&data, "o/a").labels, vec!["read-1"]);
    assert_eq!(load(&data, "o/b").labels, vec!["read-2"]);

    clock.advance(300);
    assert_eq!(tick(&t, &data, &clock).unwrap(), Vec::<String>::new());
    assert_eq!(log(&t).len(), 2);

    clock.advance(300);
    assert_eq!(tick(&t, &data, &clock).unwrap(), vec!["o/a", "o/b"]);
    assert_eq!(
        log(&t),
        vec![
            "graphql",
            "graphql",
            "GET /repos/o/a/events",
            "graphql",
            "GET /repos/o/b/events",
            "graphql"
        ]
    );
    let a = load(&data, "o/a");
    assert_eq!(a.read_at, T0 + 600);
    assert_eq!(a.labels, vec!["read-3"]);
    assert_eq!(a.etag.as_deref(), Some("\"e1\""));
    assert_eq!(load(&data, "o/b").etag.as_deref(), Some("\"e2\""));
}

#[test]
fn a_tick_with_no_repos_sends_nothing() {
    let data = temp_dir("empty");
    let t = Recorded::new(answers(1), vec![]);
    assert_eq!(
        tick(&t, &data, &FixedClock::at(T0)).unwrap(),
        Vec::<String>::new()
    );
    assert!(log(&t).is_empty());
}

#[test]
fn a_tick_writes_the_store_the_sky_reads() {
    let data = temp_dir("sky");
    write_priority(&data, &["o/b", "o/a"]);
    let t = Recorded::new(answers(2), vec![]);
    tick(&t, &data, &FixedClock::at(T0)).unwrap();
    let sky = switchboard_core::sky::model::load(&data, T0 + 120).unwrap();
    assert_eq!(sky.age_minutes, 2);
    let names: Vec<&str> = sky.constellations.iter().map(|c| c.repo.as_str()).collect();
    assert_eq!(names, vec!["o/b", "o/a"]);
}

#[test]
fn a_failed_read_is_the_error_and_the_other_repo_is_still_kept() {
    let data = temp_dir("fail");
    write_priority(&data, &["o/a", "o/b"]);
    let t = Recorded::new(answers(1), vec![]);
    assert!(matches!(
        tick(&t, &data, &FixedClock::at(T0)),
        Err(CoreError::GitHub(_))
    ));
    assert_eq!(load(&data, "o/a").labels, vec!["read-1"]);
    assert_eq!(SnapshotStore::new(&data).load("o/b").unwrap(), None);
}

#[test]
fn refresh_all_reads_every_repo_now_with_no_rest_check() {
    let data = temp_dir("refresh");
    write_priority(&data, &["o/a", "o/b"]);
    let t = Recorded::new(answers(4), vec![]);
    let clock = FixedClock::at(T0);
    tick(&t, &data, &clock).unwrap();
    clock.advance(30);
    assert_eq!(refresh_all(&t, &data, &clock).unwrap(), vec!["o/a", "o/b"]);
    assert_eq!(log(&t), vec!["graphql", "graphql", "graphql", "graphql"]);
    let b = load(&data, "o/b");
    assert_eq!(b.read_at, T0 + 30);
    assert_eq!(b.labels, vec!["read-4"]);
}

#[test]
fn refresh_all_with_no_repos_reads_nothing() {
    let data = temp_dir("refresh-empty");
    let t = Recorded::new(answers(1), vec![]);
    assert_eq!(
        refresh_all(&t, &data, &FixedClock::at(T0)).unwrap(),
        Vec::<String>::new()
    );
    assert!(log(&t).is_empty());
}

#[test]
fn after_write_reads_that_repo_once_and_no_other() {
    let data = temp_dir("after");
    write_priority(&data, &["o/a", "o/b"]);
    let t = Recorded::new(answers(3), vec![]);
    let clock = FixedClock::at(T0);
    tick(&t, &data, &clock).unwrap();
    clock.advance(5);
    after_write(&t, &data, &clock, "o/b").unwrap();
    assert_eq!(log(&t), vec!["graphql", "graphql", "graphql"]);
    let b = load(&data, "o/b");
    assert_eq!(b.read_at, T0 + 5);
    assert_eq!(b.labels, vec!["read-3"]);
    assert_eq!(load(&data, "o/a").read_at, T0);
}

#[test]
fn after_write_reads_a_repo_the_poll_does_not_list_yet() {
    let data = temp_dir("after-new");
    let t = Recorded::new(answers(1), vec![]);
    after_write(&t, &data, &FixedClock::at(T0), "o/z").unwrap();
    assert_eq!(log(&t), vec!["graphql"]);
    assert_eq!(load(&data, "o/z").read_at, T0);
    assert_eq!(repos(&data).unwrap(), vec!["o/z"]);
}
