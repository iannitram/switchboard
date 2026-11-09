//! Tests for FD-9 (`rating_row` logs a thumbs up or down as that turn's outcome; `implicit_row`
//! logs one of the four implicit signals as its own row, marked implicit; `Implicit::name` gives
//! the id suffix; both kinds of row sit beside ledger rows through `read_all`). Answers are built
//! in code; no service is called.

use serde_json::json;
use switchboard_core::frontdoor::adapters::Answer;
use switchboard_core::frontdoor::sticky::{implicit_row, rating_row, Implicit};
use switchboard_core::joblog::{read_all, RowSource};

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
fn thumbs_up_is_rated_up_and_passed() {
    let row = rating_row(&answer(), Some("chat"), "handoff-7", true);
    assert_eq!(row.rating.as_deref(), Some("up"));
    assert_eq!(row.outcome.as_deref(), Some("passed"));
    assert_eq!(row.lane.as_deref(), Some("gemini"));
    assert_eq!(row.model.as_deref(), Some("gemini-2.5-flash"));
    assert_eq!(row.kind.as_deref(), Some("chat"));
    assert_eq!(row.handoff_id.as_deref(), Some("handoff-7"));
    assert_eq!(row.source, RowSource::Chat);
    assert!(!row.implicit);
}

#[test]
fn thumbs_down_is_rated_down_and_failed() {
    let row = rating_row(&answer(), Some("chat"), "handoff-8", false);
    assert_eq!(row.rating.as_deref(), Some("down"));
    assert_eq!(row.outcome.as_deref(), Some("failed"));
    assert_eq!(row.lane.as_deref(), Some("gemini"));
    assert_eq!(row.handoff_id.as_deref(), Some("handoff-8"));
    assert_eq!(row.source, RowSource::Chat);
    assert!(!row.implicit);
}

#[test]
fn rating_row_without_kind_leaves_kind_none() {
    let row = rating_row(&answer(), None, "handoff-9", true);
    assert_eq!(row.kind, None);
    assert_eq!(row.tokens, None);
    assert_eq!(row.cost_usd, None);
}

#[test]
fn every_implicit_signal_has_its_name() {
    assert_eq!(Implicit::OpenedInClaude.name(), "opened-in-claude");
    assert_eq!(Implicit::ReAsked.name(), "re-asked");
    assert_eq!(Implicit::Abandoned.name(), "abandoned");
    assert_eq!(Implicit::CopiedAnswer.name(), "copied-answer");
}

#[test]
fn implicit_row_is_implicit_with_no_rating_or_outcome() {
    let row = implicit_row(
        Implicit::ReAsked,
        Some("deepseek"),
        Some("deepseek-chat"),
        Some("chat"),
        "handoff-10",
    );
    assert!(row.implicit);
    assert_eq!(row.rating, None);
    assert_eq!(row.outcome, None);
    assert_eq!(row.id.as_deref(), Some("handoff-10#implicit-re-asked"));
    assert_eq!(row.lane.as_deref(), Some("deepseek"));
    assert_eq!(row.model.as_deref(), Some("deepseek-chat"));
    assert_eq!(row.kind.as_deref(), Some("chat"));
    assert_eq!(row.handoff_id.as_deref(), Some("handoff-10"));
    assert_eq!(row.source, RowSource::Chat);
}

#[test]
fn each_implicit_signal_gets_its_own_id_suffix() {
    let cases = [
        (Implicit::OpenedInClaude, "h#implicit-opened-in-claude"),
        (Implicit::ReAsked, "h#implicit-re-asked"),
        (Implicit::Abandoned, "h#implicit-abandoned"),
        (Implicit::CopiedAnswer, "h#implicit-copied-answer"),
    ];
    for (signal, want) in cases {
        let row = implicit_row(signal, Some("gemini"), None, None, "h");
        assert_eq!(row.id.as_deref(), Some(want));
        assert!(row.implicit);
    }
}

#[test]
fn implicit_row_with_unknown_service_leaves_lane_none() {
    let row = implicit_row(Implicit::Abandoned, None, None, None, "handoff-11");
    assert_eq!(row.lane, None);
    assert_eq!(row.model, None);
    assert_eq!(row.kind, None);
    assert_eq!(row.rating, None);
    assert_eq!(row.outcome, None);
    assert!(row.implicit);
}

#[test]
fn rating_and_implicit_rows_come_back_from_read_all_beside_ledger_rows() {
    let events = vec![json!({"id": "a#1", "event": "end", "lane": "qwen", "outcome": "passed"})];
    let chat = vec![
        rating_row(&answer(), Some("chat"), "handoff-12", true),
        implicit_row(
            Implicit::CopiedAnswer,
            Some("gemini"),
            Some("gemini-2.5-flash"),
            Some("chat"),
            "handoff-12",
        ),
    ];
    let rows = read_all(&events, &chat);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].id.as_deref(), Some("a#1"));
    assert!(!rows[0].implicit);
    assert_eq!(rows[1].rating.as_deref(), Some("up"));
    assert_eq!(rows[1].source, RowSource::Chat);
    assert!(rows[2].implicit);
    assert_eq!(
        rows[2].id.as_deref(),
        Some("handoff-12#implicit-copied-answer")
    );
    let implicit_count = rows.iter().filter(|r| r.implicit).count();
    assert_eq!(implicit_count, 1);
}
