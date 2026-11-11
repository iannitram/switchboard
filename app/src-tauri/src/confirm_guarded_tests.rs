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
    assert!(!signature.contains("Option<"), "{signature}");
}

#[test]
fn the_window_registers_the_confirm_command() {
    let main = include_str!("main.rs");
    assert!(main.contains("commands::confirm_guarded,"));
}
