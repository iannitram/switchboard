//! REP follow-up (#331): a chat thread the thread store cannot read is still skipped, and the
//! reports answer carries a warning that names it and why, so the Reports view can show it.

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::joblog::{JobRow, RowSource};
use switchboard_core::reports::{assemble, thread_rows_checked, ReportsAnswer};

/// A fresh, empty folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-331-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// One chat row for a thread, in the shape the chat commands write.
fn chat_row(handoff_id: &str) -> JobRow {
    JobRow {
        handoff_id: Some(handoff_id.to_string()),
        rating: Some("up".to_string()),
        source: RowSource::Chat,
        ..JobRow::default()
    }
}

#[test]
fn an_unreadable_thread_is_skipped_with_one_warning_naming_it() {
    let dir = scratch("one-bad");
    let store = ThreadStore::new(&dir);
    store.append("chat-good", &chat_row("chat-good")).unwrap();
    // A thread file whose row is not a job row: `read` fails on it.
    fs::write(
        dir.join("threads").join("chat-bad.jsonl"),
        "not a job row\n",
    )
    .unwrap();

    let (threads, warnings) = thread_rows_checked(&dir);
    let reports = assemble(&[], &[], &threads, None);

    // The readable thread gives its report; the unreadable one gives none.
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].id, "chat-good");
    // One warning, naming the bad thread and carrying a reason.
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].thread_id, "chat-bad");
    assert!(!warnings[0].reason.trim().is_empty());
}

#[test]
fn readable_threads_give_no_warnings() {
    let dir = scratch("all-good");
    let store = ThreadStore::new(&dir);
    store.append("chat-a", &chat_row("chat-a")).unwrap();

    let (threads, warnings) = thread_rows_checked(&dir);
    assert_eq!(threads.len(), 1);
    assert!(warnings.is_empty());
}

#[test]
fn a_missing_store_folder_gives_no_threads_and_no_warnings() {
    let dir = scratch("missing").join("never-created");
    let (threads, warnings) = thread_rows_checked(&dir);
    assert!(threads.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn the_answer_has_an_empty_warnings_field_when_there_are_none() {
    let answer = ReportsAnswer {
        reports: Vec::new(),
        warnings: Vec::new(),
    };
    let value = serde_json::to_value(&answer).unwrap();
    assert_eq!(value, json!({"reports": [], "warnings": []}));
}

#[test]
fn the_answer_carries_the_thread_id_and_reason_of_a_warning() {
    let dir = scratch("json");
    write(
        &dir.join("threads").join("chat-bad.jsonl"),
        "not a job row\n",
    );
    let (_, warnings) = thread_rows_checked(&dir);
    let answer = ReportsAnswer {
        reports: Vec::new(),
        warnings,
    };
    let value = serde_json::to_value(&answer).unwrap();
    assert_eq!(value["warnings"][0]["thread_id"], "chat-bad");
    assert!(value["warnings"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("line 1"));
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
