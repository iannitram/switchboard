//! #426: `switchboard confirm restore <owner/name> --into <owner/new-name>`: the CLI confirms a
//! restore into a new private repo with the repo's name typed, through the same core function
//! the app calls. The writes here are fakes (no GitHub): they record what the CLI asked for.

// The binary crate has no library target, so the sources are compiled in directly.
#[path = "../src/main.rs"]
mod app;
#[path = "../src/cli.rs"]
#[allow(dead_code, clippy::duplicate_mod)]
mod cli;

#[allow(unused_imports)]
use app::{parse_args, Args};

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cli::{run_with, CliWrites, Writes};
use switchboard_core::rules::{Caller, Face, Role};
use switchboard_core::waiting::Item;

const NOW: i64 = 1_700_000_000;

#[derive(Default)]
struct Fake {
    confirmed: RefCell<Vec<(String, String, Caller)>>,
    answer: Option<Result<Vec<String>, String>>,
}

impl Writes for Fake {
    fn merge(&self, _: &str, _: u64) -> Result<(), String> {
        Err("no merge here".to_string())
    }
    fn waiting(&self, _: &Item) -> Result<(), String> {
        Ok(())
    }
    fn knows_repo(&self, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    fn confirm_restore_new(
        &self,
        typed: &str,
        into: &str,
        caller: &Caller,
    ) -> Result<Vec<String>, String> {
        self.confirmed
            .borrow_mut()
            .push((typed.to_string(), into.to_string(), caller.clone()));
        self.answer
            .clone()
            .unwrap_or_else(|| Ok(vec!["restore: done".to_string()]))
    }
}

fn folder(name: &str, job_worktree: bool) -> PathBuf {
    let base = std::env::temp_dir().join(format!("sb-cli-confirm-{}-{name}", std::process::id()));
    let dir = if job_worktree {
        base.join("switchboard-wt").join("fleet-12")
    } else {
        base.join("plain")
    };
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(argv: &[&str], cwd: &Path, writes: &dyn Writes) -> (ExitCode, String) {
    let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
    let mut err: Vec<u8> = Vec::new();
    let code = run_with(&argv, cwd, NOW, writes, &mut err);
    (code, String::from_utf8(err).unwrap())
}

#[test]
fn confirm_restore_into_asks_the_core_with_the_typed_name_and_no_role() {
    let fake = Fake::default();
    let (code, err) = run(
        &[
            "confirm",
            "restore",
            "o/alpha",
            "--into",
            "o/alpha-restored",
        ],
        &folder("ok", false),
        &fake,
    );
    assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
    {
        let calls = fake.confirmed.borrow();
        assert_eq!(calls.len(), 1);
        let (typed, into, caller) = &calls[0];
        assert_eq!(
            (typed.as_str(), into.as_str()),
            ("o/alpha", "o/alpha-restored")
        );
        // The typed name is the CLI's confirm; no role, and not inside a job worktree.
        assert_eq!(caller.face, Face::Cli);
        assert_eq!(caller.role, None);
        assert!(caller.confirmed_in_app && !caller.in_job_worktree);
    }
    // The flag may come first, as the other flags may.
    let (code, _) = run(
        &["--into", "o/b", "confirm", "restore", "o/alpha"],
        &folder("ok", false),
        &fake,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(fake.confirmed.borrow()[1].1, "o/b");
}

#[test]
fn a_role_and_a_job_worktree_reach_the_core_to_be_refused_there() {
    let fake = Fake::default();
    run(
        &[
            "--role",
            "project-manager",
            "confirm",
            "restore",
            "o/a",
            "--into",
            "o/b",
        ],
        &folder("role", false),
        &fake,
    );
    run(
        &["confirm", "restore", "o/a", "--into", "o/b"],
        &folder("wt", true),
        &fake,
    );
    let calls = fake.confirmed.borrow();
    assert_eq!(calls[0].2.role, Some(Role::ProjectManager));
    assert!(calls[1].2.in_job_worktree);
}

#[test]
fn a_refusal_is_printed_and_exits_1() {
    let fake = Fake {
        answer: Some(Err(
            "restore over o/a: type o/a to confirm; the name typed does not match, nothing was changed"
                .to_string(),
        )),
        ..Fake::default()
    };
    let (code, err) = run(
        &["confirm", "restore", "o/A", "--into", "o/b"],
        &folder("refused", false),
        &fake,
    );
    assert_eq!(code, ExitCode::from(1));
    assert!(err.contains("type o/a to confirm"), "{err}");
}

#[test]
fn confirm_without_restore_one_name_and_into_is_a_usage_error() {
    let fake = Fake::default();
    for argv in [
        &["confirm"][..],
        &["confirm", "restore"][..],
        &["confirm", "archive", "o/a", "--into", "o/b"][..],
        &["confirm", "restore", "o/a", "o/b", "--into", "o/c"][..],
        // Over the live repo is the app's.
        &["confirm", "restore", "o/a"][..],
        &["confirm", "restore", "o/a", "--into"][..],
        &["confirm", "restore", "o/a", "--into", "--json"][..],
    ] {
        let (code, err) = run(argv, &folder("usage", false), &fake);
        assert_eq!(code, ExitCode::from(2), "{argv:?}: {err}");
    }
    assert!(fake.confirmed.borrow().is_empty());
    // `--into` belongs to confirm.
    let (code, _) = run(
        &["restore", "o/a", "--into", "o/b"],
        &folder("usage", false),
        &fake,
    );
    assert_eq!(code, ExitCode::from(2));
}

#[test]
fn confirming_a_request_that_is_not_waiting_is_refused_and_changes_nothing() {
    let base = std::env::temp_dir().join(format!("sb-cli-confirm-real-{}", std::process::id()));
    let data = base.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let writes = CliWrites {
        data: Some(data.clone()),
        pipeline: None,
        dev_root: None,
        in_job_worktree: false,
        transport: None,
    };
    let (code, err) = run(
        &["confirm", "restore", "o/alpha", "--into", "o/new"],
        &folder("real", false),
        &writes,
    );
    assert_eq!(code, ExitCode::from(1));
    assert!(err.contains("no such request is waiting on you"), "{err}");
    assert!(!data.join("waiting").exists());
}
