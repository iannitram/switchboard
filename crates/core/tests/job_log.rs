//! Locked tests for issue 5 (CORE-4: job log rows from the ledger with chat rows beside them).
//! Qwen never edits this file.

use serde_json::{json, Value};
use std::path::PathBuf;
use switchboard_core::joblog::{
    ledger_files, parse_jsonl, read_all, read_ledger, row_from_ledger, shown, JobRow, RowSource,
    NOT_RECORDED,
};

/// An `end` row written after the pipeline batch: every field CORE-4 names.
fn after_batch() -> Value {
    json!({
        "id": "switchboard#5", "event": "end", "at": "2026-11-08T09:00:00-07:00",
        "kind": "data-shape", "size": "medium", "lane": "qwen", "model": "qwen3-coder-30b",
        "effort": "-", "tokens": 1200, "tokens_in": 1000, "tokens_out": 200.0,
        "cost_usd": 0.0, "seconds": 91.5, "tools": ["read", "edit"], "loc_changed": 40,
        "files_changed": 2, "outcome": "passed", "failure_mode": null, "failure_class": null,
        "attempt": 1, "rounds": 2, "rating": "pass", "handoff_id": "thread-7",
        "checks": [{"name": "tests", "ok": true}], "escalation_step": 1, "review_fix_loc": 12,
        "machine": "machine-a"
    })
}

/// An `end` row written before the batch: no rating, handoff, checks, step or fix size.
fn before_batch() -> Value {
    json!({
        "id": "old#1", "event": "end", "kind": "ui", "size": "easy", "lane": "chatgpt",
        "model": "gpt-6-luna", "tokens": 5000, "cost_usd": 0.12, "seconds": 300,
        "outcome": "failed", "failure_mode": "tests red", "failure_class": "tests", "attempt": 2
    })
}

#[test]
fn a_row_after_the_batch_carries_every_field() {
    let r = row_from_ledger(&after_batch());
    assert_eq!(r.id.as_deref(), Some("switchboard#5"));
    assert_eq!(r.kind.as_deref(), Some("data-shape"));
    assert_eq!(r.size.as_deref(), Some("medium"));
    assert_eq!(r.lane.as_deref(), Some("qwen"));
    assert_eq!(r.model.as_deref(), Some("qwen3-coder-30b"));
    assert_eq!(r.effort.as_deref(), Some("-"));
    assert_eq!(
        (r.tokens, r.tokens_in, r.tokens_out),
        (Some(1200), Some(1000), Some(200))
    );
    assert_eq!((r.cost_usd, r.seconds), (Some(0.0), Some(91.5)));
    assert_eq!(r.tools, Some(json!(["read", "edit"])));
    assert_eq!((r.loc_changed, r.files_changed), (Some(40), Some(2)));
    assert_eq!(r.outcome.as_deref(), Some("passed"));
    assert_eq!(
        (r.failure_mode.clone(), r.failure_class.clone()),
        (None, None)
    );
    assert_eq!((r.attempt, r.rounds), (Some(1), Some(2)));
    assert_eq!(r.rating.as_deref(), Some("pass"));
    assert_eq!(r.handoff_id.as_deref(), Some("thread-7"));
    assert_eq!(r.checks, Some(json!([{"name": "tests", "ok": true}])));
    assert_eq!(r.escalation_step.as_deref(), Some("1"));
    assert_eq!(r.review_fix_loc, Some(12));
    assert_eq!((r.source, r.implicit), (RowSource::Ledger, false));
}

#[test]
fn a_row_before_the_batch_leaves_the_new_fields_unset() {
    let r = row_from_ledger(&before_batch());
    assert_eq!(r.seconds, Some(300.0));
    assert_eq!(r.failure_class.as_deref(), Some("tests"));
    assert_eq!(r.attempt, Some(2));
    assert_eq!((r.rating.clone(), r.handoff_id.clone()), (None, None));
    assert_eq!(
        (
            r.checks.clone(),
            r.escalation_step.clone(),
            r.review_fix_loc
        ),
        (None, None, None)
    );
    assert_eq!(
        (r.tokens_in, r.tokens_out, r.rounds, r.effort.clone()),
        (None, None, None, None)
    );
}

#[test]
fn a_value_of_the_wrong_type_is_none_never_a_guess() {
    let r = row_from_ledger(&json!({
        "event": "end", "id": 7, "tokens": -5, "loc_changed": 2.5, "cost_usd": "free",
        "attempt": 5_000_000_000u64, "model": true, "tools": null, "seconds": "ninety"
    }));
    assert_eq!(r.id.as_deref(), Some("7"));
    assert_eq!(
        (r.tokens, r.loc_changed, r.cost_usd, r.attempt),
        (None, None, None, None)
    );
    assert_eq!(
        (r.model.clone(), r.tools.clone(), r.seconds),
        (None, None, None)
    );
}

#[test]
fn something_that_is_not_an_object_is_an_empty_ledger_row() {
    assert_eq!(row_from_ledger(&json!("text")), JobRow::default());
}

#[test]
fn shown_prints_the_value_or_not_recorded() {
    let r = row_from_ledger(&before_batch());
    assert_eq!(NOT_RECORDED, "not recorded");
    assert_eq!(shown(&r.rating), "not recorded");
    assert_eq!(shown(&r.review_fix_loc), "not recorded");
    assert_eq!(shown(&r.tokens), "5000");
    assert_eq!(shown(&r.model), "gpt-6-luna");
}

fn chat_row(id: &str) -> JobRow {
    JobRow {
        id: Some(id.into()),
        lane: Some("deepseek".into()),
        outcome: Some("passed".into()),
        source: RowSource::Ledger, // read_all marks it Chat
        ..JobRow::default()
    }
}

#[test]
fn read_all_gives_ledger_end_rows_in_order_then_chat_rows_marked_chat() {
    let events = vec![
        json!({"id": "a#1", "event": "start", "lane": "qwen"}),
        json!({"id": "a#1", "event": "end", "lane": "qwen", "outcome": "passed"}),
        json!({"id": "b#2", "event": "end", "lane": "deepseek", "outcome": "failed"}),
        json!({"id": "c#3", "lane": "qwen"}),
    ];
    let rows = read_all(&events, &[chat_row("thread-1"), chat_row("thread-2")]);
    let got: Vec<(&str, RowSource)> = rows
        .iter()
        .map(|r| (r.id.as_deref().unwrap(), r.source))
        .collect();
    assert_eq!(
        got,
        vec![
            ("a#1", RowSource::Ledger),
            ("b#2", RowSource::Ledger),
            ("thread-1", RowSource::Chat),
            ("thread-2", RowSource::Chat),
        ]
    );
    assert_eq!(rows[2].lane.as_deref(), Some("deepseek"));
}

#[test]
fn a_rating_event_rates_the_last_end_row_with_its_id_wherever_it_sits() {
    let events = vec![
        json!({"id": "a#1", "event": "rating", "rating": "down"}),
        json!({"id": "a#1", "event": "end", "outcome": "failed", "attempt": 1}),
        json!({"id": "a#1", "event": "end", "outcome": "passed", "attempt": 2}),
        json!({"id": "a#1", "event": "rating", "rating": "up"}),
        json!({"id": "zz#9", "event": "rating", "rating": "up"}),
    ];
    let rows = read_all(&events, &[]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].rating, None);
    // Ratings apply in slice order: "down" first, then "up" overwrites it.
    assert_eq!(rows[1].rating.as_deref(), Some("up"));
}

#[test]
fn a_rating_that_names_an_attempt_rates_that_attempt() {
    let events = vec![
        json!({"id": "a#1", "event": "end", "outcome": "failed", "attempt": 1}),
        json!({"id": "a#1", "event": "end", "outcome": "passed", "attempt": 2}),
        json!({"id": "a#1", "event": "rating", "rating": "fail", "attempt": 1}),
    ];
    let rows = read_all(&events, &[]);
    assert_eq!(rows[0].rating.as_deref(), Some("fail"));
    assert_eq!(rows[1].rating, None);
}

#[test]
fn read_all_does_not_change_its_inputs() {
    let events = vec![after_batch(), before_batch()];
    let chats = vec![chat_row("thread-1")];
    let (e0, c0) = (events.clone(), chats.clone());
    let _ = read_all(&events, &chats);
    assert_eq!((events, chats), (e0, c0));
}

#[test]
fn parse_jsonl_skips_blank_and_unreadable_lines() {
    let text =
        "{\"id\":\"a\",\"event\":\"end\"}\n\n   \nnot json\n{\"id\":\"b\",\"event\":\"end\"}\r\n";
    assert_eq!(
        parse_jsonl(text),
        vec![
            json!({"id": "a", "event": "end"}),
            json!({"id": "b", "event": "end"})
        ]
    );
}

/// A fresh folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("switchboard-joblog-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn ledger_files_are_the_legacy_file_then_each_machine_file_by_name() {
    let base = scratch("files");
    let (pipeline, state) = (base.join("pipeline"), base.join("state"));
    std::fs::create_dir_all(pipeline.join("logs")).unwrap();
    std::fs::create_dir_all(state.join("ledger")).unwrap();
    std::fs::write(pipeline.join("logs").join("jobs.jsonl"), "").unwrap();
    for name in ["machine-b.jsonl", "machine-a.jsonl", "notes.txt"] {
        std::fs::write(state.join("ledger").join(name), "").unwrap();
    }
    assert_eq!(
        ledger_files(&pipeline, &state),
        vec![
            pipeline.join("logs").join("jobs.jsonl"),
            state.join("ledger").join("machine-a.jsonl"),
            state.join("ledger").join("machine-b.jsonl"),
        ]
    );
    let empty = scratch("nofiles");
    assert!(ledger_files(&empty.join("p"), &empty.join("s")).is_empty());
}

#[test]
fn read_ledger_joins_files_in_order_drops_exact_duplicates_and_skips_missing_files() {
    let d = scratch("read");
    let (one, two) = (d.join("one.jsonl"), d.join("two.jsonl"));
    std::fs::write(
        &one,
        "{\"id\":\"a\",\"event\":\"end\"}\n{\"id\":\"b\",\"event\":\"end\"}\n",
    )
    .unwrap();
    // The same event as one.jsonl's first line, with its keys in another order.
    std::fs::write(
        &two,
        "{\"event\":\"end\",\"id\":\"a\"}\n{\"id\":\"c\",\"event\":\"end\"}\n",
    )
    .unwrap();
    let got = read_ledger(&[one, d.join("missing.jsonl"), two]).unwrap();
    let ids: Vec<&str> = got.iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["a", "b", "c"]);
}

#[test]
fn the_code_map_shows_core_joblog_present() {
    let map = include_str!("../../../docs/code-map.md");
    let line = map
        .lines()
        .skip_while(|l| *l != "### core-joblog")
        .nth(1)
        .unwrap();
    assert!(line.contains("· present."), "{line}");
}
