// Tests for the chat threads the `reports` command reads (REP follow-up, #286). `commands.rs`
// takes this file in with `#[cfg(test)] #[path = "reports_threads_tests.rs"] mod reports_threads_tests;`.
use super::*;
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::joblog::{JobRow, RowSource};

/// A fresh folder for one test, under the system temp folder.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("switchboard-286-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
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
fn a_missing_store_folder_gives_no_threads() {
    let dir = scratch("missing").join("never-created");
    assert!(thread_rows(&dir).is_empty());
}

#[test]
fn an_empty_store_folder_gives_no_threads() {
    let dir = scratch("empty");
    std::fs::create_dir_all(dir.join("threads")).unwrap();
    assert!(thread_rows(&dir).is_empty());
}

#[test]
fn two_stored_threads_are_both_returned() {
    let dir = scratch("two");
    let store = ThreadStore::new(&dir);
    store.append("chat-a", &chat_row("chat-a")).unwrap();
    store.append("chat-b", &chat_row("chat-b")).unwrap();
    store.append("chat-b", &chat_row("chat-b")).unwrap();

    let threads = thread_rows(&dir);
    let ids: Vec<&str> = threads.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, vec!["chat-a", "chat-b"]);
    assert_eq!(threads[0].1.len(), 1);
    assert_eq!(threads[1].1.len(), 2);
    assert_eq!(threads[1].1[0].handoff_id.as_deref(), Some("chat-b"));
}

#[test]
fn an_unreadable_thread_is_skipped_and_the_other_still_shows() {
    let dir = scratch("one-bad");
    let store = ThreadStore::new(&dir);
    store.append("chat-good", &chat_row("chat-good")).unwrap();
    // A thread file whose row is not a job row: `read` fails on it.
    std::fs::write(
        dir.join("threads").join("chat-bad.jsonl"),
        "not a job row\n",
    )
    .unwrap();

    let threads = thread_rows(&dir);
    let ids: Vec<&str> = threads.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, vec!["chat-good"]);
    assert_eq!(threads[0].1.len(), 1);
}
