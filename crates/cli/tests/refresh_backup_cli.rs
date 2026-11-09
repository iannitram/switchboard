//! Locked tests for CLI-2 (`switchboard refresh`, `backup <repo>`, `backup --all`). Qwen never
//! edits this file (R28). `refresh` runs the real `CliWrites` in-process through `run_io` over
//! a temp app data folder and a fake GitHub transport (GH-2's 304 answers and a failing repo).
//! `backup` runs through a `Writes` fake built on the core's own seams: `backup_repo_from` /
//! `backup_all_from` against a folder of local bare repos and recorded API answers, then
//! `backup_run::record`, the same calls `run_one` / `run_all` make around the real GitHub.

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

use cli::{run_io, CliWrites, Writes};
use serde_json::{json, Value};
use switchboard_core::backup::{backup_all_from, backup_repo_from, Remote};
use switchboard_core::backup_run::{self, BackupRow};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, Recorded, RestAnswer};
use switchboard_core::github::RepoSnapshot;
use switchboard_core::settings::Settings;
use switchboard_core::waiting::{Item, LocalItems, Source};
use switchboard_core::CoreError;

const NOW: i64 = 1_794_139_200;
const TAKEN_AT: &str = "2026-11-08T12:00:00Z";

fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-cli2-{tag}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn save(data: &Path, repo: &str, etag: Option<&str>) {
    SnapshotStore::new(data)
        .save(&RepoSnapshot {
            repo: repo.to_string(),
            read_at: NOW - 600,
            etag: etag.map(str::to_string),
            ..Default::default()
        })
        .unwrap();
}

/// Run one command in-process; the exit code, stdout and stderr.
fn run(argv: &[&str], w: &dyn Writes) -> (ExitCode, String, String) {
    let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
    let code = run_io(&argv, Path::new("."), NOW, w, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

// --- refresh ---

/// Answers `/repos/<repo>/events` with 304 (ETag `e2`) for the repos in `unchanged` and 200
/// otherwise; GraphQL fails for the repos in `fail` and answers an empty repository otherwise.
struct Hub {
    unchanged: Vec<&'static str>,
    fail: Vec<&'static str>,
    log: Mutex<Vec<String>>,
}

impl GitHubTransport for Hub {
    fn graphql(&self, _query: &str, variables: &Value) -> Result<Value, CoreError> {
        let repo = format!(
            "{}/{}",
            variables["owner"].as_str().unwrap_or("?"),
            variables["name"].as_str().unwrap_or("?")
        );
        self.log.lock().unwrap().push(format!("graphql {repo}"));
        if self.fail.contains(&repo.as_str()) {
            return Err(CoreError::GitHub("boom".to_string()));
        }
        Ok(json!({ "repository": { "labels": { "nodes": [] } } }))
    }

    fn rest_get(&self, path: &str, etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("GET {path} {}", etag.unwrap_or("-")));
        let repo = path
            .strip_prefix("/repos/")
            .and_then(|p| p.strip_suffix("/events"))
            .unwrap_or("");
        if self.unchanged.contains(&repo) {
            return Ok(RestAnswer {
                status: 304,
                etag: Some("e2".to_string()),
                body: Value::Null,
            });
        }
        Ok(RestAnswer {
            status: 200,
            etag: None,
            body: json!([]),
        })
    }
}

fn writes(data: &Path, hub: Hub) -> CliWrites {
    CliWrites {
        data: Some(data.to_path_buf()),
        pipeline: None,
        dev_root: None,
        in_job_worktree: false,
        transport: Some(Box::new(hub)),
    }
}

#[test]
fn refresh_prints_one_line_per_repo_and_skips_the_unchanged_read() {
    let data = scratch("refresh");
    save(&data, "o/a", Some("e1"));
    save(&data, "o/b", None);
    let hub = Hub {
        unchanged: vec!["o/a"],
        fail: vec![],
        log: Mutex::new(Vec::new()),
    };
    let (code, out, err) = run(&["refresh"], &writes(&data, hub));
    assert_eq!(err, "");
    assert_eq!(out, "o/a: unchanged\no/b: refreshed\n");
    assert_eq!(code, ExitCode::SUCCESS);
    let store = SnapshotStore::new(&data);
    let a = store.load("o/a").unwrap().unwrap();
    assert_eq!((a.read_at, a.etag.as_deref()), (NOW, Some("e2")));
    assert_eq!(store.load("o/b").unwrap().unwrap().read_at, NOW);
}

#[test]
fn refresh_sends_no_graphql_read_for_an_unchanged_repo() {
    let data = scratch("refresh-log");
    save(&data, "o/a", Some("e1"));
    save(&data, "o/b", None);
    let hub = Hub {
        unchanged: vec!["o/a"],
        fail: vec![],
        log: Mutex::new(Vec::new()),
    };
    let w = writes(&data, hub);
    run(&["refresh"], &w);
    let (_, out, _) = run(&["refresh", "--json"], &w);
    let rows: Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(rows[0]["outcome"], "unchanged");
    assert_eq!(rows[1]["outcome"], "refreshed");
}

#[test]
fn a_failing_repo_is_on_stderr_and_exit_1_after_the_others_ran() {
    let data = scratch("refresh-fail");
    save(&data, "o/a", None);
    save(&data, "o/b", None);
    save(&data, "o/c", None);
    let hub = Hub {
        unchanged: vec![],
        fail: vec!["o/b"],
        log: Mutex::new(Vec::new()),
    };
    let (code, out, err) = run(&["refresh"], &writes(&data, hub));
    assert_eq!(out, "o/a: refreshed\no/c: refreshed\n");
    assert_eq!(err, "o/b: failed: GitHub: boom\n");
    assert_eq!(code, ExitCode::from(1));
}

#[test]
fn refresh_with_no_snapshot_says_so() {
    let data = scratch("refresh-none");
    let hub = Hub {
        unchanged: vec![],
        fail: vec![],
        log: Mutex::new(Vec::new()),
    };
    let (code, out, err) = run(&["refresh"], &writes(&data, hub));
    assert_eq!(out, "");
    assert_eq!(
        err,
        "switchboard: no snapshot yet; run switchboard refresh\n"
    );
    assert_eq!(code, ExitCode::from(1));
}

// --- backup ---

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Switchboard Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=master",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A bare repo at `<root>/server/<repo>.git` with one commit.
fn serve(root: &Path, repo: &str) {
    let work = root.join(format!("work-{}", repo.replace('/', "-")));
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "master"]);
    std::fs::write(work.join("a.txt"), "a\n").unwrap();
    git(&work, &["add", "a.txt"]);
    git(&work, &["commit", "-q", "-m", "first"]);
    let bare = root.join("server").join(format!("{repo}.git"));
    std::fs::create_dir_all(bare.parent().unwrap()).unwrap();
    git(
        root,
        &[
            "clone",
            "-q",
            "--bare",
            &work.to_string_lossy(),
            &bare.to_string_lossy(),
        ],
    );
}

fn ok(body: Value) -> RestAnswer {
    RestAnswer {
        status: 200,
        etag: None,
        body,
    }
}

/// The answers for one backup, in the order it asks: issues, pulls, comments, labels,
/// milestones; `open` open issues and `closed` closed ones.
fn answers(open: u64, closed: u64) -> Vec<RestAnswer> {
    let issue = |n: u64, state: &str| json!({"number": n, "state": state, "labels": []});
    let mut issues: Vec<Value> = (1..=open).map(|n| issue(n, "open")).collect();
    issues.extend((open + 1..=open + closed).map(|n| issue(n, "closed")));
    vec![
        ok(Value::Array(issues)),
        ok(json!([])),
        ok(json!([])),
        ok(json!([{"name": "bug"}])),
        ok(json!([])),
    ]
}

/// The CLI-2 backup writes over the core's seams: local bare repos under `<root>/server`,
/// recorded API answers, backups under `<root>/backups`, the app data folder `<root>/data`.
struct Backups {
    root: PathBuf,
    api: Recorded,
}

impl Backups {
    fn new(tag: &str, rest: Vec<RestAnswer>) -> Self {
        let root = scratch(tag);
        std::fs::create_dir_all(root.join("backups")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        Self {
            root,
            api: Recorded::new(vec![], rest),
        }
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn settings(&self) -> Settings {
        Settings {
            nextcloud_backups: Some(self.root.join("backups")),
            ..Settings::default()
        }
    }

    fn remote(&self) -> Remote<'_> {
        Remote {
            git_base: self.root.join("server").to_string_lossy().into_owned(),
            git_config: Vec::new(),
            api: &self.api,
        }
    }

    fn folder(&self, repo: &str) -> String {
        backup_run::folder(&self.root.join("backups"), repo)
            .display()
            .to_string()
    }
}

impl Writes for Backups {
    fn merge(&self, repo: &str, pr: u64) -> Result<(), String> {
        Err(format!("merge {repo}#{pr}: not here"))
    }

    fn waiting(&self, _item: &Item) -> Result<(), String> {
        Ok(())
    }

    fn backup(&self, repo: &str, now: i64) -> Result<BackupRow, String> {
        let outcome = backup_repo_from(&self.settings(), repo, &self.remote(), TAKEN_AT)
            .map_err(|e| format!("backup {repo}: {e}"))?;
        backup_run::record(
            &self.data(),
            &self.root.join("backups"),
            repo,
            &outcome,
            now,
        )
        .map_err(|e| format!("backup {repo}: {e}"))
    }

    fn backup_all(&self, now: i64) -> Result<Vec<BackupRow>, String> {
        let names = backup_run::repos(&self.data()).map_err(|e| format!("backup --all: {e}"))?;
        backup_all_from(&self.settings(), &names, &self.remote(), TAKEN_AT)
            .map_err(|e| format!("backup --all: {e}"))?
            .iter()
            .map(|(repo, outcome)| {
                backup_run::record(&self.data(), &self.root.join("backups"), repo, outcome, now)
                    .map_err(|e| format!("backup {repo}: {e}"))
            })
            .collect()
    }
}

#[test]
fn backup_one_repo_prints_the_path_line() {
    let b = Backups::new("backup-one", answers(40, 2));
    serve(&b.root, "o/r");
    save(&b.data(), "o/r", None);
    let (code, out, err) = run(&["backup", "o/r"], &b);
    assert_eq!(err, "");
    assert_eq!(out, format!("o/r: backed up to {}\n", b.folder("o/r")));
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(Path::new(&b.folder("o/r")).join("manifest.json").is_file());
    assert!(LocalItems::new(&b.data()).load().unwrap().is_empty());
}

#[test]
fn a_refused_backup_exits_1_with_the_refusal_text_and_its_waiting_item() {
    let b = Backups::new("backup-refused", [answers(40, 2), answers(10, 2)].concat());
    serve(&b.root, "o/r");
    save(&b.data(), "o/r", None);
    let (code, _, err) = run(&["backup", "o/r"], &b);
    assert_eq!(code, ExitCode::SUCCESS, "first backup: {err}");
    let (code, out, err) = run(&["backup", "o/r"], &b);
    assert_eq!(out, "");
    assert!(
        err.starts_with("o/r: refused: the new backup has "),
        "stderr: {err}"
    );
    assert_eq!(code, ExitCode::from(1));
    let items = LocalItems::new(&b.data()).load().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].source,
        Source::RefusedBackup {
            repo: "o/r".to_string()
        }
    );
    assert_eq!(items[0].since, NOW);
}

#[test]
fn backup_all_prints_one_line_per_repo_in_store_order() {
    let b = Backups::new("backup-all", [answers(3, 1), answers(2, 0)].concat());
    serve(&b.root, "o/r");
    serve(&b.root, "o/s");
    save(&b.data(), "o/s", None);
    save(&b.data(), "o/r", None);
    let (code, out, err) = run(&["backup", "--all"], &b);
    assert_eq!(err, "");
    assert_eq!(
        out,
        format!(
            "o/r: backed up to {}\no/s: backed up to {}\n",
            b.folder("o/r"),
            b.folder("o/s")
        )
    );
    assert_eq!(code, ExitCode::SUCCESS);
}

#[test]
fn backup_with_neither_a_repo_nor_all_exits_2_with_usage() {
    let b = Backups::new("backup-usage", vec![]);
    let (code, out, err) = run(&["backup"], &b);
    assert_eq!(out, "");
    assert!(err.starts_with("switchboard: backup needs one <repo> or --all\nusage: switchboard"));
    assert_eq!(code, ExitCode::from(2));
    let (code, _, err) = run(&["backup", "o/r", "--all"], &b);
    assert!(err.starts_with("switchboard: backup needs one <repo> or --all\n"));
    assert_eq!(code, ExitCode::from(2));
    let (code, _, err) = run(&["refresh", "--all"], &b);
    assert_eq!(err.lines().next(), Some("switchboard: --all is for backup"));
    assert_eq!(code, ExitCode::from(2));
}

/// The built binary with `SWITCHBOARD_DATA` set to an app data folder with no settings file.
fn switchboard(argv: &[&str], data: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(argv)
        .current_dir(data)
        .env("SWITCHBOARD_DATA", data)
        .output()
        .expect("run switchboard")
}

#[test]
fn the_binary_backup_alone_exits_2_and_without_the_setting_exits_1() {
    let data = scratch("bin");
    let out = switchboard(&["backup"], &data);
    assert_eq!(out.status.code(), Some(2));
    let out = switchboard(&["backup", "o/r"], &data);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "backup o/r: setting not set: nextcloud_backups\n"
    );
}
