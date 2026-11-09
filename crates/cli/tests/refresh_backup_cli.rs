//! Locked tests for #75 (CLI-2: `refresh`, `backup <repo>`, `backup --all`). Qwen never edits
//! this file (R28). `refresh_with` and `backup_with` run in-process over a temp app data folder,
//! a fake GitHub transport and a fake backup; the binary test only runs `backup` alone.

// The binary crate has no library target, so the sources are compiled in directly.
#[path = "../src/main.rs"]
mod app;
// `cli.rs` reads `super::parse_args`, so it sits at this root beside the parser's import.
#[path = "../src/cli.rs"]
#[allow(dead_code, clippy::duplicate_mod)]
mod cli;

#[allow(unused_imports)]
use app::{parse_args, Args};

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::Mutex;

use cli::{backup_with, refresh_with};
use serde_json::{json, Value};
use switchboard_core::backup::{BackupOutcome, Manifest};
use switchboard_core::clock::FixedClock;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::RepoSnapshot;
use switchboard_core::settings::Settings;
use switchboard_core::waiting::{LocalItems, Source};
use switchboard_core::CoreError;

const NOW: i64 = 1_700_000_000;

/// REST: `a/same` answers 304 and `a/moved` answers 200 on `/events`. GraphQL answers a tiny
/// repository, except `a/bad`, which fails.
#[derive(Default)]
struct Fake {
    log: Mutex<Vec<String>>,
}

impl GitHubTransport for Fake {
    fn graphql(&self, _query: &str, variables: &Value) -> Result<Value, CoreError> {
        let name = variables["name"].as_str().unwrap_or("?");
        self.log.lock().unwrap().push(format!("graphql {name}"));
        if name == "bad" {
            return Err(CoreError::GitHub("boom".to_string()));
        }
        Ok(json!({ "repository": { "labels": { "nodes": [ { "name": "fresh" } ] } } }))
    }

    fn rest_get(&self, path: &str, _etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        self.log.lock().unwrap().push(format!("GET {path}"));
        let status = if path.contains("/same/") { 304 } else { 200 };
        Ok(RestAnswer {
            status,
            etag: Some("\"new\"".to_string()),
            body: Value::Null,
        })
    }
}

fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("sb-cli2-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(data: &Path, repo: &str) {
    let snap = RepoSnapshot {
        repo: repo.to_string(),
        read_at: 1,
        etag: Some("\"old\"".to_string()),
        ..Default::default()
    };
    SnapshotStore::new(data).save(&snap).unwrap();
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap()
}

#[test]
fn refresh_prints_one_line_per_repo_and_exits_0() {
    let data = scratch("refresh");
    save(&data, "a/same");
    save(&data, "a/moved");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let clock = FixedClock::at(NOW);
    let code = refresh_with(&Fake::default(), &data, &clock, false, &mut out, &mut err);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(text(out), "a/moved: refreshed\na/same: unchanged\n");
    assert_eq!(text(err), "");
    // The unchanged repo kept its snapshot and was stamped read now.
    let kept = SnapshotStore::new(&data).load("a/same").unwrap().unwrap();
    assert_eq!(kept.read_at, NOW);
    assert_eq!(kept.etag.as_deref(), Some("\"new\""));
}

#[test]
fn a_failing_repo_goes_to_stderr_and_exit_1_after_the_others_ran() {
    let data = scratch("refresh-bad");
    save(&data, "a/bad");
    save(&data, "a/moved");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let clock = FixedClock::at(NOW);
    let fake = Fake::default();
    let code = refresh_with(&fake, &data, &clock, false, &mut out, &mut err);
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(text(out), "a/moved: refreshed\n");
    assert_eq!(text(err), "a/bad: failed: GitHub: boom\n");
    let log = fake.log.lock().unwrap().join("|");
    assert!(log.contains("graphql moved"), "{log}");
}

#[test]
fn refresh_json_is_one_array() {
    let data = scratch("refresh-json");
    save(&data, "a/same");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let clock = FixedClock::at(NOW);
    let code = refresh_with(&Fake::default(), &data, &clock, true, &mut out, &mut err);
    assert_eq!(code, ExitCode::SUCCESS);
    let rows: Value = serde_json::from_str(&text(out)).unwrap();
    assert_eq!(rows, json!([{"repo": "a/same", "status": "unchanged"}]));
}

fn manifest(repo: &str) -> Manifest {
    Manifest {
        repo: repo.to_string(),
        ..Default::default()
    }
}

fn settings(backups: &Path) -> Settings {
    Settings {
        nextcloud_backups: Some(backups.to_path_buf()),
        ..Default::default()
    }
}

/// A backup that writes for every repo except `a/wipe`, which BAK-2 refuses.
fn fake_backup(repo: &str) -> Result<BackupOutcome, CoreError> {
    match repo {
        "a/wipe" => Ok(BackupOutcome::Refused(
            "the new backup has 12 open issues, the last had 40".to_string(),
        )),
        "a/down" => Err(CoreError::GitHub("down".to_string())),
        _ => Ok(BackupOutcome::Written(manifest(repo))),
    }
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| w.to_string()).collect()
}

#[test]
fn backup_one_prints_the_path_line() {
    let data = scratch("one");
    let backups = scratch("one-b");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = backup_with(
        &settings(&backups),
        &data,
        &words(&["a/one"]),
        NOW,
        &fake_backup,
        &mut out,
        &mut err,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let path = backups.join("github").join("a").join("one");
    assert_eq!(
        text(out),
        format!("a/one: backed up to {}\n", path.display())
    );
    assert_eq!(text(err), "");
}

#[test]
fn backup_all_prints_one_line_per_repo() {
    let data = scratch("all");
    let backups = scratch("all-b");
    save(&data, "a/one");
    save(&data, "a/two");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = backup_with(
        &settings(&backups),
        &data,
        &words(&["--all"]),
        NOW,
        &fake_backup,
        &mut out,
        &mut err,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let lines = text(out);
    assert_eq!(lines.lines().count(), 2);
    assert!(lines.starts_with("a/one: backed up to "), "{lines}");
    assert!(lines.contains("\na/two: backed up to "), "{lines}");
}

#[test]
fn a_refused_backup_exits_1_with_the_text_and_adds_its_waiting_item() {
    let data = scratch("refused");
    let backups = scratch("refused-b");
    save(&data, "a/wipe");
    save(&data, "a/zed");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = backup_with(
        &settings(&backups),
        &data,
        &words(&["--all"]),
        NOW,
        &fake_backup,
        &mut out,
        &mut err,
    );
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(
        text(err),
        "a/wipe: the new backup has 12 open issues, the last had 40\n"
    );
    assert!(text(out).starts_with("a/zed: backed up to "));
    let items = LocalItems::new(&data).load().unwrap();
    assert!(items.iter().any(|item| item.source
        == Source::RefusedBackup {
            repo: "a/wipe".to_string()
        }));
}

#[test]
fn a_backup_that_errors_exits_1_after_the_others_ran() {
    let data = scratch("down");
    let backups = scratch("down-b");
    save(&data, "a/down");
    save(&data, "a/up");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = backup_with(
        &settings(&backups),
        &data,
        &words(&["--all"]),
        NOW,
        &fake_backup,
        &mut out,
        &mut err,
    );
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(text(err), "a/down: failed: GitHub: down\n");
    assert!(text(out).starts_with("a/up: backed up to "));
}

#[test]
fn backup_with_no_target_is_usage_exit_2() {
    let data = scratch("usage");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = backup_with(
        &Settings::default(),
        &data,
        &[],
        NOW,
        &fake_backup,
        &mut out,
        &mut err,
    );
    assert_eq!(code, ExitCode::from(2));
    assert!(text(err).contains("usage: switchboard"));
}

#[test]
fn the_binary_exits_2_on_backup_alone_and_on_refresh_with_a_repo() {
    for argv in [
        &["backup"][..],
        &["refresh", "a/one"][..],
        &["--all", "budget"][..],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_switchboard"))
            .args(argv)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{argv:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage: switchboard"));
    }
}
