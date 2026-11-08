//! REP-1 (#44): reports from fleet done cards and chat threads. Locked tests (R28): the builder
//! never edits this file.

use serde_json::{json, Value};
use std::path::PathBuf;
use switchboard_core::joblog::{JobRow, RowSource};
use switchboard_core::reports::{
    from_card, from_thread, newest_first, parse_time, project_of, Changed, Cost, CHAT_NOT_CHECKED,
    NO_CHECKS,
};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A done card in the shape `fleet` writes to `jobs/done/<id>.json`.
fn card_a() -> Value {
    json!({
        "id": "job-a1",
        "title": "Add the settings screen",
        "kind": "ui",
        "lane": "qwen",
        "model": "qwen3-coder",
        "outcome": "failed",
        "failure_mode": "tests",
        "checks": [
            {"name": "build", "status": "passed", "exit": 0, "seconds": 41.2, "tail": "", "note": ""},
            {"name": "test", "status": "failed", "exit": 1, "seconds": 12.0, "tail": "1 failed", "note": "2 of 9 failed"},
            {"name": "lint", "status": "warn", "exit": 0, "seconds": 3.1, "tail": "", "note": null},
            {"name": "e2e", "status": "skipped", "exit": null, "seconds": 0, "tail": "", "note": "no display"}
        ],
        "seconds": 931.4,
        "tokens": 48211,
        "cost_usd": 0.0,
        "pr_url": "https://github.com/owner/app-a/pull/12",
        "pushed": true,
        "at": "2026-11-06T13:00:43-07:00"
    })
}

fn row(id: &str, files: u64, lines: u64) -> JobRow {
    JobRow {
        id: Some(id.to_string()),
        lane: Some("gemini".to_string()),
        tokens: Some(999),
        files_changed: Some(files),
        loc_changed: Some(lines),
        ..Default::default()
    }
}

fn chat(lane: &str, tokens: Option<u64>, usd: Option<f64>, secs: Option<f64>) -> JobRow {
    JobRow {
        lane: Some(lane.to_string()),
        tokens,
        cost_usd: usd,
        seconds: secs,
        source: RowSource::Chat,
        ..Default::default()
    }
}

#[test]
fn parse_time_reads_the_card_time_as_unix_seconds() {
    assert_eq!(parse_time("2026-11-06T13:00:43-07:00"), Some(1_793_995_243));
    assert_eq!(
        parse_time("2026-11-08T09:15:00.250+00:00"),
        Some(1_794_129_300)
    );
    assert_eq!(parse_time("2000-02-29T23:59:59+05:30"), Some(951_848_999));
    assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
}

#[test]
fn parse_time_refuses_anything_else() {
    for bad in [
        "",
        "2026-11-06",
        "2026-11-06 13:00:43Z",
        "2026-11-06T13:00:43",
        "2026-11-06T13:00:43+0700",
        "2026-13-01T00:00:00Z",
        "2026-11-06T24:00:00Z",
        "2026-11-06T13:00:43.Z",
        "yesterday at 4:45pm",
    ] {
        assert_eq!(parse_time(bad), None, "{bad:?}");
    }
}

#[test]
fn project_of_takes_owner_and_repo_from_a_pull_url() {
    assert_eq!(
        project_of("https://github.com/owner/app-a/pull/12").as_deref(),
        Some("owner/app-a")
    );
    for bad in [
        "",
        "https://github.com/owner/app-a/issues/3",
        "http://github.com/owner/app-a/pull/12",
        "https://github.com/owner/pull/12",
        "https://example.com/owner/app-a/pull/12",
    ] {
        assert_eq!(project_of(bad), None, "{bad:?}");
    }
}

#[test]
fn a_card_gives_id_title_outcome_mode_time_and_project() {
    let r = from_card(&card_a(), &[]);
    assert_eq!(r.id, "job-a1");
    assert_eq!(r.title.as_deref(), Some("Add the settings screen"));
    assert_eq!(r.outcome, "failed");
    assert_eq!(r.failure_mode.as_deref(), Some("tests"));
    assert_eq!(r.at, Some(1_793_995_243));
    assert_eq!(r.project.as_deref(), Some("owner/app-a"));
}

#[test]
fn a_card_repo_wins_over_the_pull_url() {
    let mut card = card_a();
    card["repo"] = json!("owner/app-x");
    assert_eq!(
        from_card(&card, &[]).project.as_deref(),
        Some("owner/app-x")
    );
}

#[test]
fn a_bare_card_says_not_recorded() {
    let r = from_card(&json!({"lane": "", "title": ""}), &[row("job-a1", 1, 2)]);
    assert_eq!(r.id, "not recorded");
    assert_eq!(r.outcome, "not recorded");
    assert_eq!(r.title, None);
    assert_eq!(r.failure_mode, None);
    assert_eq!(r.at, None);
    assert_eq!(r.project, None);
    assert_eq!(r.changed, Changed::default());
    assert_eq!(r.cost, Cost::default());
}

#[test]
fn changed_takes_the_pr_from_the_card_and_files_and_lines_from_the_last_matching_row() {
    let rows = [
        row("job-a1", 1, 10),
        row("job-zz", 7, 700),
        row("job-a1", 3, 64),
    ];
    let r = from_card(&card_a(), &rows);
    assert_eq!(
        r.changed,
        Changed {
            pr: Some("https://github.com/owner/app-a/pull/12".to_string()),
            files: Some(3),
            lines: Some(64),
        }
    );
}

#[test]
fn checks_split_into_checked_and_not_checked_lines() {
    let r = from_card(&card_a(), &[]);
    assert_eq!(
        r.checked,
        vec![
            "build: passed",
            "test: failed (2 of 9 failed)",
            "lint: warn"
        ]
    );
    assert_eq!(r.not_checked, vec!["e2e: skipped (no display)"]);
}

#[test]
fn no_checks_says_so_in_not_checked() {
    for checks in [json!([]), json!("none"), Value::Null] {
        let mut card = card_a();
        card["checks"] = checks;
        let r = from_card(&card, &[]);
        assert!(r.checked.is_empty());
        assert_eq!(r.not_checked, vec![NO_CHECKS]);
    }
    let mut card = card_a();
    card.as_object_mut().unwrap().remove("checks");
    assert_eq!(from_card(&card, &[]).not_checked, vec![NO_CHECKS]);
    assert_eq!(NO_CHECKS, "no checks were recorded");
}

#[test]
fn cost_comes_from_the_card_first() {
    let r = from_card(&card_a(), &[row("job-a1", 3, 64)]);
    assert_eq!(
        r.cost,
        Cost {
            pool: Some("qwen".to_string()),
            tokens: Some(48211),
            usd: Some(0.0),
            seconds: Some(931.4),
        }
    );
}

#[test]
fn cost_falls_back_to_the_row() {
    let card = json!({"id": "job-c3", "outcome": "passed", "pr_url": "https://github.com/owner/app-c/pull/7"});
    let rows = [JobRow {
        id: Some("job-c3".to_string()),
        lane: Some("gemini".to_string()),
        tokens: Some(1200),
        cost_usd: Some(0.42),
        seconds: Some(60.0),
        ..Default::default()
    }];
    let r = from_card(&card, &rows);
    assert_eq!(
        r.cost,
        Cost {
            pool: Some("gemini".to_string()),
            tokens: Some(1200),
            usd: Some(0.42),
            seconds: Some(60.0),
        }
    );
    assert_eq!(r.project.as_deref(), Some("owner/app-c"));
}

#[test]
fn from_card_leaves_its_inputs_alone() {
    let card = card_a();
    let rows = vec![row("job-a1", 3, 64)];
    let (card_before, rows_before) = (card.clone(), rows.clone());
    let _ = from_card(&card, &rows);
    assert_eq!(card, card_before);
    assert_eq!(rows, rows_before);
}

#[test]
fn a_thread_skips_implicit_rows_and_sums_the_rest() {
    let mut first = chat("claude", Some(100), Some(0.01), Some(5.0));
    first.outcome = Some("up".to_string());
    let mut hidden = chat("qwen", Some(5000), Some(9.0), Some(900.0));
    hidden.implicit = true;
    hidden.outcome = Some("failed".to_string());
    let mut last = chat("chatgpt", Some(250), None, Some(7.5));
    last.outcome = Some("down".to_string());
    last.failure_mode = Some("refused".to_string());
    let rows = [first, last, hidden];

    let r = from_thread("thread-7", &rows);
    assert_eq!(r.id, "thread-7");
    assert_eq!(r.title, None);
    assert_eq!(r.project, None);
    assert_eq!(r.at, None);
    assert_eq!(r.outcome, "down");
    assert_eq!(r.failure_mode.as_deref(), Some("refused"));
    assert_eq!(r.changed, Changed::default());
    assert!(r.checked.is_empty());
    assert_eq!(r.not_checked, vec![CHAT_NOT_CHECKED]);
    assert_eq!(CHAT_NOT_CHECKED, "a chat runs no build, test or lint");
    assert_eq!(
        r.cost,
        Cost {
            pool: Some("chatgpt".to_string()),
            tokens: Some(350),
            usd: Some(0.01),
            seconds: Some(12.5),
        }
    );
}

#[test]
fn an_empty_thread_says_not_recorded() {
    let r = from_thread("thread-0", &[]);
    assert_eq!(r.outcome, "not recorded");
    assert_eq!(r.failure_mode, None);
    assert_eq!(r.cost, Cost::default());
    assert_eq!(r.not_checked, vec![CHAT_NOT_CHECKED]);
}

#[test]
fn newest_first_puts_timed_reports_first_and_keeps_untimed_order() {
    let at = |id: &str, t: Option<&str>| {
        let mut c = json!({"id": id});
        if let Some(t) = t {
            c["at"] = json!(t);
        }
        from_card(&c, &[])
    };
    let reports = vec![
        at("mid", Some("2026-11-06T10:00:00-07:00")),
        at("u1", None),
        at("new", Some("2026-11-08T09:15:00.250+00:00")),
        at("u2", None),
        at("old", Some("2000-02-29T23:59:59+05:30")),
        from_thread("t1", &[]),
    ];
    let before = reports.clone();
    let ids: Vec<String> = newest_first(&reports).into_iter().map(|r| r.id).collect();
    assert_eq!(ids, ["new", "mid", "old", "u1", "u2", "t1"]);
    assert_eq!(reports, before);
}

#[test]
fn a_report_serializes_with_null_for_missing_parts() {
    let v = serde_json::to_value(from_thread("thread-0", &[])).unwrap();
    assert_eq!(
        v,
        json!({
            "id": "thread-0",
            "title": null,
            "project": null,
            "at": null,
            "outcome": "not recorded",
            "failure_mode": null,
            "changed": {"pr": null, "files": null, "lines": null},
            "checked": [],
            "not_checked": ["a chat runs no build, test or lint"],
            "cost": {"pool": null, "tokens": null, "usd": null, "seconds": null},
            "moves": []
        })
    );
}

#[test]
fn the_code_map_marks_the_reports_present() {
    let map = std::fs::read_to_string(manifest().join("../../docs/code-map.md")).unwrap();
    assert!(map.contains(
        "- REP-1 reports (`from_card`, `from_thread`, `newest_first`, `parse_time`, `project_of`): present."
    ));
}
