//! #372 (FD-9): a rating names a turn of a thread that exists, that turn is an answer, and the
//! service and model are the ones that answered it; else it is refused and nothing is written.
//! A second rating of one turn replaces the first. An implicit signal for a thread that does not
//! exist is refused too.

use std::fs;
use std::path::{Path, PathBuf};
use switchboard_core::frontdoor::implicit_log::log_implicit;
use switchboard_core::frontdoor::rating_log::log_rating;
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::joblog::{JobRow, RowSource};
use switchboard_core::CoreError;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-fd9c-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn turn(role: &str, service: Option<&str>, model: Option<&str>) -> JobRow {
    JobRow {
        role: Some(role.to_string()),
        text: Some(format!("a {role} turn")),
        lane: service.map(str::to_string),
        model: model.map(str::to_string),
        source: RowSource::Chat,
        ..JobRow::default()
    }
}

/// Thread `t`: a question (turn 0), qwen's answer (turn 1), an implicit row (not a turn), a
/// question (turn 2), gemini's answer (turn 3).
fn thread(data: &Path) -> Vec<JobRow> {
    let store = ThreadStore::new(data);
    let rows = vec![
        turn("user", None, None),
        turn("assistant", Some("qwen"), Some("qwen3-coder")),
        JobRow {
            id: Some("t#implicit-copied-answer".to_string()),
            implicit: true,
            source: RowSource::Chat,
            ..JobRow::default()
        },
        turn("user", None, None),
        turn("assistant", Some("gemini"), Some("gemini-3-flash")),
    ];
    for row in &rows {
        store.append("t", row).unwrap();
    }
    rows
}

fn refused(text: &str) -> Result<JobRow, CoreError> {
    Err(CoreError::Refused(text.to_string()))
}

#[test]
fn a_rating_for_a_thread_that_does_not_exist_is_refused_and_writes_nothing() {
    let data = scratch("ghost");
    for _ in 0..2 {
        assert_eq!(
            log_rating(&data, "ghost-123", 0, "up", "qwen", "qwen3-coder", None),
            refused("no thread ghost-123")
        );
    }
    assert!(!data.join("threads").join("ghost-123.jsonl").exists());
    assert_eq!(
        ThreadStore::new(&data).list().unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn a_rating_for_a_turn_the_thread_does_not_have_is_refused() {
    let data = scratch("no-turn");
    let rows = thread(&data);
    assert_eq!(
        log_rating(&data, "t", 4, "up", "gemini", "gemini-3-flash", None),
        refused("no turn 4 in thread t")
    );
    assert_eq!(
        log_rating(&data, "t", 99999, "up", "gemini", "gemini-3-flash", None),
        refused("no turn 99999 in thread t")
    );
    assert_eq!(ThreadStore::new(&data).read("t").unwrap(), rows);
}

#[test]
fn a_rating_of_a_question_is_refused() {
    let data = scratch("question");
    let rows = thread(&data);
    assert_eq!(
        log_rating(&data, "t", 2, "down", "gemini", "gemini-3-flash", None),
        refused("turn 2 of thread t is not an answer")
    );
    assert_eq!(ThreadStore::new(&data).read("t").unwrap(), rows);
}

#[test]
fn a_rating_for_a_model_that_did_not_answer_is_refused() {
    let data = scratch("model");
    let rows = thread(&data);
    assert_eq!(
        log_rating(&data, "t", 1, "up", "gemini", "gemini-3-flash", None),
        refused("turn 1 of thread t was answered by qwen qwen3-coder, not gemini gemini-3-flash")
    );
    assert_eq!(
        log_rating(&data, "t", 3, "up", "gemini", "gemini-2-pro", None),
        refused(
            "turn 3 of thread t was answered by gemini gemini-3-flash, not gemini gemini-2-pro"
        )
    );
    assert_eq!(ThreadStore::new(&data).read("t").unwrap(), rows);
}

#[test]
fn the_turn_index_counts_turns_not_rating_or_implicit_rows() {
    let data = scratch("index");
    thread(&data);
    // Turn 3 is gemini's answer, though it is the fifth row.
    log_rating(&data, "t", 3, "up", "gemini", "gemini-3-flash", None).unwrap();
    // And the rating row did not shift the turns: turn 1 is still qwen's answer.
    log_rating(&data, "t", 1, "down", "qwen", "qwen3-coder", None).unwrap();
}

#[test]
fn a_second_rating_of_a_turn_replaces_the_first() {
    let data = scratch("replace");
    let mut rows = thread(&data);
    log_rating(&data, "t", 1, "up", "qwen", "qwen3-coder", None).unwrap();
    log_rating(&data, "t", 3, "up", "gemini", "gemini-3-flash", None).unwrap();
    let last = log_rating(&data, "t", 1, "down", "qwen", "qwen3-coder", None).unwrap();
    let read = ThreadStore::new(&data).read("t").unwrap();
    assert_eq!(read.len(), rows.len() + 2, "{read:?}");
    let ratings: Vec<(Option<&str>, Option<&str>)> = read[rows.len()..]
        .iter()
        .map(|r| (r.id.as_deref(), r.rating.as_deref()))
        .collect();
    assert_eq!(
        ratings,
        vec![
            (Some("t#rating-1"), Some("down")),
            (Some("t#rating-3"), Some("up"))
        ]
    );
    rows.extend(read[rows.len()..].iter().cloned());
    assert_eq!(read[5], last);
    // No temp file is left beside the thread.
    let names: Vec<String> = fs::read_dir(data.join("threads"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["t.jsonl".to_string()]);
}

#[test]
fn an_implicit_signal_for_a_thread_that_does_not_exist_is_refused() {
    let data = scratch("implicit-ghost");
    assert_eq!(
        log_implicit(&data, "ghost-123", "copied-answer", None, None, None),
        refused("no thread ghost-123")
    );
    assert!(!data.join("threads").exists());
}
