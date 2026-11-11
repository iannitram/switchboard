//! #347, turn a request down from the CLI: `switchboard drop <delete | restore> <repo>` removes
//! the request that command left in Waiting on you (and a held restore's record) and does
//! nothing else. `CliWrites` runs in-process through `run_with` over a temp app data folder;
//! the transport is a `Recorded` with no answers and the pipeline folder is empty, so a drop
//! that reached GitHub or the create queue would fail.

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
use std::process::ExitCode;

use cli::{run_with, CliWrites};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::Recorded;
use switchboard_core::github::RepoSnapshot;
use switchboard_core::waiting::{LocalItems, Source};

const NOW: i64 = 1_700_000_000;

struct Setup {
    data: PathBuf,
    pipeline: PathBuf,
    plain: PathBuf,
    job: PathBuf,
}

/// A data folder that knows `o/switchboard`, a plain folder and a job worktree.
fn setup(tag: &str) -> Setup {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("sb-drop-{tag}-{}-{nanos}", std::process::id()));
    let s = Setup {
        data: base.join("data"),
        pipeline: base.join("pipeline"),
        plain: base.join("plain"),
        job: base.join("switchboard-wt").join("fleet-12"),
    };
    for dir in [&s.data, &s.pipeline, &s.plain, &s.job] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let snap = RepoSnapshot {
        repo: "o/switchboard".to_string(),
        read_at: 2000,
        ..Default::default()
    };
    SnapshotStore::new(&s.data).save(&snap).unwrap();
    s
}

fn writes(s: &Setup) -> CliWrites {
    CliWrites {
        data: Some(s.data.clone()),
        pipeline: Some(s.pipeline.clone()),
        dev_root: Some(s.plain.clone()),
        in_job_worktree: false,
        transport: Some(Box::new(Recorded::new(vec![], vec![]))),
    }
}

fn run_in(argv: &[&str], cwd: &Path, s: &Setup) -> (ExitCode, String) {
    let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
    let mut err: Vec<u8> = Vec::new();
    let code = run_with(&argv, cwd, NOW, &writes(s), &mut err);
    (code, String::from_utf8(err).unwrap())
}

fn run(argv: &[&str], s: &Setup) -> (ExitCode, String) {
    run_in(argv, &s.plain, s)
}

/// The texts of the requests waiting now.
fn waiting(s: &Setup) -> Vec<String> {
    LocalItems::new(&s.data)
        .load()
        .unwrap()
        .into_iter()
        .map(|item| match item.source {
            Source::GuardedAction { text } => text,
            other => panic!("{other:?}"),
        })
        .collect()
}

const ARCHIVE: &str = "I was asked from the CLI to archive o/switchboard";
const RESTORE: &str = "I was asked from the CLI to restore over o/switchboard";

#[test]
fn drop_removes_the_request_the_command_of_that_name_left() {
    let s = setup("both");
    run(&["delete", "switchboard"], &s);
    run(&["restore", "switchboard"], &s);
    assert_eq!(waiting(&s), [ARCHIVE, RESTORE]);

    let (code, err) = run(&["drop", "delete", "switchboard"], &s);
    assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
    assert_eq!(waiting(&s), [RESTORE], "the restore request stays");

    // By `owner/name` too; then nothing waits.
    let (code, err) = run(&["drop", "restore", "o/switchboard"], &s);
    assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
    assert_eq!(waiting(&s), Vec::<String>::new());
}

#[test]
fn dropping_what_is_not_waiting_is_refused_and_changes_nothing() {
    let s = setup("none");
    run(&["restore", "switchboard"], &s);
    let before = std::fs::read_to_string(LocalItems::new(&s.data).path()).unwrap();
    let (code, err) = run(&["drop", "delete", "switchboard"], &s);
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(
        err,
        "archive o/switchboard: no such request is waiting on you; nothing was dropped\n"
    );
    // A bare name Switchboard does not know names no repo; an `owner/name` is taken as written.
    let (code, err) = run(&["drop", "restore", "nope"], &s);
    assert_eq!(
        (code, err.as_str()),
        (ExitCode::from(1), "switchboard: no such repo: nope\n")
    );
    let (code, err) = run(&["drop", "restore", "o/gone"], &s);
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(
        err,
        "restore over o/gone: no such request is waiting on you; nothing was dropped\n"
    );
    let after = std::fs::read_to_string(LocalItems::new(&s.data).path()).unwrap();
    assert_eq!(after, before);
}

#[test]
fn drop_inside_a_job_worktree_is_refused_whatever_the_role() {
    let s = setup("worktree");
    run(&["delete", "switchboard"], &s);
    let (code, err) = run_in(
        &["--role", "project-manager", "drop", "delete", "switchboard"],
        &s.job,
        &s,
    );
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(
        err,
        "drop the request to archive o/switchboard: refused inside a job worktree\n"
    );
    assert_eq!(waiting(&s), [ARCHIVE]);
}

#[test]
fn drop_without_a_kind_and_one_repo_is_a_usage_error() {
    let s = setup("usage");
    run(&["delete", "switchboard"], &s);
    for argv in [
        &["drop"][..],
        &["drop", "switchboard"][..],
        &["drop", "merge", "switchboard"][..],
        &["drop", "delete"][..],
        &["drop", "delete", "switchboard", "more"][..],
    ] {
        let (code, err) = run(argv, &s);
        assert_eq!(code, ExitCode::from(2), "{argv:?}");
        assert!(
            err.starts_with("switchboard: drop needs <delete | restore> <repo>\nusage: "),
            "{err}"
        );
    }
    assert_eq!(waiting(&s), [ARCHIVE]);
}

/// `switchboard restore` again, while the first confirm's held pass waits, leaves that item as
/// it is: filed afresh, the next confirm would push the refs over the live repo again.
#[test]
fn asking_again_leaves_a_held_restore_as_it_is() {
    let s = setup("held-again");
    run(&["restore", "switchboard"], &s);
    let local = LocalItems::new(&s.data);
    let mut held = local.load().unwrap().remove(0);
    held.action = switchboard_core::confirm::HELD_ACTION.to_string();
    held.since = NOW - 60;
    local.add(held.clone()).unwrap();
    run(&["restore", "o/switchboard"], &s);
    assert_eq!(local.load().unwrap(), [held]);
}
