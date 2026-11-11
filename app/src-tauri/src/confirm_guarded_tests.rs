// Tests for the GUI face's confirm on a guarded request (CORE-3, BAK-3, #347), through
// `confirm_guarded_at`, the function the `confirm_guarded` command calls. `commands.rs` takes
// this file in with `#[cfg(test)] #[path = "confirm_guarded_tests.rs"] mod confirm_guarded_tests;`.
// No test here confirms a restore with a backups folder set, and none confirms a delete with
// the repo's name typed right (that would archive through the real transport), so none reaches
// GitHub. The archive itself is tested in the core against a fake (`confirm_guarded.rs`).
use super::*;
use switchboard_core::rules::GuardedAction;
use switchboard_core::waiting::guarded_item;

const NOW: UnixSeconds = 1_800_000_000;
const DELETE: &str = "I was asked from the CLI to archive owner/alpha";
const RESTORE: &str = "I was asked from the CLI to restore over owner/alpha";
const REPO: &str = "owner/alpha";

/// A fresh folder for one test, under the system temp folder.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "switchboard-app-confirm-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The folders of one test: a settings file that is not there (no path set), a data folder
/// with the two requests waiting, and the folder the app runs in.
struct Setup {
    settings: PathBuf,
    data: PathBuf,
    cwd: PathBuf,
    before: String,
}

fn setup(name: &str) -> Setup {
    let dir = scratch(name);
    let data = dir.join("data");
    let cwd = dir.join("plain");
    std::fs::create_dir_all(&cwd).unwrap();
    let local = LocalItems::new(&data);
    let repo = "owner/alpha".to_string();
    let delete = GuardedAction::DeleteRepo { repo: repo.clone() };
    let restore = GuardedAction::RestoreOverLive { repo };
    local.add(guarded_item(&delete, DELETE, NOW)).unwrap();
    local.add(guarded_item(&restore, RESTORE, NOW)).unwrap();
    let before = std::fs::read_to_string(local.path()).unwrap();
    Setup {
        settings: dir.join("settings.json"),
        data,
        cwd,
        before,
    }
}

fn unchanged(s: &Setup) {
    let now = std::fs::read_to_string(LocalItems::new(&s.data).path()).unwrap();
    assert_eq!(now, s.before);
}

#[test]
fn the_app_without_the_confirm_gets_the_guards_refusal_and_changes_nothing() {
    let s = setup("unconfirmed");
    for (text, want) in [
        (DELETE, "archive owner/alpha: needs a confirm in the app"),
        (
            RESTORE,
            "restore over owner/alpha: needs a confirm in the app",
        ),
    ] {
        let got = confirm_guarded_at(&s.settings, &s.data, text, false, REPO, &s.cwd);
        assert_eq!(got, Err(CoreError::Refused(want.to_string())));
    }
    unchanged(&s);
}

#[test]
fn the_app_inside_a_job_worktree_is_refused_even_with_the_confirm() {
    let s = setup("worktree");
    let cwd = s.cwd.join("app-wt").join("fleet-7");
    std::fs::create_dir_all(&cwd).unwrap();
    let got = confirm_guarded_at(&s.settings, &s.data, RESTORE, true, REPO, &cwd);
    assert_eq!(
        got,
        Err(CoreError::Refused(
            "restore over owner/alpha: refused inside a job worktree".to_string()
        ))
    );
    unchanged(&s);
}

#[test]
fn a_confirmed_restore_with_no_backups_folder_set_stops_there() {
    let s = setup("no-backups");
    let got = confirm_guarded_at(&s.settings, &s.data, RESTORE, true, REPO, &s.cwd);
    assert_eq!(got, Err(CoreError::MissingPath("nextcloud_backups")));
    unchanged(&s);
}

/// #347: the repo's name must be typed, letter for letter, for the archive and for the restore.
#[test]
fn a_confirm_without_the_repos_name_typed_right_is_refused_and_changes_nothing() {
    let s = setup("typed");
    for typed in ["", "owner/alph", "Owner/Alpha", "owner/alpha ", "alpha"] {
        let got = confirm_guarded_at(&s.settings, &s.data, DELETE, true, typed, &s.cwd);
        assert_eq!(
            got,
            Err(CoreError::Refused(
                "archive owner/alpha: type owner/alpha to confirm; the name typed does not \
                 match, nothing was changed"
                    .to_string()
            )),
            "{typed:?}"
        );
        let got = confirm_guarded_at(&s.settings, &s.data, RESTORE, true, typed, &s.cwd);
        assert_eq!(
            got,
            Err(CoreError::Refused(
                "restore over owner/alpha: type owner/alpha to confirm; the name typed does \
                 not match, nothing was changed"
                    .to_string()
            )),
            "{typed:?}"
        );
    }
    unchanged(&s);
}

#[test]
fn a_request_not_waiting_is_refused() {
    let s = setup("not-waiting");
    let got = confirm_guarded_at(
        &s.settings,
        &s.data,
        "delete owner/beta",
        true,
        "owner/beta",
        &s.cwd,
    );
    assert_eq!(
        got,
        Err(CoreError::Refused(
            "delete owner/beta: no such request is waiting on you".to_string()
        ))
    );
    unchanged(&s);
}

/// The command takes the typed name as a plain argument with no default, beside `confirmed`.
#[test]
fn the_command_takes_the_typed_name_with_no_default() {
    let commands = include_str!("commands.rs");
    let at = commands.find("pub fn confirm_guarded(").unwrap();
    let signature = &commands[at..at + commands[at..].find(") ->").unwrap()];
    assert!(signature.contains("confirmed: bool,"), "{signature}");
    assert!(signature.contains("typed: String,"), "{signature}");
    // #347: the only optional argument is the new private repo of a restore.
    assert_eq!(signature.matches("Option<").count(), 1, "{signature}");
    assert!(signature.contains("into: Option<String>,"), "{signature}");
}

#[test]
fn the_window_registers_the_confirm_command() {
    let main = include_str!("main.rs");
    assert!(main.contains("commands::confirm_guarded,"));
}

/// #347, turn a request down: the request leaves Waiting on you and the other one stays. No
/// settings are read: a drop goes nowhere but the data folder.
#[test]
fn dropping_a_request_removes_it_and_leaves_the_other() {
    let s = setup("drop");
    let dropped = drop_guarded_at(&s.data, DELETE, &s.cwd).unwrap();
    assert_eq!(dropped.what, "archive owner/alpha");
    assert_eq!(
        dropped.lines,
        ["the request to archive owner/alpha was dropped; nothing was archived, pushed or queued"]
    );
    let left = LocalItems::new(&s.data).load().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].what, RESTORE);
}

#[test]
fn dropping_what_is_not_waiting_or_from_a_job_worktree_is_refused() {
    let s = setup("drop-refused");
    assert_eq!(
        drop_guarded_at(&s.data, "restore over owner/beta", &s.cwd),
        Err(CoreError::Refused(
            "restore over owner/beta: no such request is waiting on you; nothing was dropped"
                .to_string()
        ))
    );
    let job = s
        .cwd
        .parent()
        .unwrap()
        .join("switchboard-wt")
        .join("fleet-9");
    std::fs::create_dir_all(&job).unwrap();
    assert_eq!(
        drop_guarded_at(&s.data, RESTORE, &job),
        Err(CoreError::Refused(
            "drop the request to restore over owner/alpha: refused inside a job worktree"
                .to_string()
        ))
    );
    unchanged(&s);
}

/// #347: a restore into a new private repo needs the same confirm and the same typed name as a
/// restore over the live one, and a name that can be a new repo. With all of that and no
/// backups folder set it stops there: no test here reaches GitHub.
#[test]
fn a_restore_into_a_new_repo_needs_the_same_confirm_and_stops_without_a_backups_folder() {
    let s = setup("into");
    let new = Some("owner/alpha-restored");
    let run = |said: (bool, &str), into: Option<&str>| {
        confirm_guarded_into_at(&s.settings, &s.data, RESTORE, said, into, &s.cwd)
    };
    assert_eq!(
        run((false, REPO), new),
        Err(CoreError::Refused(
            "restore over owner/alpha: needs a confirm in the app".to_string()
        ))
    );
    for typed in ["", "owner/alpha-restored", "Owner/Alpha"] {
        match run((true, typed), new) {
            Err(CoreError::Refused(why)) => {
                assert!(why.contains("type owner/alpha to confirm"), "{why}")
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    for (bad, needle) in [
        ("alpha-restored", "is not an owner/name"),
        ("owner/alpha", "is the repo itself"),
    ] {
        match run((true, REPO), Some(bad)) {
            Err(CoreError::Refused(why)) => assert!(why.contains(needle), "{why}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert_eq!(
        run((true, REPO), new),
        Err(CoreError::MissingPath("nextcloud_backups"))
    );
    unchanged(&s);
}
