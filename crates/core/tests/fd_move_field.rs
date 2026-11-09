//! Tests for FD-10 (`with_move` sets `moved` on a thumbs-down rating row and returns every other
//! row unchanged; `thumbs_down_counts` splits the thumbs-downs by whether they moved the chat).
//! Rows are built in code; no service is called.

use serde_json::json;
use switchboard_core::frontdoor::adapters::Answer;
use switchboard_core::frontdoor::sticky::{
    implicit_row, rating_row, thumbs_down_counts, with_move, Implicit,
};
use switchboard_core::joblog::{row_from_ledger, JobRow};

fn answer() -> Answer {
    Answer {
        text: "Take it.".to_string(),
        service: "gemini".to_string(),
        model: "gemini-2.5-flash".to_string(),
        tokens_in: Some(120),
        tokens_out: Some(30),
        cost_usd: Some(0.0004),
        handoff: None,
    }
}

#[test]
fn with_move_true_sets_moved_and_keeps_rating_and_outcome() {
    let row = with_move(
        rating_row(&answer(), Some("chat"), "handoff-1", false),
        true,
    );
    assert_eq!(row.rating.as_deref(), Some("down"));
    assert_eq!(row.outcome.as_deref(), Some("failed"));
    assert_eq!(row.moved, Some(true));
}

#[test]
fn with_move_false_sets_moved_false() {
    let row = with_move(
        rating_row(&answer(), Some("chat"), "handoff-2", false),
        false,
    );
    assert_eq!(row.rating.as_deref(), Some("down"));
    assert_eq!(row.outcome.as_deref(), Some("failed"));
    assert_eq!(row.moved, Some(false));
}

#[test]
fn with_move_leaves_a_thumbs_up_row_unchanged() {
    let before = rating_row(&answer(), Some("chat"), "handoff-3", true);
    let after = with_move(before.clone(), true);
    assert_eq!(after, before);
    assert_eq!(after.moved, None);
}

#[test]
fn with_move_leaves_an_implicit_row_unchanged() {
    let before = implicit_row(Implicit::Abandoned, None, None, None, "handoff-4");
    let after = with_move(before.clone(), true);
    assert_eq!(after, before);
    assert_eq!(after.moved, None);
}

#[test]
fn with_move_leaves_a_ledger_row_unchanged() {
    let before = row_from_ledger(&json!({"event": "end", "id": "job-9", "outcome": "passed"}));
    let after = with_move(before.clone(), false);
    assert_eq!(after, before);
}

#[test]
fn thumbs_down_counts_splits_downs_by_move() {
    let implicit_down = JobRow {
        implicit: true,
        rating: Some("down".to_string()),
        moved: Some(true),
        ..JobRow::default()
    };
    let ledger = JobRow {
        id: Some("job-1".to_string()),
        outcome: Some("failed".to_string()),
        ..JobRow::default()
    };
    let rows = [
        with_move(rating_row(&answer(), Some("chat"), "h-1", false), true),
        with_move(rating_row(&answer(), Some("chat"), "h-2", false), false),
        with_move(rating_row(&answer(), Some("chat"), "h-3", true), true),
        implicit_row(Implicit::ReAsked, None, None, None, "h-4"),
        implicit_down,
        ledger,
        rating_row(&answer(), Some("chat"), "h-5", false),
    ];
    assert_eq!(thumbs_down_counts(&rows), (1, 1));
}

#[test]
fn thumbs_down_counts_is_zero_for_no_rows() {
    assert_eq!(thumbs_down_counts(&[]), (0, 0));
}
