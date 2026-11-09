//! Tests for FD-11 (`split_handoff` takes the last `[[handoff: <reason>]]` marker out of a reply
//! and keeps the rest as given; `log_row` copies an answer into a job-log row, with a field the
//! answer lacks left as "not recorded"; a `ChatAdapter` is called through `&dyn`). Answers are
//! built in code; no service is called.

use switchboard_core::frontdoor::adapters::{
    log_row, split_handoff, Answer, ChatAdapter, StubAdapter,
};
use switchboard_core::frontdoor::packet::Turn;
use switchboard_core::joblog::RowSource;

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
fn marker_is_split_off_with_trailing_space_trimmed() {
    assert_eq!(
        split_handoff("Take it. [[handoff: needs the repo]]"),
        ("Take it.".to_string(), Some("needs the repo".to_string()))
    );
}

#[test]
fn reply_without_marker_is_returned_as_is() {
    let reply = "  Keep going.\n\tno marker here  ";
    assert_eq!(split_handoff(reply), (reply.to_string(), None));
}

#[test]
fn two_markers_use_the_last_reason_and_remove_both() {
    assert_eq!(
        split_handoff("Part one. [[handoff: first]][[handoff: second]]"),
        ("Part one.".to_string(), Some("second".to_string()))
    );
}

#[test]
fn text_around_the_marker_is_not_otherwise_changed() {
    let reply = "Line one.\n  Line two.\n[[handoff: long context]]";
    let kept = "Line one.\n  Line two.\n".to_string();
    assert_eq!(
        split_handoff(reply),
        (kept, Some("long context".to_string()))
    );
}

#[test]
fn marker_without_closing_brackets_is_left_in_the_text() {
    let reply = "Maybe [[handoff: unfinished";
    assert_eq!(split_handoff(reply), (reply.to_string(), None));
}

#[test]
fn log_row_copies_the_answer_and_the_arguments() {
    let row = log_row(&answer(), Some("chat"), "handoff-7");
    assert_eq!(row.lane.as_deref(), Some("gemini"));
    assert_eq!(row.model.as_deref(), Some("gemini-2.5-flash"));
    assert_eq!(row.tokens_in, Some(120));
    assert_eq!(row.tokens_out, Some(30));
    assert_eq!(row.tokens, Some(150));
    assert_eq!(row.cost_usd, Some(0.0004));
    assert_eq!(row.kind.as_deref(), Some("chat"));
    assert_eq!(row.handoff_id.as_deref(), Some("handoff-7"));
    assert_eq!(row.source, RowSource::Chat);
    assert_eq!(row.id, None);
    assert_eq!(row.outcome, None);
}

#[test]
fn log_row_without_cost_leaves_cost_none() {
    let mut no_cost = answer();
    no_cost.cost_usd = None;
    let row = log_row(&no_cost, None, "handoff-8");
    assert_eq!(row.cost_usd, None);
    assert_eq!(row.kind, None);
    assert_eq!(row.tokens_in, Some(120));
}

#[test]
fn log_row_without_token_counts_leaves_them_none() {
    let mut no_tokens = answer();
    no_tokens.tokens_in = None;
    no_tokens.tokens_out = None;
    let row = log_row(&no_tokens, Some("chat"), "handoff-9");
    assert_eq!(row.tokens, None);
    assert_eq!(row.tokens_in, None);
    assert_eq!(row.tokens_out, None);
}

#[test]
fn stub_adapter_is_called_through_a_trait_object() {
    let stub = StubAdapter {
        service: "gemini",
        answer: answer(),
    };
    let adapter: &dyn ChatAdapter = &stub;
    let turns = vec![Turn {
        role: "user".to_string(),
        text: "hello".to_string(),
    }];
    assert_eq!(adapter.service(), "gemini");
    let got = adapter
        .send("gemini-2.5-flash", "system", &turns)
        .expect("stub never fails");
    assert_eq!(got, answer());
}
