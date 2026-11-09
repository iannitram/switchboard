//! CLI-1 (#64) with #50 and #44/#48: `switchboard waiting` and `switchboard reports` as the built
//! binary. The snapshot is written to a temp data folder with the core's own store; the reports
//! come from the fleet stub's `cards` answer.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::{Issue, PullRequest, RepoSnapshot};

const WAITING_TEXT: &str = "\
owner/app-a#5: Pick the sync rule | Labelled needs-ian. | Open issue | https://github.com/owner/app-a/issues/5
owner/app-b#9: Budget copy | Labelled needs-ian. | Open PR | https://github.com/owner/app-b/pull/9
";

const REPORTS_TEXT: &str = "\
Tidy the budget copy | passed | owner/app-b
Add the settings screen | passed | owner/app-a
Fix the sync retry | failed | owner/app-a
Scratch run | failed | not recorded
";

fn stub_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/fleet-stub")
}

/// An empty app data folder for one test.
fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-wr-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(data: &Path, snaps: &[RepoSnapshot]) {
    let store = SnapshotStore::new(data);
    for snap in snaps {
        store.save(snap).unwrap();
    }
}

/// Two repos: app-a's issue was read later than app-b's pull request.
fn snapshots() -> Vec<RepoSnapshot> {
    let app_a = RepoSnapshot {
        repo: "owner/app-a".to_string(),
        read_at: 2000,
        issues: vec![Issue {
            number: 5,
            title: "Pick the sync rule".to_string(),
            state: "open".to_string(),
            labels: vec!["needs-ian".to_string()],
            ..Default::default()
        }],
        ..Default::default()
    };
    let app_b = RepoSnapshot {
        repo: "owner/app-b".to_string(),
        read_at: 1000,
        pulls: vec![PullRequest {
            number: 9,
            title: "Budget copy".to_string(),
            state: "open".to_string(),
            labels: vec!["needs-ian".to_string()],
            ..Default::default()
        }],
        ..Default::default()
    };
    vec![app_b, app_a]
}

/// The built binary against `data`, with the fleet stub as the pipeline and no fleet overrides.
fn switchboard(argv: &[&str], data: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(argv)
        .env("SWITCHBOARD_DATA", data)
        .env("SWITCHBOARD_STATE", data)
        .env("SWITCHBOARD_PIPELINE", stub_root())
        .env_remove("FLEET_ROOT")
        .env_remove("FLEET_STUB_EXIT")
        .env_remove("FLEET_STUB_STDERR")
        .env_remove("FLEET_STUB_BADJSON")
        .output()
        .expect("run switchboard")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn waiting_lists_the_items_newest_first() {
    let data = data_dir("waiting-text");
    save(&data, &snapshots());
    let out = switchboard(&["waiting"], &data);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), WAITING_TEXT);
}

#[test]
fn waiting_with_no_items_prints_the_nothing_waiting_line() {
    let data = data_dir("waiting-empty");
    save(
        &data,
        &[RepoSnapshot {
            repo: "owner/app-a".to_string(),
            read_at: 2000,
            ..Default::default()
        }],
    );
    let out = switchboard(&["waiting"], &data);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "nothing waiting on you\n");
}

#[test]
fn waiting_json_is_an_array_with_every_field() {
    let data = data_dir("waiting-json");
    save(&data, &snapshots());
    let out = switchboard(&["waiting", "--json"], &data);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let printed: Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    let items = printed.as_array().expect("a JSON array");
    assert_eq!(items.len(), 2);
    let whats: Vec<&str> = items.iter().map(|i| i["what"].as_str().unwrap()).collect();
    assert_eq!(
        whats,
        [
            "owner/app-a#5: Pick the sync rule",
            "owner/app-b#9: Budget copy"
        ]
    );
    for item in items {
        for field in ["what", "why", "source", "link", "action", "since"] {
            assert!(item.get(field).is_some(), "{field} missing in {item}");
        }
    }
    assert_eq!(items[0]["since"], 2000);
    assert_eq!(items[0]["action"], "Open issue");
}

#[test]
fn waiting_without_a_snapshot_exits_1_with_the_message() {
    let data = data_dir("waiting-missing");
    let out = switchboard(&["waiting"], &data);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out).trim_end(),
        "switchboard: no snapshot yet; run switchboard refresh"
    );
}

#[test]
fn reports_lists_the_fixture_reports_newest_first() {
    let data = data_dir("reports-text");
    let out = switchboard(&["reports"], &data);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), REPORTS_TEXT);
}

#[test]
fn reports_json_is_an_array_of_reports() {
    let data = data_dir("reports-json");
    let out = switchboard(&["reports", "--json"], &data);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let printed: Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    let reports = printed.as_array().expect("a JSON array");
    let ids: Vec<&str> = reports.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["job-b1", "job-a1", "job-a2", "job-n1"]);
    assert_eq!(reports[0]["project"], "owner/app-b");
    assert_eq!(reports[0]["cost"]["pool"], "gemini");
    assert_eq!(reports[3]["project"], Value::Null);
}

#[test]
fn reports_project_flag_keeps_that_project() {
    let data = data_dir("reports-project");
    let out = switchboard(&["reports", "--project", "owner/app-a"], &data);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "Add the settings screen | passed | owner/app-a\n\
         Fix the sync retry | failed | owner/app-a\n"
    );
}

#[test]
fn reports_outcome_flag_keeps_that_outcome() {
    let data = data_dir("reports-outcome");
    let out = switchboard(&["reports", "--outcome", "failed"], &data);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "Fix the sync retry | failed | owner/app-a\n\
         Scratch run | failed | not recorded\n"
    );
}

#[test]
fn reports_pool_flag_keeps_that_pool() {
    let data = data_dir("reports-pool");
    let out = switchboard(&["--json", "reports", "--pool", "qwen"], &data);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let printed: Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    let ids: Vec<&str> = printed
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["job-a1", "job-n1"]);
}

#[test]
fn a_filter_flag_without_a_value_exits_2() {
    let data = data_dir("reports-no-value");
    let out = switchboard(&["reports", "--project"], &data);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--project needs a value"));
}
