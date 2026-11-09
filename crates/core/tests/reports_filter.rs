//! REP-3 (#48): filter reports by project, outcome and pool. Locked tests (R28): the builder
//! never edits this file.

use serde_json::json;
use std::path::PathBuf;
use switchboard_core::joblog::JobRow;
use switchboard_core::reports::{filter, from_card, from_thread, Filter, Report};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Five reports: three cards over two projects and two pools, a card with no project and a
/// thread with no project.
fn reports() -> Vec<Report> {
    let card = |id: &str, repo: Option<&str>, outcome: &str, lane: &str| {
        let mut c = json!({"id": id, "outcome": outcome, "lane": lane});
        if let Some(repo) = repo {
            c["repo"] = json!(repo);
        }
        from_card(&c, &[])
    };
    let chat = JobRow {
        lane: Some("qwen".to_string()),
        outcome: Some("down".to_string()),
        ..Default::default()
    };
    vec![
        card("a1", Some("owner/app-a"), "passed", "qwen"),
        card("a2", Some("owner/app-a"), "failed", "gemini"),
        card("b1", Some("owner/app-b"), "passed", "gemini"),
        card("n1", None, "failed", "qwen"),
        from_thread("t1", &[chat]),
    ]
}

fn ids(f: &Filter) -> Vec<String> {
    let all = reports();
    filter(&all, f).into_iter().map(|r| r.id.clone()).collect()
}

fn set(v: &str) -> Option<String> {
    Some(v.to_string())
}

#[test]
fn an_empty_filter_keeps_every_report_in_order() {
    assert_eq!(ids(&Filter::default()), ["a1", "a2", "b1", "n1", "t1"]);
}

#[test]
fn project_keeps_only_that_project() {
    let f = Filter {
        project: set("owner/app-a"),
        ..Default::default()
    };
    assert_eq!(ids(&f), ["a1", "a2"]);
}

#[test]
fn outcome_keeps_only_that_outcome() {
    let f = Filter {
        outcome: set("failed"),
        ..Default::default()
    };
    assert_eq!(ids(&f), ["a2", "n1"]);
}

#[test]
fn pool_keeps_only_that_pool() {
    let f = Filter {
        pool: set("qwen"),
        ..Default::default()
    };
    assert_eq!(ids(&f), ["a1", "n1", "t1"]);
}

#[test]
fn every_set_field_must_match() {
    let f = Filter {
        project: set("owner/app-a"),
        outcome: set("passed"),
        pool: set("qwen"),
    };
    assert_eq!(ids(&f), ["a1"]);
    let none = Filter {
        project: set("owner/app-b"),
        pool: set("qwen"),
        ..Default::default()
    };
    assert!(ids(&none).is_empty());
}

#[test]
fn a_report_without_a_project_never_matches_a_project() {
    let f = Filter {
        project: set("not recorded"),
        ..Default::default()
    };
    assert!(ids(&f).is_empty());
}

#[test]
fn filter_leaves_the_reports_alone() {
    let all = reports();
    let before = all.clone();
    let _ = filter(
        &all,
        &Filter {
            outcome: set("passed"),
            ..Default::default()
        },
    );
    assert_eq!(all, before);
}

#[test]
fn a_filter_reads_from_json_with_null_for_all() {
    let f: Filter =
        serde_json::from_value(json!({"project": null, "outcome": "failed", "pool": null}))
            .unwrap();
    assert_eq!(
        f,
        Filter {
            outcome: set("failed"),
            ..Default::default()
        }
    );
    assert_eq!(
        serde_json::to_value(&f).unwrap(),
        json!({"project": null, "outcome": "failed", "pool": null})
    );
}

#[test]
fn the_code_map_marks_the_filter_present() {
    let map = std::fs::read_to_string(manifest().join("../../docs/code-map.md")).unwrap();
    assert!(map.contains("- REP-3 filter (`filter`, `Filter`): present."));
}
