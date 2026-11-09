//! REP-2 (#46): the work tree of moves in a report. Locked tests (R28): the builder never edits
//! this file.

use serde_json::json;
use std::path::PathBuf;
use switchboard_core::joblog::JobRow;
use switchboard_core::reports::{from_card, from_thread, moves, Move};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn turn(lane: &str, model: &str, outcome: Option<&str>, mode: Option<&str>) -> JobRow {
    JobRow {
        lane: Some(lane.to_string()),
        model: Some(model.to_string()),
        outcome: outcome.map(str::to_string),
        failure_mode: mode.map(str::to_string),
        ..Default::default()
    }
}

/// Six rows, three moves: qwen twice, then gemini flash, a hidden claude row, gemini pro and
/// claudeapi sonnet.
fn thread() -> Vec<JobRow> {
    let mut hidden = turn("claude", "opus", Some("passed"), None);
    hidden.implicit = true;
    vec![
        turn("qwen", "qwen3-coder", Some("up"), None),
        turn("qwen", "qwen3-coder", Some("down"), None),
        turn("gemini", "gemini-flash", Some("failed"), Some("tests")),
        hidden,
        turn("gemini", "gemini-pro", None, None),
        turn("claudeapi", "sonnet", Some("passed"), None),
    ]
}

fn mv(from: (&str, &str), to: (&str, &str), why: &str) -> Move {
    Move {
        from_service: from.0.to_string(),
        from_model: from.1.to_string(),
        to_service: to.0.to_string(),
        to_model: to.1.to_string(),
        why: why.to_string(),
    }
}

fn three_moves() -> Vec<Move> {
    vec![
        mv(("qwen", "qwen3-coder"), ("gemini", "gemini-flash"), "down"),
        mv(
            ("gemini", "gemini-flash"),
            ("gemini", "gemini-pro"),
            "failed (tests)",
        ),
        mv(
            ("gemini", "gemini-pro"),
            ("claudeapi", "sonnet"),
            "not recorded",
        ),
    ]
}

#[test]
fn one_move_per_change_of_service_or_model_oldest_first() {
    assert_eq!(moves(&thread()), three_moves());
}

#[test]
fn no_rows_or_one_row_or_no_change_gives_no_moves() {
    assert!(moves(&[]).is_empty());
    assert!(moves(&[turn("qwen", "qwen3-coder", Some("up"), None)]).is_empty());
    let same = [
        turn("qwen", "qwen3-coder", Some("up"), None),
        turn("qwen", "qwen3-coder", Some("down"), None),
    ];
    assert!(moves(&same).is_empty());
}

#[test]
fn why_is_the_earlier_rows_mode_when_it_has_no_outcome() {
    let rows = [
        turn("qwen", "qwen3-coder", None, Some("timeout")),
        turn("gemini", "gemini-flash", None, None),
    ];
    assert_eq!(moves(&rows)[0].why, "timeout");
}

#[test]
fn a_missing_service_or_model_says_not_recorded() {
    let rows = [
        JobRow::default(),
        turn("gemini", "gemini-flash", None, None),
    ];
    assert_eq!(
        moves(&rows),
        vec![mv(
            ("not recorded", "not recorded"),
            ("gemini", "gemini-flash"),
            "not recorded"
        )]
    );
}

#[test]
fn moves_leaves_its_rows_alone() {
    let rows = thread();
    let before = rows.clone();
    let _ = moves(&rows);
    assert_eq!(rows, before);
}

#[test]
fn a_thread_report_carries_its_moves() {
    assert_eq!(from_thread("thread-3", &thread()).moves, three_moves());
}

#[test]
fn a_card_report_carries_the_moves_of_its_own_rows_only() {
    let mut rows = thread();
    for r in rows.iter_mut() {
        r.id = Some("job-b2".to_string());
    }
    let mut other = turn("chatgpt", "gpt-5", Some("down"), None);
    other.id = Some("job-zz".to_string());
    rows.insert(2, other);
    let card = json!({"id": "job-b2", "outcome": "passed"});
    assert_eq!(from_card(&card, &rows).moves, three_moves());
    assert!(from_card(&json!({"outcome": "passed"}), &rows)
        .moves
        .is_empty());
}

#[test]
fn the_code_map_marks_the_moves_present() {
    let map = std::fs::read_to_string(manifest().join("../../docs/code-map.md")).unwrap();
    assert!(map.contains("- REP-2 moves (`moves`): present."));
}
