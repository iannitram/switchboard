//! Locked tests for #17 (GH-1: poll every 10 minutes with an injected clock).
//! Qwen never edits this file (R28).

use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use switchboard_core::clock::FixedClock;
use switchboard_core::github::poll::{Poller, POLL_SECONDS};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::RepoSnapshot;
use switchboard_core::CoreError;

const T0: i64 = 1_800_000_000;

/// Answers GraphQL with a tiny repository whose one label names the read ("read-<n>"), fails
/// the repos in `fail`, and answers REST from `rest` (200, no ETag, when it is empty).
#[derive(Default)]
struct Fake {
    fail: Mutex<HashSet<String>>,
    rest: Mutex<VecDeque<RestAnswer>>,
    log: Mutex<Vec<String>>,
}

impl GitHubTransport for Fake {
    fn graphql(&self, _query: &str, variables: &Value) -> Result<Value, CoreError> {
        let repo = format!(
            "{}/{}",
            variables["owner"].as_str().unwrap_or("?"),
            variables["name"].as_str().unwrap_or("?")
        );
        let mut log = self.log.lock().unwrap();
        log.push(format!("graphql {repo}"));
        if self.fail.lock().unwrap().contains(&repo) {
            return Err(CoreError::GitHub("boom".to_string()));
        }
        let n = log.iter().filter(|l| l.starts_with("graphql")).count();
        Ok(json!({ "repository": { "labels": { "nodes": [ { "name": format!("read-{n}") } ] } } }))
    }

    fn rest_get(&self, path: &str, etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("GET {path} {}", etag.unwrap_or("-")));
        Ok(self.rest.lock().unwrap().pop_front().unwrap_or(RestAnswer {
            status: 200,
            etag: None,
            body: json!([]),
        }))
    }
}

impl Fake {
    fn reads(&self) -> Vec<String> {
        let log = self.log.lock().unwrap();
        log.iter()
            .filter(|l| l.starts_with("graphql"))
            .cloned()
            .collect()
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn repos() -> Vec<String> {
    vec!["o/a".to_string(), "o/b".to_string()]
}

#[test]
fn poll_seconds_is_ten_minutes() {
    assert_eq!(POLL_SECONDS, 600);
}

#[test]
fn first_tick_reads_every_repo_in_order_and_saves_each_snapshot() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("first")),
        FixedClock::at(T0),
    );
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: repos(),
    };
    assert_eq!(p.tick().unwrap(), vec!["o/a", "o/b"]);
    assert_eq!(t.reads(), vec!["graphql o/a", "graphql o/b"]);
    let a = store.load("o/a").unwrap().unwrap();
    assert_eq!(a.repo, "o/a");
    assert_eq!(a.read_at, T0);
    assert_eq!(a.labels, vec!["read-1"]);
    assert_eq!(store.load("o/b").unwrap().unwrap().labels, vec!["read-2"]);
}

#[test]
fn a_tick_before_ten_minutes_reads_nothing_and_at_ten_minutes_reads_again() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("due")),
        FixedClock::at(T0),
    );
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: repos(),
    };
    p.tick().unwrap();
    clock.advance(599);
    assert_eq!(p.tick().unwrap(), Vec::<String>::new());
    assert_eq!(t.reads().len(), 2);
    clock.advance(1);
    assert_eq!(p.tick().unwrap(), vec!["o/a", "o/b"]);
    assert_eq!(t.reads().len(), 4);
    assert_eq!(store.load("o/a").unwrap().unwrap().read_at, T0 + 600);
}

#[test]
fn ticking_every_minute_for_thirty_minutes_reads_each_repo_four_times() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("thirty")),
        FixedClock::at(T0),
    );
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: repos(),
    };
    for _ in 0..=30 {
        p.tick().unwrap();
        clock.advance(60);
    }
    let reads = t.reads();
    assert_eq!(reads.iter().filter(|r| *r == "graphql o/a").count(), 4);
    assert_eq!(reads.iter().filter(|r| *r == "graphql o/b").count(), 4);
}

#[test]
fn a_failed_read_keeps_the_old_snapshot_reads_the_rest_and_returns_the_error() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("fail")),
        FixedClock::at(T0),
    );
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: repos(),
    };
    p.tick().unwrap();
    let old_a = store.load("o/a").unwrap().unwrap();
    t.fail.lock().unwrap().insert("o/a".to_string());
    clock.advance(600);
    assert!(matches!(p.tick(), Err(CoreError::GitHub(_))));
    assert_eq!(store.load("o/a").unwrap().unwrap(), old_a);
    let b = store.load("o/b").unwrap().unwrap();
    assert_eq!(b.read_at, T0 + 600);
    assert_eq!(b.labels, vec!["read-4"]);
}

#[test]
fn refresh_all_reads_every_repo_now_whatever_its_age() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("refresh")),
        FixedClock::at(T0),
    );
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: repos(),
    };
    p.tick().unwrap();
    clock.advance(30);
    assert_eq!(p.refresh_all().unwrap(), vec!["o/a", "o/b"]);
    assert_eq!(t.reads().len(), 4);
    assert_eq!(store.load("o/b").unwrap().unwrap().read_at, T0 + 30);
}

#[test]
fn refresh_all_keeps_the_stored_etag_and_sends_no_rest_check() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("etag")),
        FixedClock::at(T0),
    );
    store
        .save(&RepoSnapshot {
            repo: "o/a".to_string(),
            read_at: T0 - 5_000,
            etag: Some("\"e1\"".to_string()),
            ..Default::default()
        })
        .unwrap();
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec!["o/a".to_string()],
    };
    p.refresh_all().unwrap();
    let a = store.load("o/a").unwrap().unwrap();
    assert_eq!(a.etag.as_deref(), Some("\"e1\""));
    assert_eq!(a.labels, vec!["read-1"]);
    assert_eq!(*t.log.lock().unwrap(), vec!["graphql o/a"]);
}

#[test]
fn refresh_all_with_a_failure_reads_the_rest_and_returns_the_error() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("refail")),
        FixedClock::at(T0),
    );
    t.fail.lock().unwrap().insert("o/a".to_string());
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: repos(),
    };
    assert!(matches!(p.refresh_all(), Err(CoreError::GitHub(_))));
    assert_eq!(store.load("o/a").unwrap(), None);
    assert!(store.load("o/b").unwrap().is_some());
}

#[test]
fn the_graphql_variables_are_owner_and_name() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("vars")),
        FixedClock::at(T0),
    );
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec!["iannitram/switchboard".to_string()],
    };
    p.tick().unwrap();
    assert_eq!(t.reads(), vec!["graphql iannitram/switchboard"]);
    assert!(store.load("iannitram/switchboard").unwrap().is_some());
}
