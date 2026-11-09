//! R28 probe for #17: behaviour the issue states that the locked file does not cover.

use serde_json::{json, Value};
use std::path::PathBuf;
use switchboard_core::clock::FixedClock;
use switchboard_core::github::poll::Poller;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::RepoSnapshot;
use switchboard_core::CoreError;

const T0: i64 = 1_800_000_000;

struct Ok1;
impl GitHubTransport for Ok1 {
    fn graphql(&self, _q: &str, _v: &Value) -> Result<Value, CoreError> {
        Ok(json!({ "repository": { "labels": { "nodes": [ { "name": "x" } ] } } }))
    }
    fn rest_get(&self, _p: &str, _e: Option<&str>) -> Result<RestAnswer, CoreError> {
        panic!("no REST in tick");
    }
}

struct NoRepo;
impl GitHubTransport for NoRepo {
    fn graphql(&self, _q: &str, _v: &Value) -> Result<Value, CoreError> {
        Ok(json!({ "repository": null }))
    }
    fn rest_get(&self, _p: &str, _e: Option<&str>) -> Result<RestAnswer, CoreError> {
        panic!("no REST");
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

#[test]
fn a_repo_without_a_slash_is_an_error_naming_it_and_the_rest_are_still_read() {
    let (store, clock) = (SnapshotStore::new(&temp_dir("slash")), FixedClock::at(T0));
    let p = Poller {
        transport: &Ok1,
        store: &store,
        clock: &clock,
        repos: vec!["noslash".to_string(), "o/b".to_string()],
    };
    match p.tick() {
        Err(CoreError::GitHub(m)) => assert_eq!(m, "not owner/name: noslash"),
        other => panic!("{other:?}"),
    }
    assert!(store.load("o/b").unwrap().is_some());
    assert_eq!(store.load("noslash").unwrap(), None);
}

#[test]
fn a_due_tick_keeps_the_stored_etag() {
    let (store, clock) = (SnapshotStore::new(&temp_dir("tagt")), FixedClock::at(T0));
    store
        .save(&RepoSnapshot {
            repo: "o/a".to_string(),
            read_at: T0 - 600,
            etag: Some("\"e1\"".to_string()),
            ..Default::default()
        })
        .unwrap();
    let p = Poller {
        transport: &Ok1,
        store: &store,
        clock: &clock,
        repos: vec!["o/a".to_string()],
    };
    assert_eq!(p.tick().unwrap(), vec!["o/a"]);
    let a = store.load("o/a").unwrap().unwrap();
    assert_eq!(a.etag.as_deref(), Some("\"e1\""));
    assert_eq!(a.read_at, T0);
}

#[test]
fn an_answer_that_does_not_parse_saves_nothing_and_keeps_the_old_snapshot() {
    let (store, clock) = (SnapshotStore::new(&temp_dir("parse")), FixedClock::at(T0));
    let old = RepoSnapshot {
        repo: "o/a".to_string(),
        read_at: T0 - 700,
        ..Default::default()
    };
    store.save(&old).unwrap();
    let p = Poller {
        transport: &NoRepo,
        store: &store,
        clock: &clock,
        repos: vec!["o/a".to_string()],
    };
    assert!(p.tick().is_err());
    assert_eq!(store.load("o/a").unwrap().unwrap(), old);
    assert!(p.refresh_all().is_err());
    assert_eq!(store.load("o/a").unwrap().unwrap(), old);
}
