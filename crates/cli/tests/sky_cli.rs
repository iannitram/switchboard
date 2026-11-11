//! CLI-1 (#73): `switchboard sky` and `switchboard star <repo>#<n>` run as the built binary
//! against a snapshot fixture written to a temporary app data folder.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};
use switchboard_core::clock::{Clock, SystemClock};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::{Issue, PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::sky::model::{self, PRIORITY_FILE};

fn issue(number: u64, title: &str, state: &str, labels: &[&str]) -> Issue {
    Issue {
        number,
        title: title.to_string(),
        state: state.to_string(),
        labels: labels.iter().map(|s| s.to_string()).collect(),
        milestone: Some("1.0".to_string()),
        body: String::new(),
        closed_by_pr: None,
    }
}

fn pull(number: u64, state: &str, closes: &[u64], verdicts: &[&str], ci: &str) -> PullRequest {
    PullRequest {
        number,
        title: format!("PR {number}"),
        state: state.to_string(),
        closes: closes.to_vec(),
        verdicts: verdicts
            .iter()
            .map(|v| ReviewVerdict {
                verdict: v.to_string(),
                sha: "abc".to_string(),
                needs_me: false,
            })
            .collect(),
        ci_on_head: ci.to_string(),
        // The verdicts name the head (#342: `star` marks a verdict on an older commit).
        head_sha: "abc".to_string(),
        ..Default::default()
    }
}

/// Brightness 1/3 (one of three 1.0 stars closed), 3 open, drift: orphan #4 and the stale date.
fn alpha() -> RepoSnapshot {
    let mut stray = issue(4, "Stray", "open", &[]);
    stray.milestone = None;
    RepoSnapshot {
        repo: "iannitram/alpha".to_string(),
        issues: vec![
            issue(1, "Core one", "closed", &["feature:core", "size:easy"]),
            issue(2, "Core two", "open", &["feature:core", "size:medium"]),
            issue(3, "Sky one", "open", &["feature:sky"]),
            stray,
        ],
        pulls: vec![
            pull(10, "open", &[2], &["FAIL", "PASS"], "success"),
            pull(11, "merged", &[1], &[], "none"),
        ],
        roadmap_md: Some(
            "# Roadmap\n\nUpdated: 2020-01-01\n\nfeatures:\n\
             - core: The core (release: 1.0)\n\
             - sky: The sky (release: 1.0)\n\
             - relay: A relay (release: after-1.0)\n"
                .to_string(),
        ),
        ..Default::default()
    }
}

/// Brightness 0 (one open star, one dark star), 1 open, drift: orphan #5, dark `mail`, no
/// `Updated:` line.
fn beta() -> RepoSnapshot {
    RepoSnapshot {
        repo: "iannitram/beta".to_string(),
        issues: vec![issue(
            5,
            "Two homes",
            "open",
            &["feature:chat", "feature:sky"],
        )],
        roadmap_md: Some(
            "features:\n- chat: Chat (release: 1.0)\n- mail: Mail (release: 1.0)\n".to_string(),
        ),
        ..Default::default()
    }
}

/// No issues and no roadmap: brightness 0, drift: the missing `Updated:` line.
fn gamma() -> RepoSnapshot {
    RepoSnapshot {
        repo: "iannitram/gamma".to_string(),
        ..Default::default()
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("switchboard-sky-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The fixture data folder: alpha, beta and gamma, with beta ranked above alpha and gamma
/// unranked (so it comes last).
fn fixture(tag: &str) -> PathBuf {
    let dir = temp_dir(tag);
    let store = SnapshotStore::new(&dir);
    for snap in [alpha(), beta(), gamma()] {
        store.save(&snap).unwrap();
    }
    std::fs::write(
        dir.join(PRIORITY_FILE),
        r#"{"updated_at": "2026-11-05T12:00:00Z", "order": ["iannitram/beta", "iannitram/alpha"]}"#,
    )
    .unwrap();
    dir
}

/// The fixture priority copy's age in days now.
fn copy_age() -> i64 {
    let copy = switchboard_core::sky::priority::parse(
        r#"{"updated_at": "2026-11-05T12:00:00Z", "order": []}"#,
    )
    .unwrap();
    switchboard_core::sky::priority::age_days(&copy, SystemClock.now())
}

/// A pipeline checkout whose legacy job log holds a running job on alpha#2 and a finished one
/// on alpha#1.
fn pipeline(tag: &str) -> PathBuf {
    let dir = temp_dir(tag);
    std::fs::create_dir_all(dir.join("logs")).unwrap();
    let events = [
        json!({"event": "start", "id": "alpha#1", "lane": "qwen", "model": "qwen3", "machine": "strix-halo", "attempt": 1}),
        json!({"event": "end", "id": "alpha#1", "lane": "qwen", "model": "qwen3", "machine": "strix-halo", "outcome": "passed"}),
        json!({"event": "start", "id": "alpha#2", "lane": "claude", "model": "claude-opus-5-5", "machine": "strix-halo", "attempt": 2}),
    ];
    let text: String = events.iter().map(|e| format!("{e}\n")).collect();
    std::fs::write(dir.join("logs").join("jobs.jsonl"), text).unwrap();
    dir
}

/// The built binary with `SWITCHBOARD_DATA` set to `data` and, when given, the pipeline set
/// (its fleet state folder pointed at a folder that does not exist).
fn switchboard(argv: &[&str], data: &Path, pipeline: Option<&Path>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_switchboard"));
    cmd.args(argv)
        .env("SWITCHBOARD_DATA", data)
        .env_remove("SWITCHBOARD_PIPELINE")
        .env_remove("FLEET_STATE_DIR")
        .env("SWITCHBOARD_STATE", data.join("no-fleet-state"));
    if let Some(root) = pipeline {
        cmd.env("SWITCHBOARD_PIPELINE", root);
    }
    cmd.output().expect("run switchboard")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn sky_prints_one_line_per_repo_in_placement_order() {
    let data = fixture("text");
    let out = switchboard(&["sky"], &data, None);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    // #345 added the priority copy's last line.
    assert_eq!(
        stdout(&out),
        format!(
            "iannitram/beta: brightness 0.00, 1 open, 3 drifted\n\
             iannitram/alpha: brightness 0.33, 3 open, 2 drifted\n\
             iannitram/gamma: brightness 0.00, 0 open, 1 drifted\n\
             priority copy: {} days old\n",
            copy_age()
        )
    );
}

#[test]
fn sky_json_is_the_same_rows_as_the_window() {
    let data = fixture("json");
    let out = switchboard(&["--json", "sky"], &data, None);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let rows: Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    assert_eq!(
        rows,
        // #345 added the priority copy's age to each row.
        json!([
            {"repo": "iannitram/beta", "brightness": 0.0, "open": 1, "drifted": 3, "priority_age_days": copy_age()},
            {"repo": "iannitram/alpha", "brightness": 1.0 / 3.0, "open": 3, "drifted": 2, "priority_age_days": copy_age()},
            {"repo": "iannitram/gamma", "brightness": 0.0, "open": 0, "drifted": 1, "priority_age_days": copy_age()},
        ])
    );

    // The same order, brightness and drift as the Sky view's model.
    let sky = model::load(&data, SystemClock.now()).unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), sky.constellations.len());
    for (row, c) in rows.iter().zip(&sky.constellations) {
        assert_eq!(row["repo"], json!(c.repo));
        assert_eq!(row["brightness"], json!(c.brightness));
        assert_eq!(row["drifted"], json!(c.drift.len()));
    }
}

#[test]
fn star_prints_the_facts_of_an_issue() {
    let data = fixture("star");
    let root = pipeline("star-pipeline");
    let out = switchboard(&["star", "iannitram/alpha#2"], &data, Some(&root));
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "iannitram/alpha#2: Core two\n\
         state: open\n\
         labels: feature:core, size:medium\n\
         size: medium\n\
         pr: #10 open\n\
         review: PASS\n\
         ci: success\n\
         job: claude claude-opus-5-5 on strix-halo, attempt 2\n"
    );
}

#[test]
fn star_takes_the_bare_repo_name_and_a_finished_job_is_none() {
    let data = fixture("bare");
    let root = pipeline("bare-pipeline");
    let out = switchboard(&["star", "alpha#1"], &data, Some(&root));
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "iannitram/alpha#1: Core one\n\
         state: closed\n\
         labels: feature:core, size:easy\n\
         size: easy\n\
         pr: #11 merged\n\
         review: none\n\
         ci: none\n\
         job: none\n"
    );
}

#[test]
fn star_without_a_pipeline_says_the_job_is_not_recorded() {
    let data = fixture("nopipe");
    let out = switchboard(&["star", "iannitram/alpha#4"], &data, None);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "iannitram/alpha#4: Stray\n\
         state: open\n\
         labels: none\n\
         size: none\n\
         pr: none\n\
         review: none\n\
         ci: none\n\
         job: not recorded\n"
    );
}

#[test]
fn star_json_is_the_same_facts_as_one_object() {
    let data = fixture("starjson");
    let root = pipeline("starjson-pipeline");
    let out = switchboard(&["star", "--json", "alpha#2"], &data, Some(&root));
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let star: Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    assert_eq!(
        star,
        json!({
            "repo": "iannitram/alpha",
            "number": 2,
            "title": "Core two",
            "state": "open",
            "labels": ["feature:core", "size:medium"],
            "size": "medium",
            "pr": {"number": 10, "state": "open", "review": "PASS", "ci": "success"},
            "job": "claude claude-opus-5-5 on strix-halo, attempt 2",
        })
    );

    let out = switchboard(&["--json", "star", "iannitram/alpha#4"], &data, None);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let star: Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    assert_eq!(star["pr"], Value::Null);
    assert_eq!(star["size"], Value::Null);
    assert_eq!(star["job"], json!("not recorded"));
}

#[test]
fn star_for_an_issue_the_snapshot_does_not_hold_exits_1() {
    let data = fixture("missing");
    for target in ["iannitram/alpha#99", "iannitram/nowhere#1", "nowhere#1"] {
        let out = switchboard(&["star", target], &data, None);
        assert_eq!(out.status.code(), Some(1), "{target}");
        assert_eq!(stdout(&out), "", "{target}");
        assert_eq!(
            stderr(&out),
            format!("switchboard: no such star: {target}\n")
        );
    }
}

#[test]
fn a_missing_snapshot_exits_1_with_the_message() {
    let empty = temp_dir("empty");
    let absent = empty.join("never-made");
    for data in [&empty, &absent] {
        for argv in [
            &["sky"][..],
            &["--json", "sky"][..],
            &["star", "iannitram/alpha#2"][..],
        ] {
            let out = switchboard(argv, data, None);
            assert_eq!(out.status.code(), Some(1), "{argv:?} in {}", data.display());
            assert_eq!(stdout(&out), "");
            assert_eq!(
                stderr(&out),
                "switchboard: no snapshot yet; run switchboard refresh\n"
            );
        }
    }
}

#[test]
fn star_without_a_target_is_a_usage_error() {
    let data = fixture("usage");
    for argv in [
        &["star"][..],
        &["star", "alpha"][..],
        &["star", "alpha#x"][..],
    ] {
        let out = switchboard(argv, &data, None);
        assert_eq!(out.status.code(), Some(2), "{argv:?}");
        assert_eq!(stdout(&out), "");
    }
}
