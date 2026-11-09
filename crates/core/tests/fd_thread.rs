//! Locked tests for FD-13 (the thread store: one JSON Lines file per handoff id).
//! Written before the build (R28); the builder never edits this file.

use std::path::PathBuf;
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::joblog::JobRow;
use switchboard_core::CoreError;

fn data_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-fd13-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn row(n: u64) -> JobRow {
    JobRow {
        id: Some(format!("switchboard#{n}")),
        lane: Some("qwen".into()),
        tokens: Some(n * 10),
        cost_usd: Some(0.25),
        ..Default::default()
    }
}

#[test]
fn rows_survive_a_restart_in_order() {
    let dir = data_dir("restart");
    let first = ThreadStore::new(&dir);
    first.append("thread-7", &row(1)).unwrap();
    first.append("thread-7", &row(2)).unwrap();
    drop(first);

    let second = ThreadStore::new(&dir);
    second.append("thread-7", &row(3)).unwrap();
    assert_eq!(
        second.read("thread-7").unwrap(),
        vec![row(1), row(2), row(3)]
    );
    let text = std::fs::read_to_string(dir.join("threads").join("thread-7.jsonl")).unwrap();
    assert_eq!(text.lines().count(), 3);
}

#[test]
fn a_thread_with_no_file_reads_empty() {
    let dir = data_dir("empty");
    let store = ThreadStore::new(&dir);
    assert_eq!(store.read("nothing-yet").unwrap(), Vec::<JobRow>::new());
    assert_eq!(store.list().unwrap(), Vec::<String>::new());
}

#[test]
fn list_is_sorted_and_ignores_other_files() {
    let dir = data_dir("list");
    let store = ThreadStore::new(&dir);
    store.append("b-thread", &row(1)).unwrap();
    store.append("a-thread", &row(1)).unwrap();
    std::fs::write(dir.join("threads").join("notes.txt"), "not a thread").unwrap();
    std::fs::write(dir.join("threads").join("c-thread.json"), "{}").unwrap();
    assert_eq!(store.list().unwrap(), vec!["a-thread", "b-thread"]);
}

#[test]
fn a_bad_line_is_a_parse_error_with_its_line_number() {
    let dir = data_dir("parse");
    let store = ThreadStore::new(&dir);
    store.append("thread-9", &row(1)).unwrap();
    let path = dir.join("threads").join("thread-9.jsonl");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("this is not json\n");
    std::fs::write(&path, text).unwrap();

    match store.read("thread-9") {
        Err(CoreError::Parse(why)) => {
            assert!(why.contains("thread-9.jsonl"), "{why}");
            assert!(why.contains("line 2"), "{why}");
        }
        other => panic!("expected a parse error, got {other:?}"),
    }
}

#[test]
fn a_path_escaping_id_is_refused_and_writes_nothing() {
    let dir = data_dir("refuse");
    let store = ThreadStore::new(&dir);
    for bad in ["../x", "a/b", "a\\b", ".."] {
        assert!(
            matches!(store.append(bad, &row(1)), Err(CoreError::Refused(_))),
            "append {bad}"
        );
        assert!(
            matches!(store.read(bad), Err(CoreError::Refused(_))),
            "read {bad}"
        );
    }
    assert!(!dir.join("x.jsonl").exists());
    assert!(!dir.join("threads").exists());
}
