//! Locked tests for #14 (GH-1: snapshot store on disk). Qwen never edits this file (R28).

use std::path::PathBuf;
use switchboard_core::github::snapshot::{age_minutes, SnapshotStore};
use switchboard_core::github::{Issue, Milestone, PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::CoreError;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn snap(repo: &str, read_at: i64) -> RepoSnapshot {
    RepoSnapshot {
        repo: repo.to_string(),
        read_at,
        etag: Some("W/\"abc\"".to_string()),
        archived: false,
        issues: vec![Issue {
            number: 14,
            title: "GH-1: snapshot store on disk".to_string(),
            state: "open".to_string(),
            labels: vec!["size:easy".to_string(), "feature:github".to_string()],
            milestone: Some("1.0".to_string()),
            body: "line one\nline two".to_string(),
            closed_by_pr: None,
        }],
        pulls: vec![PullRequest {
            draft: false,
            number: 60,
            title: "Snapshot store".to_string(),
            state: "open".to_string(),
            head_sha: "a".repeat(40),
            head_ref: "qwen/14-snapshot".to_string(),
            labels: vec!["kind:state".to_string()],
            closes: vec![14],
            verdicts: vec![ReviewVerdict {
                verdict: "PASS".to_string(),
                sha: "a".repeat(40),
                needs_me: false,
            }],
            ci_on_head: "success".to_string(),
        }],
        milestones: vec![Milestone {
            title: "1.0".to_string(),
            open_issues: 40,
            closed_issues: 2,
        }],
        labels: vec!["size:easy".to_string()],
        roadmap_md: Some("# Roadmap\n".to_string()),
        code_map_md: None,
    }
}

#[test]
fn path_for_is_owner_double_underscore_name_json_under_snapshot() {
    let data = temp_dir("path");
    let store = SnapshotStore::new(&data);
    assert_eq!(
        store.path_for("iannitram/switchboard"),
        data.join("snapshot").join("iannitram__switchboard.json")
    );
}

#[test]
fn save_creates_the_snapshot_folder_and_load_reads_it_back() {
    let data = temp_dir("roundtrip");
    let store = SnapshotStore::new(&data);
    let s = snap("iannitram/switchboard", 1_000);
    store.save(&s).unwrap();
    assert!(data.join("snapshot").is_dir());
    assert!(store.path_for("iannitram/switchboard").is_file());
    assert_eq!(store.load("iannitram/switchboard").unwrap(), Some(s));
}

#[test]
fn load_is_none_when_nothing_was_saved() {
    let data = temp_dir("none");
    let store = SnapshotStore::new(&data);
    assert_eq!(store.load("iannitram/missing").unwrap(), None);
    store.save(&snap("iannitram/other", 1)).unwrap();
    assert_eq!(store.load("iannitram/missing").unwrap(), None);
}

#[test]
fn save_replaces_the_old_snapshot_and_leaves_no_temp_file() {
    let data = temp_dir("replace");
    let store = SnapshotStore::new(&data);
    store.save(&snap("iannitram/switchboard", 1_000)).unwrap();
    let mut newer = snap("iannitram/switchboard", 1_600);
    newer.issues.clear();
    store.save(&newer).unwrap();
    assert_eq!(store.load("iannitram/switchboard").unwrap(), Some(newer));
    let names: Vec<String> = std::fs::read_dir(data.join("snapshot"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["iannitram__switchboard.json".to_string()]);
}

#[test]
fn load_of_a_file_that_is_not_json_is_a_parse_error() {
    let data = temp_dir("bad");
    let store = SnapshotStore::new(&data);
    std::fs::create_dir_all(data.join("snapshot")).unwrap();
    std::fs::write(store.path_for("iannitram/broken"), "not json").unwrap();
    assert!(matches!(
        store.load("iannitram/broken"),
        Err(CoreError::Parse(_))
    ));
}

#[test]
fn load_all_is_every_snapshot_sorted_by_repo() {
    let data = temp_dir("all");
    let store = SnapshotStore::new(&data);
    for repo in ["zeta/b", "alpha/c", "alpha/a"] {
        store.save(&snap(repo, 5)).unwrap();
    }
    let repos: Vec<String> = store
        .load_all()
        .unwrap()
        .into_iter()
        .map(|s| s.repo)
        .collect();
    assert_eq!(repos, vec!["alpha/a", "alpha/c", "zeta/b"]);
}

#[test]
fn load_all_skips_files_that_do_not_end_in_json() {
    let data = temp_dir("skip");
    let store = SnapshotStore::new(&data);
    store.save(&snap("alpha/a", 5)).unwrap();
    std::fs::write(data.join("snapshot").join("alpha__b.json.tmp"), "half").unwrap();
    std::fs::write(data.join("snapshot").join("notes.txt"), "x").unwrap();
    let all = store.load_all().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].repo, "alpha/a");
}

#[test]
fn load_all_is_empty_when_the_folder_does_not_exist() {
    let data = temp_dir("empty");
    let store = SnapshotStore::new(&data);
    assert_eq!(store.load_all().unwrap(), Vec::<RepoSnapshot>::new());
}

#[test]
fn age_minutes_is_whole_minutes_rounded_down_and_never_negative() {
    let s = snap("alpha/a", 1_000);
    assert_eq!(age_minutes(&s, 1_000), 0);
    assert_eq!(age_minutes(&s, 1_059), 0);
    assert_eq!(age_minutes(&s, 1_060), 1);
    assert_eq!(age_minutes(&s, 1_600), 10);
    assert_eq!(age_minutes(&s, 940), 0);
}
