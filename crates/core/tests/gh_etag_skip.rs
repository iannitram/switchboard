//! Locked tests for #18 (GH-2: skip unchanged repos by ETag). Qwen never edits this file (R28).

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use switchboard_core::clock::FixedClock;
use switchboard_core::github::poll::{unchanged, Poller};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::RepoSnapshot;
use switchboard_core::CoreError;

const T0: i64 = 1_800_000_000;

/// GraphQL answers a tiny repository labelled "fresh"; REST answers come from `rest` in order
/// (an `Err` when it is empty).
#[derive(Default)]
struct Fake {
    rest: Mutex<VecDeque<RestAnswer>>,
    log: Mutex<Vec<String>>,
}

impl GitHubTransport for Fake {
    fn graphql(&self, _query: &str, variables: &Value) -> Result<Value, CoreError> {
        self.log.lock().unwrap().push(format!(
            "graphql {}/{}",
            variables["owner"].as_str().unwrap_or("?"),
            variables["name"].as_str().unwrap_or("?")
        ));
        Ok(json!({ "repository": { "labels": { "nodes": [ { "name": "fresh" } ] } } }))
    }

    fn rest_get(&self, path: &str, etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("GET {path} {}", etag.unwrap_or("-")));
        self.rest
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| CoreError::GitHub("no answer".to_string()))
    }
}

fn answer(status: u16, etag: Option<&str>) -> RestAnswer {
    RestAnswer {
        status,
        etag: etag.map(String::from),
        body: if status == 304 {
            Value::Null
        } else {
            json!([])
        },
    }
}

fn fake(answers: Vec<RestAnswer>) -> Fake {
    let f = Fake::default();
    f.rest.lock().unwrap().extend(answers);
    f
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

fn stored(store: &SnapshotStore, etag: Option<&str>) -> RepoSnapshot {
    let s = RepoSnapshot {
        repo: "o/a".to_string(),
        read_at: T0,
        etag: etag.map(String::from),
        labels: vec!["old".to_string()],
        ..Default::default()
    };
    store.save(&s).unwrap();
    s
}

#[test]
fn unchanged_gets_the_repo_events_path_with_the_stored_etag() {
    let t = fake(vec![answer(304, None)]);
    unchanged(&t, "iannitram/switchboard", Some("\"e1\"")).unwrap();
    assert_eq!(
        *t.log.lock().unwrap(),
        vec!["GET /repos/iannitram/switchboard/events \"e1\""]
    );
}

#[test]
fn unchanged_304_is_true_and_keeps_the_sent_etag_when_the_answer_has_none() {
    let t = fake(vec![answer(304, None)]);
    assert_eq!(
        unchanged(&t, "o/a", Some("\"e1\"")).unwrap(),
        (true, Some("\"e1\"".to_string()))
    );
}

#[test]
fn unchanged_304_takes_the_answers_etag_when_it_has_one() {
    let t = fake(vec![answer(304, Some("\"e1b\""))]);
    assert_eq!(
        unchanged(&t, "o/a", Some("\"e1\"")).unwrap(),
        (true, Some("\"e1b\"".to_string()))
    );
}

#[test]
fn unchanged_200_is_false_with_the_new_etag() {
    let t = fake(vec![answer(200, Some("\"e2\""))]);
    assert_eq!(
        unchanged(&t, "o/a", Some("\"e1\"")).unwrap(),
        (false, Some("\"e2\"".to_string()))
    );
}

#[test]
fn unchanged_with_no_stored_etag_sends_none_and_a_200_is_false() {
    let t = fake(vec![answer(200, Some("\"e2\""))]);
    assert_eq!(
        unchanged(&t, "o/a", None).unwrap(),
        (false, Some("\"e2\"".to_string()))
    );
    assert_eq!(*t.log.lock().unwrap(), vec!["GET /repos/o/a/events -"]);
}

#[test]
fn unchanged_any_other_status_is_a_github_error() {
    for status in [404u16, 500] {
        let t = fake(vec![answer(status, None)]);
        assert!(matches!(
            unchanged(&t, "o/a", Some("\"e1\"")),
            Err(CoreError::GitHub(_))
        ));
    }
}

#[test]
fn unchanged_passes_a_transport_error_on() {
    let t = fake(vec![]);
    assert!(matches!(
        unchanged(&t, "o/a", Some("\"e1\"")),
        Err(CoreError::GitHub(_))
    ));
}

#[test]
fn tick_on_a_304_sends_no_graphql_and_moves_read_at_keeping_the_data() {
    let (store, clock) = (SnapshotStore::new(&temp_dir("skip")), FixedClock::at(T0));
    let old = stored(&store, Some("\"e1\""));
    let t = fake(vec![answer(304, None)]);
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec!["o/a".to_string()],
    };
    clock.advance(600);
    assert_eq!(p.tick().unwrap(), vec!["o/a"]);
    assert_eq!(*t.log.lock().unwrap(), vec!["GET /repos/o/a/events \"e1\""]);
    let now = store.load("o/a").unwrap().unwrap();
    assert_eq!(now.read_at, T0 + 600);
    assert_eq!(now.etag.as_deref(), Some("\"e1\""));
    assert_eq!(now.labels, old.labels);
}

#[test]
fn tick_on_a_200_reads_graphql_and_stores_the_new_etag() {
    let (store, clock) = (SnapshotStore::new(&temp_dir("read")), FixedClock::at(T0));
    stored(&store, Some("\"e1\""));
    let t = fake(vec![answer(200, Some("\"e2\""))]);
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec!["o/a".to_string()],
    };
    clock.advance(600);
    assert_eq!(p.tick().unwrap(), vec!["o/a"]);
    assert_eq!(
        *t.log.lock().unwrap(),
        vec!["GET /repos/o/a/events \"e1\"", "graphql o/a"]
    );
    let now = store.load("o/a").unwrap().unwrap();
    assert_eq!(now.etag.as_deref(), Some("\"e2\""));
    assert_eq!(now.labels, vec!["fresh"]);
    assert_eq!(now.read_at, T0 + 600);
}

#[test]
fn tick_with_no_stored_snapshot_reads_graphql_with_no_rest_check() {
    let (store, clock) = (SnapshotStore::new(&temp_dir("new")), FixedClock::at(T0));
    let t = fake(vec![]);
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec!["o/a".to_string()],
    };
    assert_eq!(p.tick().unwrap(), vec!["o/a"]);
    assert_eq!(*t.log.lock().unwrap(), vec!["graphql o/a"]);
}

#[test]
fn tick_when_the_rest_check_fails_keeps_the_old_snapshot_and_returns_the_error() {
    let (store, clock) = (
        SnapshotStore::new(&temp_dir("restfail")),
        FixedClock::at(T0),
    );
    let old = stored(&store, Some("\"e1\""));
    let t = fake(vec![answer(500, None)]);
    let p = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec!["o/a".to_string()],
    };
    clock.advance(600);
    assert!(matches!(p.tick(), Err(CoreError::GitHub(_))));
    assert_eq!(store.load("o/a").unwrap().unwrap(), old);
    assert_eq!(
        t.log.lock().unwrap().len(),
        1,
        "no GraphQL after a failed check"
    );
}
