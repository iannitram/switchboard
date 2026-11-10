//! CORE-3, BAK-3 (#347): the confirm step in the app for a guarded request the CLI or MCP left
//! in Waiting on you. Refused without the confirm, refused from any face but the app whatever
//! the role, run with it, and a refusal changes nothing. The restore itself is a closure here
//! (it is tested against a fixture GitHub in `restore_issues.rs`); no test reaches GitHub.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use switchboard_core::backup::{IssuesReport, RestoreReport};
use switchboard_core::confirm::{confirm_with, guarded_request, Done};
use switchboard_core::rules::{Caller, Face, GuardedAction, Role};
use switchboard_core::settings::Settings;
use switchboard_core::waiting::{guarded_item, Item, LocalItems, Source};
use switchboard_core::CoreError;

const NOW: i64 = 1_794_121_200;
const REPO: &str = "owner/alpha";

fn data_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-confirm-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn delete() -> GuardedAction {
    GuardedAction::DeleteRepo { repo: REPO.into() }
}

fn restore() -> GuardedAction {
    GuardedAction::RestoreOverLive { repo: REPO.into() }
}

/// The item the CLI leaves (`cli::write_command`): its text, and the guard's wording after "to".
fn asked(action: &GuardedAction) -> Item {
    let request = match action {
        GuardedAction::DeleteRepo { repo } => format!("delete {repo}"),
        GuardedAction::RestoreOverLive { repo } => format!("restore over {repo}"),
        GuardedAction::Merge { repo, pr } => format!("merge {repo}#{pr}"),
    };
    guarded_item(
        action,
        &format!("I was asked from the CLI to {request}"),
        NOW,
    )
}

/// A data folder holding the delete and the restore request, and its `local.json` text.
fn seeded(name: &str) -> (PathBuf, String) {
    let data = data_dir(name);
    let local = LocalItems::new(&data);
    local.add(asked(&delete())).unwrap();
    local.add(asked(&restore())).unwrap();
    let text = std::fs::read_to_string(local.path()).unwrap();
    (data, text)
}

fn unchanged(data: &Path, before: &str) {
    let now = std::fs::read_to_string(LocalItems::new(data).path()).unwrap();
    assert_eq!(now, before, "a refusal leaves Waiting on you as it was");
}

fn caller(face: Face, role: Option<Role>, in_wt: bool, confirmed: bool) -> Caller {
    Caller {
        face,
        role,
        in_job_worktree: in_wt,
        confirmed_in_app: confirmed,
    }
}

fn text_of(item: &Item) -> String {
    match &item.source {
        Source::GuardedAction { text } => text.clone(),
        other => panic!("not a guarded request: {other:?}"),
    }
}

/// A restore that records who asked and for which repo, and answers `report`.
struct Restore {
    calls: RefCell<Vec<(Caller, String)>>,
    report: Result<RestoreReport, CoreError>,
}

impl Restore {
    fn answering(report: Result<RestoreReport, CoreError>) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            report,
        }
    }

    fn run(&self, data: &Path, text: &str, who: &Caller) -> Result<Done, CoreError> {
        confirm_with(data, text, who, &|caller, repo| {
            self.calls
                .borrow_mut()
                .push((caller.clone(), repo.to_string()));
            self.report.clone()
        })
    }
}

fn report() -> RestoreReport {
    RestoreReport {
        refs: 3,
        labels: 2,
        milestones: 1,
        issues: IssuesReport {
            issues: 2,
            comments: 1,
            ..IssuesReport::default()
        },
        left_out: vec!["the wiki is not restored".to_string()],
    }
}

fn refused(result: Result<Done, CoreError>) -> String {
    match result {
        Err(CoreError::Refused(why)) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn the_request_is_read_back_from_the_item_the_cli_leaves() {
    assert_eq!(guarded_request(&asked(&delete())), Some(delete()));
    assert_eq!(guarded_request(&asked(&restore())), Some(restore()));
}

#[test]
fn an_item_that_is_not_one_clear_delete_or_restore_request_is_no_request() {
    // Not a guarded request at all.
    let mut other = asked(&delete());
    other.source = Source::StalePriority;
    assert_eq!(guarded_request(&other), None);
    // A merge is never left waiting for a confirm.
    let merge = GuardedAction::Merge {
        repo: REPO.into(),
        pr: 4,
    };
    assert_eq!(guarded_request(&asked(&merge)), None);
    // The link must name the same repo as the text.
    let mut elsewhere = asked(&delete());
    elsewhere.link = "https://github.com/owner/beta".to_string();
    assert_eq!(guarded_request(&elsewhere), None);
    // A bare name, a path, or words after the repo name no repo.
    for repo in ["alpha", "a/b/c", "owner/alpha and owner/beta", "../x", ""] {
        let odd = GuardedAction::DeleteRepo { repo: repo.into() };
        assert_eq!(guarded_request(&asked(&odd)), None, "{repo:?}");
    }
}

#[test]
fn without_the_confirm_the_app_is_refused_and_nothing_changes() {
    let (data, before) = seeded("unconfirmed");
    let restore_run = Restore::answering(Ok(report()));
    let app = caller(Face::Gui, None, false, false);
    for (action, want) in [
        (delete(), "delete owner/alpha: needs a confirm in the app"),
        (
            restore(),
            "restore over owner/alpha: needs a confirm in the app",
        ),
    ] {
        let why = refused(restore_run.run(&data, &text_of(&asked(&action)), &app));
        assert_eq!(why, want);
    }
    assert!(restore_run.calls.borrow().is_empty(), "no restore ran");
    unchanged(&data, &before);
}

#[test]
fn the_cli_and_mcp_cannot_confirm_whatever_the_role() {
    let (data, before) = seeded("face");
    let restore_run = Restore::answering(Ok(report()));
    let roles = [
        None,
        Some(Role::ProjectManager),
        Some(Role::BudgetManager),
        Some(Role::Satellite),
    ];
    for face in [Face::Cli, Face::Mcp] {
        for role in &roles {
            // Even a caller that claims the confirm.
            let who = caller(face, role.clone(), false, true);
            let why = refused(restore_run.run(&data, &text_of(&asked(&restore())), &who));
            assert_eq!(
                why, "restore over owner/alpha: needs a confirm in the app",
                "{face:?} {role:?}"
            );
            let why = refused(restore_run.run(&data, &text_of(&asked(&delete())), &who));
            assert_eq!(why, "delete owner/alpha: needs a confirm in the app");
        }
    }
    assert!(restore_run.calls.borrow().is_empty(), "no restore ran");
    unchanged(&data, &before);
}

#[test]
fn inside_a_job_worktree_even_a_confirmed_app_is_refused() {
    let (data, before) = seeded("worktree");
    let restore_run = Restore::answering(Ok(report()));
    let who = caller(Face::Gui, None, true, true);
    let why = refused(restore_run.run(&data, &text_of(&asked(&restore())), &who));
    assert_eq!(
        why,
        "restore over owner/alpha: refused inside a job worktree"
    );
    assert!(restore_run.calls.borrow().is_empty());
    unchanged(&data, &before);
}

#[test]
fn a_request_that_is_not_waiting_is_refused_and_nothing_changes() {
    let (data, before) = seeded("absent");
    let restore_run = Restore::answering(Ok(report()));
    let app = caller(Face::Gui, None, false, true);
    for text in [
        "I was asked from the CLI to restore over owner/beta",
        "restore over owner/alpha",
        "",
    ] {
        let why = refused(restore_run.run(&data, text, &app));
        assert!(why.contains("no such request"), "{why}");
    }
    assert!(restore_run.calls.borrow().is_empty());
    unchanged(&data, &before);
    // No Waiting file at all: refused, and none is made.
    let empty = data_dir("absent-empty");
    let why = refused(restore_run.run(&empty, &text_of(&asked(&restore())), &app));
    assert!(why.contains("no such request"), "{why}");
    assert!(!empty.exists());
}

#[test]
fn a_waiting_item_that_names_no_clear_request_is_refused() {
    let data = data_dir("unclear");
    let local = LocalItems::new(&data);
    let mut odd = asked(&restore());
    odd.link = String::new();
    local.add(odd.clone()).unwrap();
    let before = std::fs::read_to_string(local.path()).unwrap();
    let restore_run = Restore::answering(Ok(report()));
    let app = caller(Face::Gui, None, false, true);
    let why = refused(restore_run.run(&data, &text_of(&odd), &app));
    assert!(
        why.contains("does not name one repo to delete or restore over"),
        "{why}"
    );
    assert!(restore_run.calls.borrow().is_empty());
    unchanged(&data, &before);
}

#[test]
fn a_confirmed_restore_runs_once_for_its_repo_and_its_item_leaves_waiting() {
    let (data, _) = seeded("confirmed");
    let restore_run = Restore::answering(Ok(report()));
    let app = caller(Face::Gui, None, false, true);
    let done = restore_run
        .run(&data, &text_of(&asked(&restore())), &app)
        .unwrap();
    assert_eq!(
        *restore_run.calls.borrow(),
        vec![(app.clone(), REPO.to_string())]
    );
    assert_eq!(done.what, "restore over owner/alpha");
    assert!(done.cleared);
    assert_eq!(
        done.lines,
        vec![
            "3 refs pushed".to_string(),
            "2 labels and 1 milestones queued".to_string(),
            "2 issues and 1 comments queued".to_string(),
            "the wiki is not restored".to_string(),
        ]
    );
    // WAIT-3: the restore request is gone; the delete request is still there.
    let left = LocalItems::new(&data).load().unwrap();
    assert_eq!(left, vec![asked(&delete())]);
    // A second confirm finds no request and runs nothing.
    let why = refused(restore_run.run(&data, &text_of(&asked(&restore())), &app));
    assert!(why.contains("no such request"), "{why}");
    assert_eq!(restore_run.calls.borrow().len(), 1);
}

#[test]
fn a_restore_that_holds_comments_back_keeps_its_item_for_a_second_confirm() {
    let (data, before) = seeded("held");
    let mut held = report();
    held.issues.comments_waiting = 4;
    let restore_run = Restore::answering(Ok(held));
    let app = caller(Face::Gui, None, false, true);
    let done = restore_run
        .run(&data, &text_of(&asked(&restore())), &app)
        .unwrap();
    assert!(!done.cleared);
    assert!(
        done.lines
            .iter()
            .any(|l| l.starts_with("4 comments wait for their issues")),
        "{:?}",
        done.lines
    );
    unchanged(&data, &before);
}

#[test]
fn a_restore_that_fails_keeps_its_item() {
    let (data, before) = seeded("failed");
    let restore_run = Restore::answering(Err(CoreError::GitHub("push failed".to_string())));
    let app = caller(Face::Gui, None, false, true);
    let got = restore_run.run(&data, &text_of(&asked(&restore())), &app);
    assert_eq!(got, Err(CoreError::GitHub("push failed".to_string())));
    assert_eq!(restore_run.calls.borrow().len(), 1);
    unchanged(&data, &before);
}

#[test]
fn a_confirmed_delete_is_refused_because_no_repo_delete_is_built_and_nothing_changes() {
    let (data, before) = seeded("delete");
    let restore_run = Restore::answering(Ok(report()));
    let app = caller(Face::Gui, None, false, true);
    let why = refused(restore_run.run(&data, &text_of(&asked(&delete())), &app));
    assert_eq!(
        why,
        "delete owner/alpha: Switchboard has no repo delete yet; nothing was deleted"
    );
    assert!(restore_run.calls.borrow().is_empty());
    unchanged(&data, &before);
}

#[test]
fn the_apps_confirm_without_a_backups_folder_stops_before_any_restore() {
    let (data, before) = seeded("settings");
    let app = caller(Face::Gui, None, false, true);
    let got = switchboard_core::confirm::confirm(
        &Settings::default(),
        &data,
        &text_of(&asked(&restore())),
        &app,
    );
    assert_eq!(got, Err(CoreError::MissingPath("nextcloud_backups")));
    unchanged(&data, &before);
    // Unconfirmed, the guard answers first, with the same text as everywhere.
    let unconfirmed = caller(Face::Gui, None, false, false);
    let why = refused(switchboard_core::confirm::confirm(
        &Settings::default(),
        &data,
        &text_of(&asked(&restore())),
        &unconfirmed,
    ));
    assert_eq!(why, "restore over owner/alpha: needs a confirm in the app");
    unchanged(&data, &before);
}
