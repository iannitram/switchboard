//! Locked tests for BAK-W (the `backup_repo` and `backup_all` commands the Sky's buttons call:
//! BAK-1, BAK-2). Written before the build (R28); the builder never edits this file.
//!
//! Nothing here backs up a real repo: `record` is driven with hand-made outcomes, and
//! `run_one` / `run_all` only on the paths that stop before `backup::backup_repo` /
//! `backup::backup_all` run.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use switchboard_core::backup::{BackupOutcome, Manifest};
use switchboard_core::backup_run::{folder, record, repos, run_all, run_one, BackupRow};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::RepoSnapshot;
use switchboard_core::settings::Settings;
use switchboard_core::waiting::{refused_backup_item, Item, LocalItems, Source};
use switchboard_core::CoreError;

/// 2026-11-08T12:00:00Z
const NOW: i64 = 1_794_139_200;

/// A fresh, empty temp dir for one test.
fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-bakw-{tag}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn manifest() -> Manifest {
    Manifest {
        repo: "iannitram/alpha".to_string(),
        taken_at: "2026-11-08T12:00:00Z".to_string(),
        refs: 3,
        issues: 40,
        open_issues: 12,
        pulls: 20,
        open_pulls: 2,
        merged_pulls: 17,
        comments: 90,
        labels: 9,
        label_assignments: 61,
        milestones: 2,
        releases: 1,
        closed_without_pr: 4,
        wiki: true,
        bundle_hash: String::new(),
        resolved_repo: None,
    }
}

fn snap(repo: &str) -> RepoSnapshot {
    RepoSnapshot {
        repo: repo.to_string(),
        read_at: NOW - 600,
        ..Default::default()
    }
}

fn backups_at(dir: &Path) -> Settings {
    Settings {
        nextcloud_backups: Some(dir.to_path_buf()),
        ..Default::default()
    }
}

fn other_item(what: &str) -> Item {
    Item {
        what: what.to_string(),
        why: "because".to_string(),
        source: Source::PoolEdit {
            pool: "qwen".to_string(),
        },
        link: "pool-edits.jsonl".to_string(),
        action: "Open".to_string(),
        since: NOW - 60,
    }
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

// --- folder ---

#[test]
fn the_folder_is_github_owner_name_under_the_backups_folder() {
    let backups = PathBuf::from("backups-root");
    assert_eq!(
        folder(&backups, "iannitram/alpha"),
        backups.join("github").join("iannitram").join("alpha")
    );
}

// --- repos: what "back up all" covers ---

#[test]
fn repos_are_every_repo_in_the_snapshot_store_by_name() {
    let data = scratch("repos");
    let store = SnapshotStore::new(&data);
    store.save(&snap("iannitram/gamma")).unwrap();
    store.save(&snap("iannitram/alpha")).unwrap();
    store.save(&snap("other/beta")).unwrap();
    assert_eq!(
        repos(&data).unwrap(),
        vec![
            "iannitram/alpha".to_string(),
            "iannitram/gamma".to_string(),
            "other/beta".to_string()
        ]
    );
}

#[test]
fn repos_with_no_snapshot_is_the_no_snapshot_error() {
    let data = scratch("repos-none");
    let err = repos(&data).unwrap_err();
    assert_eq!(err, CoreError::GitHub("no snapshot yet".to_string()));
    assert_eq!(err.to_string(), "GitHub: no snapshot yet");
}

// --- record: one outcome as a row, and its Waiting on you item ---

#[test]
fn a_row_has_exactly_the_face_field_names() {
    let data = scratch("keys");
    let backups = scratch("keys-backups");
    let row = record(
        &data,
        &backups,
        "iannitram/alpha",
        &BackupOutcome::Written(manifest()),
        NOW,
    )
    .unwrap();
    let v = serde_json::to_value(&row).unwrap();
    assert_eq!(
        keys(&v),
        vec!["folder", "manifest", "outcome", "reason", "repo"]
    );
}

#[test]
fn a_written_backup_is_a_written_row_with_its_manifest() {
    let data = scratch("written");
    let backups = scratch("written-backups");
    let row = record(
        &data,
        &backups,
        "iannitram/alpha",
        &BackupOutcome::Written(manifest()),
        NOW,
    )
    .unwrap();
    let dir = folder(&backups, "iannitram/alpha");
    assert_eq!(
        row,
        BackupRow {
            repo: "iannitram/alpha".to_string(),
            outcome: "written".to_string(),
            folder: dir.display().to_string(),
            manifest: Some(manifest()),
            reason: None,
        }
    );
    assert_eq!(
        serde_json::to_value(&row).unwrap(),
        json!({
            "repo": "iannitram/alpha",
            "outcome": "written",
            "folder": dir.display().to_string(),
            "manifest": serde_json::to_value(manifest()).unwrap(),
            "reason": null
        })
    );
}

#[test]
fn a_written_backup_with_no_waiting_items_writes_no_waiting_file() {
    let data = scratch("written-quiet");
    let backups = scratch("written-quiet-backups");
    record(
        &data,
        &backups,
        "iannitram/alpha",
        &BackupOutcome::Written(manifest()),
        NOW,
    )
    .unwrap();
    assert!(!LocalItems::new(&data).path().exists());
}

#[test]
fn a_refused_backup_is_a_refused_row_and_its_waiting_item() {
    let data = scratch("refused");
    let backups = scratch("refused-backups");
    let reason = "the new backup has 12 open issues, the last had 40";
    let row = record(
        &data,
        &backups,
        "iannitram/alpha",
        &BackupOutcome::Refused(reason.to_string()),
        NOW,
    )
    .unwrap();
    let dir = folder(&backups, "iannitram/alpha");
    assert_eq!(
        serde_json::to_value(&row).unwrap(),
        json!({
            "repo": "iannitram/alpha",
            "outcome": "refused",
            "folder": dir.display().to_string(),
            "manifest": null,
            "reason": reason
        })
    );
    assert_eq!(
        LocalItems::new(&data).load().unwrap(),
        vec![refused_backup_item("iannitram/alpha", reason, &dir, NOW)]
    );
}

#[test]
fn a_second_refusal_replaces_the_first_item_and_keeps_the_others() {
    let data = scratch("refused-twice");
    let backups = scratch("refused-twice-backups");
    let local = LocalItems::new(&data);
    local.add(other_item("Land the qwen edit")).unwrap();
    record(
        &data,
        &backups,
        "iannitram/alpha",
        &BackupOutcome::Refused("first".to_string()),
        NOW - 86_400,
    )
    .unwrap();
    record(
        &data,
        &backups,
        "iannitram/beta",
        &BackupOutcome::Refused("beta's".to_string()),
        NOW - 3_600,
    )
    .unwrap();
    record(
        &data,
        &backups,
        "iannitram/alpha",
        &BackupOutcome::Refused("second".to_string()),
        NOW,
    )
    .unwrap();
    assert_eq!(
        local.load().unwrap(),
        vec![
            other_item("Land the qwen edit"),
            refused_backup_item(
                "iannitram/alpha",
                "second",
                &folder(&backups, "iannitram/alpha"),
                NOW
            ),
            refused_backup_item(
                "iannitram/beta",
                "beta's",
                &folder(&backups, "iannitram/beta"),
                NOW - 3_600
            ),
        ]
    );
}

#[test]
fn a_written_backup_clears_only_its_own_repo_refused_item() {
    let data = scratch("clears");
    let backups = scratch("clears-backups");
    let local = LocalItems::new(&data);
    local.add(other_item("Land the qwen edit")).unwrap();
    for repo in ["iannitram/alpha", "iannitram/beta"] {
        record(
            &data,
            &backups,
            repo,
            &BackupOutcome::Refused("shrunk".to_string()),
            NOW - 60,
        )
        .unwrap();
    }
    record(
        &data,
        &backups,
        "iannitram/alpha",
        &BackupOutcome::Written(manifest()),
        NOW,
    )
    .unwrap();
    assert_eq!(
        local.load().unwrap(),
        vec![
            other_item("Land the qwen edit"),
            refused_backup_item(
                "iannitram/beta",
                "shrunk",
                &folder(&backups, "iannitram/beta"),
                NOW - 60
            ),
        ]
    );
}

// --- run_one / run_all: the paths that stop before any backup runs ---

#[test]
fn run_one_with_the_backups_folder_unset_is_the_missing_setting() {
    let data = scratch("one-unset");
    for settings in [
        Settings::default(),
        Settings {
            nextcloud_backups: Some(PathBuf::new()),
            ..Default::default()
        },
    ] {
        let err = run_one(&settings, &data, "iannitram/alpha", NOW).unwrap_err();
        assert_eq!(err, CoreError::MissingPath("nextcloud_backups"));
        assert_eq!(err.to_string(), "setting not set: nextcloud_backups");
    }
    assert!(!LocalItems::new(&data).path().exists());
}

#[test]
fn run_all_with_the_backups_folder_unset_is_the_missing_setting() {
    let data = scratch("all-unset");
    SnapshotStore::new(&data)
        .save(&snap("iannitram/alpha"))
        .unwrap();
    let err = run_all(&Settings::default(), &data, NOW).unwrap_err();
    assert_eq!(err, CoreError::MissingPath("nextcloud_backups"));
    // The setting is checked before the snapshot store.
    let empty = scratch("all-unset-empty");
    assert_eq!(
        run_all(&Settings::default(), &empty, NOW).unwrap_err(),
        CoreError::MissingPath("nextcloud_backups")
    );
    assert!(!LocalItems::new(&data).path().exists());
}

#[test]
fn run_all_with_no_snapshot_is_the_no_snapshot_error() {
    let data = scratch("all-none");
    let backups = scratch("all-none-backups");
    assert_eq!(
        run_all(&backups_at(&backups), &data, NOW).unwrap_err(),
        CoreError::GitHub("no snapshot yet".to_string())
    );
}

// --- The shell registers both commands; the code map says so ---

/// The text of `pub fn <name>(` up to the end of its body, checking `#[tauri::command]` is on
/// it.
fn command_body<'a>(commands: &'a str, name: &str) -> &'a str {
    let sig = format!("pub fn {name}(");
    let at = commands
        .find(&sig)
        .unwrap_or_else(|| panic!("commands.rs has no `{sig}`"));
    let before = &commands[..at];
    let attr = before
        .rfind("#[tauri::command]")
        .unwrap_or_else(|| panic!("no #[tauri::command] before `{sig}`"));
    assert!(
        !before[attr..].contains("fn "),
        "#[tauri::command] is not on {name}"
    );
    let end = commands[at..]
        .find("\n}\n")
        .map_or(commands.len(), |i| at + i);
    &commands[at..end]
}

#[test]
fn the_shell_registers_backup_repo_and_backup_all() {
    let main = include_str!("../../../app/src-tauri/src/main.rs");
    let handler = main
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("generate_handler! in main.rs");
    assert!(handler.contains("commands::backup_repo,"), "{handler}");
    assert!(handler.contains("commands::backup_all,"), "{handler}");
    assert!(handler.contains("commands::views,"), "{handler}");

    let commands = include_str!("../../../app/src-tauri/src/commands.rs");
    let one = command_body(commands, "backup_repo");
    assert!(one.contains("repo: String"), "{one}");
    assert!(one.contains("backup_run::run_one("), "{one}");
    assert!(one.contains("FaceError::new(\"backup_repo\""), "{one}");
    let all = command_body(commands, "backup_all");
    assert!(all.contains("backup_run::run_all("), "{all}");
    assert!(all.contains("FaceError::new(\"backup_all\""), "{all}");
}

#[test]
fn the_code_map_lists_bak_w_present() {
    let map = include_str!("../../../docs/code-map.md");
    let section: Vec<&str> = map
        .lines()
        .skip_while(|l| *l != "### backup")
        .skip(1)
        .take_while(|l| !l.starts_with("### "))
        .collect();
    assert!(
        section.iter().any(|l| l.starts_with("- BAK-W ")
            && l.contains("`backup_run`")
            && l.ends_with("): present.")),
        "no `- BAK-W ...: present.` line under `### backup`: {section:?}"
    );
}
