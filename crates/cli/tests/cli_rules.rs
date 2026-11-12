//! CLI-3: `delete`, `restore` and `merge` through `rules::guard`, with the same cases as MCP-2 and
//! MCP-3. The command runs in-process against fake writes, a fixed clock and a folder that is or
//! is not a job worktree.

// The binary crate has no library target, so the sources are compiled in directly.
#[path = "../src/main.rs"]
mod app;
// `cli.rs` reads `super::parse_args`, so it sits at this root beside the parser's import; main.rs
// loads the same file inside `app`.
#[path = "../src/cli.rs"]
#[allow(dead_code, clippy::duplicate_mod)]
mod cli;

#[allow(unused_imports)]
use app::{parse_args, Args};

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cli::{repo_known, run_with, Writes};
use switchboard_core::waiting::{Item, Source};

const NOW: i64 = 1_700_000_000;

/// Records what reached it. `refuse` makes `merge` answer with that text, as GH-5 would for a
/// missing PASS on head or a red CI.
#[derive(Default)]
struct Fake {
    refuse: Option<&'static str>,
    merged: RefCell<Vec<(String, u64)>>,
    waiting: RefCell<Vec<Item>>,
    /// `knows_repo` answers false when set; `asked` records each repo it was asked about.
    unknown: bool,
    asked: RefCell<Vec<String>>,
}

impl Writes for Fake {
    fn merge(&self, repo: &str, pr: u64) -> Result<(), String> {
        if let Some(text) = self.refuse {
            return Err(text.to_string());
        }
        self.merged.borrow_mut().push((repo.to_string(), pr));
        Ok(())
    }

    fn waiting(&self, item: &Item) -> Result<(), String> {
        self.waiting.borrow_mut().push(item.clone());
        Ok(())
    }

    fn knows_repo(&self, repo: &str) -> Result<bool, String> {
        self.asked.borrow_mut().push(repo.to_string());
        Ok(!self.unknown)
    }
}

/// Run one command in `cwd`; returns the exit code and what it wrote to stderr.
fn run(argv: &[&str], cwd: &Path, fake: &Fake) -> (ExitCode, String) {
    let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    let mut err: Vec<u8> = Vec::new();
    let code = run_with(&argv, cwd, NOW, fake, &mut err);
    (code, String::from_utf8(err).expect("utf-8 stderr"))
}

/// A folder for one test: a plain one, or a job worktree (`fleet-*` under a `*-wt` folder).
fn folder(name: &str, job_worktree: bool) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "switchboard-cli-rules-{}-{name}",
        std::process::id()
    ));
    let dir = if job_worktree {
        base.join("switchboard-wt").join("fleet-12")
    } else {
        base.join("plain")
    };
    std::fs::create_dir_all(&dir).expect("create the test folder");
    dir
}

#[test]
fn delete_over_a_live_repo_is_refused_and_adds_one_waiting_item() {
    let fake = Fake::default();
    let (code, err) = run(&["delete", "switchboard"], &folder("delete", false), &fake);
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(err, "archive switchboard: needs a confirm in the app\n");
    let waiting = fake.waiting.borrow();
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        waiting[0].source,
        Source::GuardedAction {
            text: "I was asked from the CLI to archive switchboard".to_string()
        }
    );
    assert!(fake.merged.borrow().is_empty());
}

#[test]
fn restore_over_a_live_repo_is_refused_and_adds_one_waiting_item() {
    let fake = Fake::default();
    let (code, err) = run(
        &["restore", "switchboard"],
        &folder("restore", false),
        &fake,
    );
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(
        err,
        "restore over switchboard: needs a confirm in the app\n"
    );
    let waiting = fake.waiting.borrow();
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        waiting[0].source,
        Source::GuardedAction {
            text: "I was asked from the CLI to restore over switchboard".to_string()
        }
    );
}

#[test]
fn project_manager_merge_with_a_pass_and_green_ci_merges() {
    let fake = Fake::default();
    let (code, err) = run(
        &["--role", "project-manager", "merge", "switchboard#12"],
        &folder("merge-ok", false),
        &fake,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(err, "");
    assert_eq!(*fake.merged.borrow(), vec![("switchboard".to_string(), 12)]);
}

#[test]
fn project_manager_merge_without_a_pass_on_head_is_refused_with_the_merge_text() {
    let fake = Fake {
        refuse: Some("switchboard#12: no PASS on head"),
        ..Default::default()
    };
    let (code, err) = run(
        &["--role", "project-manager", "merge", "switchboard#12"],
        &folder("merge-no-pass", false),
        &fake,
    );
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(err, "switchboard#12: no PASS on head\n");
    assert!(fake.merged.borrow().is_empty());
}

#[test]
fn project_manager_merge_with_ci_not_green_is_refused_with_the_merge_text() {
    let fake = Fake {
        refuse: Some("CI on the head is failure"),
        ..Default::default()
    };
    let (code, err) = run(
        &["--role", "project-manager", "merge", "switchboard#12"],
        &folder("merge-ci", false),
        &fake,
    );
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(err, "CI on the head is failure\n");
    assert!(fake.merged.borrow().is_empty());
}

#[test]
fn merge_without_a_role_flag_is_refused_and_merges_nothing() {
    let fake = Fake::default();
    let (code, err) = run(
        &["merge", "switchboard#12"],
        &folder("merge-none", false),
        &fake,
    );
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(
        err,
        "merge switchboard#12: only the project-manager role may merge\n"
    );
    assert!(fake.merged.borrow().is_empty());
}

#[test]
fn budget_manager_merge_is_refused_and_merges_nothing() {
    let fake = Fake::default();
    let (code, err) = run(
        &["--role", "budget-manager", "merge", "switchboard#12"],
        &folder("merge-budget", false),
        &fake,
    );
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(
        err,
        "merge switchboard#12: only the project-manager role may merge\n"
    );
    assert!(fake.merged.borrow().is_empty());
}

#[test]
fn role_merge_inside_a_job_worktree_is_refused_with_the_worktree_text() {
    let fake = Fake::default();
    let (code, err) = run(
        &["--role", "project-manager", "merge", "switchboard#12"],
        &folder("merge-worktree", true),
        &fake,
    );
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(err, "merge switchboard#12: refused inside a job worktree\n");
    assert!(fake.merged.borrow().is_empty());
}

#[test]
fn delete_inside_a_job_worktree_is_refused_and_adds_no_waiting_item() {
    let fake = Fake::default();
    let (code, err) = run(
        &["--role", "project-manager", "delete", "switchboard"],
        &folder("delete-worktree", true),
        &fake,
    );
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(err, "archive switchboard: refused inside a job worktree\n");
    assert!(fake.waiting.borrow().is_empty());
}

#[test]
fn unknown_role_exits_2() {
    let fake = Fake::default();
    let (code, err) = run(
        &["--role", "wizard", "merge", "switchboard#12"],
        &folder("unknown-role", false),
        &fake,
    );
    assert_eq!(code, ExitCode::from(2));
    assert_eq!(err, "switchboard: unknown role: wizard\n");
    assert!(fake.merged.borrow().is_empty());
}

#[test]
fn delete_and_restore_of_an_unknown_repo_exit_1_and_write_nothing() {
    for command in ["delete", "restore"] {
        let fake = Fake {
            unknown: true,
            ..Default::default()
        };
        let (code, err) = run(&[command, "nope"], &folder("unknown", false), &fake);
        assert_eq!(code, ExitCode::from(1), "{command}");
        assert_eq!(err, "switchboard: no such repo: nope\n", "{command}");
        assert!(fake.waiting.borrow().is_empty(), "{command}");
    }
}

#[test]
fn merge_never_asks_whether_the_repo_is_known() {
    let fake = Fake {
        unknown: true,
        ..Default::default()
    };
    let _ = run(
        &["--role", "project-manager", "merge", "switchboard#7"],
        &folder("merge-known", false),
        &fake,
    );
    assert!(fake.asked.borrow().is_empty());
    assert_eq!(fake.merged.borrow().len(), 1);
}

#[test]
fn repo_known_matches_owner_name_or_a_bare_name() {
    let known = vec!["o/r".to_string(), "iannitram/switchboard".to_string()];
    assert!(repo_known("o/r", &known));
    assert!(repo_known("switchboard", &known));
    assert!(!repo_known("o/switchboard", &known));
    assert!(!repo_known("witch", &known));
    assert!(!repo_known("x/r", &known));
    assert!(!repo_known("r", &[]));
}

/// #347: the app's confirm step reads the request back from the item the CLI leaves, so the
/// two wordings must stay one.
#[test]
fn the_app_reads_the_delete_and_restore_requests_back_from_the_cli_items() {
    use switchboard_core::confirm::guarded_request;
    use switchboard_core::rules::GuardedAction;
    let fake = Fake::default();
    let cwd = folder("confirm-roundtrip", false);
    let _ = run(&["delete", "owner/alpha"], &cwd, &fake);
    let _ = run(&["restore", "owner/alpha"], &cwd, &fake);
    let repo = "owner/alpha".to_string();
    let waiting = fake.waiting.borrow();
    assert_eq!(
        waiting.iter().map(guarded_request).collect::<Vec<_>>(),
        vec![
            Some(GuardedAction::DeleteRepo { repo: repo.clone() }),
            Some(GuardedAction::RestoreOverLive { repo }),
        ]
    );
}
