//! CORE-3, BAK-3 (#347): the confirm step in the app for a guarded request the CLI or MCP left
//! in Waiting on you. Refused without the confirm, refused from any face but the app whatever
//! the role, refused unless the repo's `owner/name` was typed letter for letter, run with both,
//! and a refusal changes nothing. A delete request archives the repo; a restore that held
//! something back is confirmed a second time for that only. The restore and the archive are
//! closures here (they are tested against a fixture GitHub in `restore_issues.rs`,
//! `restore_state.rs` and `gh_archive.rs`); no test reaches GitHub.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use switchboard_core::backup::{IssuesReport, Pass, RestoreReport};
use switchboard_core::confirm::{confirm_with, guarded_request, Actions, Done, HELD_ACTION};
use switchboard_core::github::archive::Archived;
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

/// The held pass of the request `asked` leaves: it changes only what was made since the request.
const HELD: Pass = Pass::Held { since: NOW };

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

/// A restore and an archive that record who asked, for which repo (and which pass), and answer
/// `report` and `archived`.
struct Restore {
    calls: RefCell<Vec<(Caller, String)>>,
    passes: RefCell<Vec<Pass>>,
    archives: RefCell<Vec<(Caller, String)>>,
    report: RefCell<Result<RestoreReport, CoreError>>,
    archived: Result<Archived, CoreError>,
}

impl Restore {
    fn answering(report: Result<RestoreReport, CoreError>) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            passes: RefCell::new(Vec::new()),
            archives: RefCell::new(Vec::new()),
            report: RefCell::new(report),
            archived: Ok(Archived::Now),
        }
    }

    /// Confirm `text` with the repo's name typed right.
    fn run(&self, data: &Path, text: &str, who: &Caller) -> Result<Done, CoreError> {
        self.run_typed(data, text, REPO, who)
    }

    fn run_typed(
        &self,
        data: &Path,
        text: &str,
        typed: &str,
        who: &Caller,
    ) -> Result<Done, CoreError> {
        confirm_with(
            data,
            text,
            typed,
            who,
            &Actions {
                restore: &|caller, repo, pass| {
                    self.calls
                        .borrow_mut()
                        .push((caller.clone(), repo.to_string()));
                    self.passes.borrow_mut().push(pass);
                    self.report.borrow().clone()
                },
                archive: &|caller, repo| {
                    self.archives
                        .borrow_mut()
                        .push((caller.clone(), repo.to_string()));
                    self.archived.clone()
                },
            },
        )
    }

    /// True when neither the restore nor the archive ran.
    fn idle(&self) -> bool {
        self.calls.borrow().is_empty() && self.archives.borrow().is_empty()
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
    assert!(restore_run.idle(), "no restore and no archive ran");
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
    assert!(restore_run.idle(), "no restore and no archive ran");
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
    assert!(restore_run.idle());
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
    assert!(restore_run.idle());
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
    assert!(restore_run.idle());
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

/// #347, owner decision 3: a restore that held comments back keeps its request, marked as held;
/// the second confirm runs only the held pass (no whole restore, so no ref push), and the
/// request leaves once nothing is held back.
#[test]
fn a_second_confirm_of_a_restore_that_held_comments_back_only_runs_the_held_pass() {
    let (data, _) = seeded("held");
    let mut held = report();
    held.issues.comments_waiting = 4;
    held.issues.state_waiting = 1;
    let restore_run = Restore::answering(Ok(held.clone()));
    let app = caller(Face::Gui, None, false, true);
    let text = text_of(&asked(&restore()));
    let done = restore_run.run(&data, &text, &app).unwrap();
    assert!(!done.cleared);
    assert_eq!(done.what, "restore over owner/alpha");
    assert!(
        done.lines.contains(
            &"4 comments and 1 closes or body changes wait: confirm again once the create \
              queue has made the issues"
                .to_string()
        ),
        "{:?}",
        done.lines
    );
    assert_eq!(*restore_run.passes.borrow(), vec![Pass::Whole]);
    // The request stays, in its place, marked as held and saying what waits.
    let items = LocalItems::new(&data).load().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], asked(&delete()));
    let kept = &items[1];
    assert_eq!(kept.action, HELD_ACTION);
    assert_eq!(
        kept.why,
        "The restore over owner/alpha pushed the backup's refs; 4 comments and 1 closes or \
         body changes wait. A confirm now only queues and sends those: it pushes nothing again \
         (CORE-3)."
    );
    let first = asked(&restore());
    assert_eq!(
        (&kept.what, &kept.source, &kept.link, kept.since),
        (&first.what, &first.source, &first.link, first.since)
    );

    // The second confirm, still held: only the held pass, and the request stays held.
    let done = restore_run.run(&data, &text, &app).unwrap();
    assert!(!done.cleared);
    assert_eq!(done.what, "restore over owner/alpha (what was held back)");
    assert_eq!(done.lines[0], "no ref was pushed again");
    assert!(!done.lines.iter().any(|l| l.contains("refs pushed")));
    assert_eq!(*restore_run.passes.borrow(), vec![Pass::Whole, HELD]);
    assert_eq!(
        LocalItems::new(&data).load().unwrap()[1].action,
        HELD_ACTION
    );

    // The third: nothing is held back any more, so the request leaves.
    *restore_run.report.borrow_mut() = Ok(RestoreReport {
        issues: IssuesReport {
            comments: 4,
            closed: 1,
            ..IssuesReport::default()
        },
        ..RestoreReport::default()
    });
    let done = restore_run.run(&data, &text, &app).unwrap();
    assert!(done.cleared);
    assert_eq!(
        done.lines,
        vec![
            "no ref was pushed again".to_string(),
            "4 comments queued".to_string(),
            "1 issues and 0 milestones closed, as in the backup".to_string(),
        ]
    );
    assert_eq!(
        *restore_run.passes.borrow(),
        vec![Pass::Whole, HELD, HELD],
        "the whole restore ran once only"
    );
    assert_eq!(
        LocalItems::new(&data).load().unwrap(),
        vec![asked(&delete())]
    );
    assert!(restore_run.archives.borrow().is_empty());
}

/// A held request is still refused like any other, and stays held.
#[test]
fn a_refused_second_confirm_runs_nothing_and_the_request_stays_held() {
    let (data, _) = seeded("held-refused");
    let mut held = report();
    held.issues.comments_waiting = 1;
    let restore_run = Restore::answering(Ok(held));
    let app = caller(Face::Gui, None, false, true);
    let text = text_of(&asked(&restore()));
    restore_run.run(&data, &text, &app).unwrap();
    let before = std::fs::read_to_string(LocalItems::new(&data).path()).unwrap();
    let unconfirmed = caller(Face::Gui, None, false, false);
    refused(restore_run.run(&data, &text, &unconfirmed));
    refused(restore_run.run(&data, &text, &caller(Face::Cli, None, false, true)));
    refused(restore_run.run(&data, &text, &caller(Face::Gui, None, true, true)));
    refused(restore_run.run_typed(&data, &text, "owner/alph", &app));
    // A held pass that fails keeps the request held too.
    *restore_run.report.borrow_mut() = Err(CoreError::GitHub("down".to_string()));
    assert!(restore_run.run(&data, &text, &app).is_err());
    assert_eq!(*restore_run.passes.borrow(), vec![Pass::Whole, HELD]);
    unchanged(&data, &before);
}

/// A new request from the CLI for the same repo writes the item afresh: its confirm is a whole
/// restore again, as the confirm step then says.
#[test]
fn a_new_request_for_a_held_restore_is_a_whole_restore_again() {
    let (data, _) = seeded("held-again");
    let mut held = report();
    held.issues.comments_waiting = 1;
    let restore_run = Restore::answering(Ok(held));
    let app = caller(Face::Gui, None, false, true);
    let text = text_of(&asked(&restore()));
    restore_run.run(&data, &text, &app).unwrap();
    LocalItems::new(&data).add(asked(&restore())).unwrap();
    restore_run.run(&data, &text, &app).unwrap();
    assert_eq!(*restore_run.passes.borrow(), vec![Pass::Whole, Pass::Whole]);
}

/// #347, owner decision 2: the repo's `owner/name` must be typed, letter for letter, for the
/// archive and for the restore (the held one too). A wrong or missing name runs nothing.
#[test]
fn a_confirm_without_the_repos_name_typed_right_is_refused_and_nothing_changes() {
    let (data, before) = seeded("typed");
    let restore_run = Restore::answering(Ok(report()));
    let app = caller(Face::Gui, None, false, true);
    for typed in [
        "",
        " ",
        "owner/alph",
        "owner/alphaa",
        "Owner/Alpha",
        "owner/alpha ",
        " owner/alpha",
        "alpha",
        "owner/beta",
        "https://github.com/owner/alpha",
    ] {
        let why = refused(restore_run.run_typed(&data, &text_of(&asked(&delete())), typed, &app));
        assert_eq!(
            why,
            "archive owner/alpha: type owner/alpha to confirm; the name typed does not match, \
             nothing was changed",
            "{typed:?}"
        );
        let why = refused(restore_run.run_typed(&data, &text_of(&asked(&restore())), typed, &app));
        assert_eq!(
            why,
            "restore over owner/alpha: type owner/alpha to confirm; the name typed does not \
             match, nothing was changed",
            "{typed:?}"
        );
    }
    assert!(restore_run.idle(), "no restore and no archive ran");
    unchanged(&data, &before);
    // The guard answers before the name is looked at: the right name does not help the CLI.
    let cli = caller(Face::Cli, Some(Role::ProjectManager), false, true);
    let why = refused(restore_run.run(&data, &text_of(&asked(&delete())), &cli));
    assert_eq!(why, "delete owner/alpha: needs a confirm in the app");
    assert!(restore_run.idle());
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

/// #347, owner decision 1: a confirmed delete request archives the repo, once, and its request
/// leaves Waiting on you. No restore runs.
#[test]
fn a_confirmed_delete_request_archives_the_repo_and_its_item_leaves_waiting() {
    let (data, _) = seeded("archive");
    let restore_run = Restore::answering(Ok(report()));
    let app = caller(Face::Gui, None, false, true);
    let done = restore_run
        .run(&data, &text_of(&asked(&delete())), &app)
        .unwrap();
    assert_eq!(
        done,
        Done {
            what: "archive owner/alpha".to_string(),
            lines: vec![
                "owner/alpha is archived on GitHub: read-only, nothing was deleted".to_string()
            ],
            cleared: true,
        }
    );
    assert_eq!(
        *restore_run.archives.borrow(),
        vec![(app.clone(), REPO.to_string())]
    );
    assert!(restore_run.calls.borrow().is_empty(), "no restore ran");
    // WAIT-3: the delete request is gone; the restore request is still there.
    assert_eq!(
        LocalItems::new(&data).load().unwrap(),
        vec![asked(&restore())]
    );
    // A second confirm finds no request and archives nothing.
    let why = refused(restore_run.run(&data, &text_of(&asked(&delete())), &app));
    assert!(why.contains("no such request"), "{why}");
    assert_eq!(restore_run.archives.borrow().len(), 1);
}

#[test]
fn a_repo_that_was_archived_already_says_so_and_its_item_leaves_waiting() {
    let (data, _) = seeded("archived-already");
    let mut restore_run = Restore::answering(Ok(report()));
    restore_run.archived = Ok(Archived::Already);
    let app = caller(Face::Gui, None, false, true);
    let done = restore_run
        .run(&data, &text_of(&asked(&delete())), &app)
        .unwrap();
    assert_eq!(
        done.lines,
        vec!["owner/alpha was archived already; nothing was changed".to_string()]
    );
    assert!(done.cleared);
    assert_eq!(
        LocalItems::new(&data).load().unwrap(),
        vec![asked(&restore())]
    );
}

#[test]
fn an_archive_that_fails_keeps_its_item() {
    let (data, before) = seeded("archive-failed");
    let mut restore_run = Restore::answering(Ok(report()));
    restore_run.archived = Err(CoreError::GitHub("status 403".to_string()));
    let app = caller(Face::Gui, None, false, true);
    let got = restore_run.run(&data, &text_of(&asked(&delete())), &app);
    assert_eq!(got, Err(CoreError::GitHub("status 403".to_string())));
    assert_eq!(restore_run.archives.borrow().len(), 1);
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
        REPO,
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
        REPO,
        &unconfirmed,
    ));
    assert_eq!(why, "restore over owner/alpha: needs a confirm in the app");
    unchanged(&data, &before);
}
