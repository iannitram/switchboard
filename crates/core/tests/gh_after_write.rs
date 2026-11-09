//! Locked tests for #19 (GH-3: refresh a repo right after an action).
//! Qwen never edits this file (R28).

use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use switchboard_core::clock::FixedClock;
use switchboard_core::github::poll::Poller;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::RepoSnapshot;
use switchboard_core::CoreError;

const T0: i64 = 1_800_000_000;

/// GraphQL answers a tiny repository whose one label names the read ("read-<n>") and fails the
/// repos in `fail`; REST always answers 200 with no ETag.
#[derive(Default)]
struct Fake {
    fail: Mutex<HashSet<String>>,
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

    fn rest_get(&self, path: &str, _etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        self.log.lock().unwrap().push(format!("GET {path}"));
        Ok(RestAnswer {
            status: 200,
            etag: None,
            body: json!([]),
        })
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
fn after_write_reads_only_that_repo_now_with_one_graphql_read() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("one")),
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
    p.after_write("o/a").unwrap();
    assert_eq!(t.reads(), vec!["graphql o/a", "graphql o/b", "graphql o/a"]);
    let a = store.load("o/a").unwrap().unwrap();
    assert_eq!(a.read_at, T0 + 30);
    assert_eq!(a.labels, vec!["read-3"]);
    assert_eq!(store.load("o/b").unwrap().unwrap().read_at, T0);
}

#[test]
fn after_write_sends_no_rest_check() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("norest")),
        FixedClock::at(T0),
    );
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: repos(),
    };
    p.after_write("o/a").unwrap();
    assert_eq!(*t.log.lock().unwrap(), vec!["graphql o/a"]);
}

#[test]
fn after_write_keeps_the_stored_etag() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("etag")),
        FixedClock::at(T0),
    );
    store
        .save(&RepoSnapshot {
            repo: "o/a".to_string(),
            read_at: T0 - 60,
            etag: Some("\"e1\"".to_string()),
            ..Default::default()
        })
        .unwrap();
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: repos(),
    };
    p.after_write("o/a").unwrap();
    let a = store.load("o/a").unwrap().unwrap();
    assert_eq!(a.etag.as_deref(), Some("\"e1\""));
    assert_eq!(a.read_at, T0);
}

#[test]
fn the_other_repos_keep_their_ten_minute_timer() {
    let (t, store, clock) = (
        Fake::default(),
        SnapshotStore::new(&temp_dir("timer")),
        FixedClock::at(T0),
    );
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: repos(),
    };
    p.tick().unwrap();
    clock.advance(300);
    p.after_write("o/a").unwrap();
    clock.advance(300);
    assert_eq!(p.tick().unwrap(), vec!["o/b"]);
    clock.advance(300);
    assert_eq!(p.tick().unwrap(), vec!["o/a"]);
    let reads = t.reads();
    assert_eq!(reads.iter().filter(|r| *r == "graphql o/a").count(), 3);
    assert_eq!(reads.iter().filter(|r| *r == "graphql o/b").count(), 2);
}

#[test]
fn a_failed_after_write_keeps_the_old_snapshot_and_returns_the_error() {
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
    let old = store.load("o/a").unwrap().unwrap();
    t.fail.lock().unwrap().insert("o/a".to_string());
    clock.advance(30);
    assert!(matches!(p.after_write("o/a"), Err(CoreError::GitHub(_))));
    assert_eq!(store.load("o/a").unwrap().unwrap(), old);
}
