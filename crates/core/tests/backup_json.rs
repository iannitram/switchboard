//! Locked tests for BAK-1 (JSON of issues, PRs, comments, labels, milestones, and the manifest
//! counts). Fixtures are in `tests/fixtures/backup-json/`.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use switchboard_core::backup::{write_json, Manifest, RecordedAnswers};
use switchboard_core::CoreError;

const TAKEN_AT: &str = "2026-11-08T07:00:00Z";

const NAMES: [&str; 6] = [
    "issues.json",
    "pulls.json",
    "comments.json",
    "labels.json",
    "milestones.json",
    "manifest.json",
];

fn array(text: &str) -> Vec<Value> {
    serde_json::from_str(text).unwrap()
}

fn fixture() -> RecordedAnswers {
    RecordedAnswers {
        repo: "example/fixture".to_string(),
        issues: array(include_str!("fixtures/backup-json/issues.json")),
        pulls: array(include_str!("fixtures/backup-json/pulls.json")),
        comments: array(include_str!("fixtures/backup-json/comments.json")),
        labels: array(include_str!("fixtures/backup-json/labels.json")),
        milestones: array(include_str!("fixtures/backup-json/milestones.json")),
    }
}

fn empty() -> RecordedAnswers {
    RecordedAnswers {
        repo: "example/empty".to_string(),
        ..RecordedAnswers::default()
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap()
}

#[test]
fn manifest_counts_match_the_fixture() {
    let dir = temp_dir("bak-counts");
    let manifest = write_json(&fixture(), &dir, TAKEN_AT).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(
        manifest,
        Manifest {
            repo: "example/fixture".to_string(),
            taken_at: TAKEN_AT.to_string(),
            refs: 0,
            issues: 5,
            open_issues: 2,
            pulls: 4,
            open_pulls: 1,
            merged_pulls: 2,
            comments: 6,
            labels: 4,
            label_assignments: 5,
            milestones: 2,
            releases: 0,
            closed_without_pr: 2,
            wiki: false,
        }
    );
}

#[test]
fn label_assignments_sums_labels_over_issues() {
    let dir = temp_dir("bak-labels");
    let answers = RecordedAnswers {
        issues: vec![
            json!({"number": 1, "state": "open", "labels": [{"name": "a"}, {"name": "b"}]}),
            json!({"number": 2, "state": "closed", "labels": []}),
            json!({"number": 3, "state": "open", "labels": [{"name": "c"}]}),
        ],
        // The repo's own label list does not count as assignments.
        labels: vec![
            json!({"name": "a"}),
            json!({"name": "b"}),
            json!({"name": "c"}),
        ],
        ..empty()
    };
    let manifest = write_json(&answers, &dir, TAKEN_AT).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(manifest.label_assignments, 3);
    assert_eq!(manifest.labels, 3);
}

#[test]
fn issue_closed_by_merged_pr_is_not_counted_and_closed_by_hand_is() {
    let dir = temp_dir("bak-closed");
    let closed_by_pr = RecordedAnswers {
        issues: vec![json!({"number": 7, "state": "closed", "labels": []})],
        pulls: vec![json!({
            "number": 8, "state": "closed", "merged_at": "2026-11-01T10:00:00Z",
            "body": "fixes: #7"
        })],
        ..empty()
    };
    let by_pr = write_json(&closed_by_pr, &dir, TAKEN_AT).unwrap();
    assert_eq!(by_pr.closed_without_pr, 0);

    let by_hand = RecordedAnswers {
        issues: vec![
            json!({"number": 7, "state": "closed", "labels": []}),
            json!({"number": 9, "state": "closed", "labels": []}),
        ],
        pulls: closed_by_pr.pulls.clone(),
        ..empty()
    };
    let manifest = write_json(&by_hand, &dir, TAKEN_AT).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(manifest.closed_without_pr, 1);
}

#[test]
fn merged_pulls_counts_pulls_with_merged_at_set() {
    // Owner ruling on #92 (2026-11-08): the manifest carries the merged count BAK-2 discounts.
    let dir = temp_dir("bak-merged");
    let answers = RecordedAnswers {
        pulls: vec![
            json!({"number": 1, "state": "closed", "merged_at": "2026-11-01T10:00:00Z", "body": ""}),
            json!({"number": 2, "state": "closed", "merged_at": null, "body": ""}),
            json!({"number": 3, "state": "open", "merged_at": null, "body": ""}),
            json!({"number": 4, "state": "closed", "merged_at": "2026-11-03T08:15:00Z", "body": ""}),
            // No `merged_at` at all counts as not merged.
            json!({"number": 5, "state": "closed", "body": ""}),
        ],
        ..empty()
    };
    let manifest = write_json(&answers, &dir, TAKEN_AT).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(manifest.merged_pulls, 2);
    assert_eq!(manifest.pulls, 5);
    assert_eq!(manifest.open_pulls, 1);
}

#[test]
fn a_closing_pr_that_is_not_merged_does_not_count() {
    let dir = temp_dir("bak-unmerged");
    let answers = fixture();
    let manifest = write_json(&answers, &dir, TAKEN_AT).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    // #5 is closed and only an open PR says `Fixes #5`; #4 only an unmerged PR says `Closes #4`.
    assert_eq!(manifest.closed_without_pr, 2);
}

#[test]
fn the_six_files_exist_and_the_manifest_file_matches_the_return_value() {
    let dir = temp_dir("bak-files");
    let out = dir.join("out");
    let manifest = write_json(&fixture(), &out, TAKEN_AT).unwrap();

    for name in NAMES {
        assert!(out.join(name).is_file(), "{name} is missing");
    }
    for name in NAMES {
        assert!(
            !out.join(format!("{name}.new")).exists(),
            "{name}.new left behind"
        );
    }
    let on_disk: Manifest = serde_json::from_str(&read(&out, "manifest.json")).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(on_disk, manifest);
}

#[test]
fn a_failure_mid_way_leaves_the_old_files_untouched() {
    let dir = temp_dir("bak-failure");
    let out = dir.join("out");
    write_json(&fixture(), &out, TAKEN_AT).unwrap();
    let before: Vec<String> = NAMES.iter().map(|name| read(&out, name)).collect();

    // A directory where `labels.json.new` should go makes the fourth write fail, after the
    // first three `.new` files are written.
    std::fs::create_dir(out.join("labels.json.new")).unwrap();
    let changed = RecordedAnswers {
        issues: Vec::new(),
        pulls: Vec::new(),
        ..empty()
    };
    let result = write_json(&changed, &out, TAKEN_AT);

    let after: Vec<String> = NAMES.iter().map(|name| read(&out, name)).collect();
    let stray_new = ["issues.json.new", "pulls.json.new", "comments.json.new"]
        .iter()
        .any(|name| out.join(name).exists());
    std::fs::remove_dir_all(&dir).ok();

    assert!(matches!(result, Err(CoreError::Io(_))), "got {result:?}");
    assert_eq!(after, before);
    assert!(!stray_new, "a partial write left .new files behind");
}
