//! Locked tests for issue 6 (CORE-3: the rules guard for merge, repo delete and restore over a
//! live repo). Qwen never edits this file.

use std::path::{Path, PathBuf};
use switchboard_core::rules::{
    guard, guard_restore_new, is_job_worktree, Caller, Face, GuardedAction, Role, Verdict,
};

const FACES: [Face; 3] = [Face::Gui, Face::Cli, Face::Mcp];

fn caller(face: Face, role: Option<Role>, in_wt: bool, confirmed: bool) -> Caller {
    Caller {
        face,
        role,
        in_job_worktree: in_wt,
        confirmed_in_app: confirmed,
    }
}

fn merge() -> GuardedAction {
    GuardedAction::Merge {
        repo: "owner/app".into(),
        pr: 12,
    }
}

fn delete() -> GuardedAction {
    GuardedAction::DeleteRepo {
        repo: "owner/app".into(),
    }
}

fn restore() -> GuardedAction {
    GuardedAction::RestoreOverLive {
        repo: "owner/app".into(),
    }
}

fn refuse(t: &str) -> Verdict {
    Verdict::Refuse(t.into())
}

#[test]
fn nothing_is_allowed_inside_a_job_worktree_from_any_face() {
    for face in FACES {
        let c = caller(face, Some(Role::ProjectManager), true, true);
        assert_eq!(
            guard(&c, &merge()),
            refuse("merge owner/app#12: refused inside a job worktree")
        );
        assert_eq!(
            guard(&c, &delete()),
            refuse("archive owner/app: refused inside a job worktree")
        );
        assert_eq!(
            guard(&c, &restore()),
            refuse("restore over owner/app: refused inside a job worktree")
        );
    }
}

#[test]
fn a_merge_from_the_app_is_allowed_with_or_without_a_role() {
    assert_eq!(
        guard(&caller(Face::Gui, None, false, false), &merge()),
        Verdict::Allow
    );
    let sat = caller(Face::Gui, Some(Role::Satellite), false, false);
    assert_eq!(guard(&sat, &merge()), Verdict::Allow);
}

#[test]
fn a_merge_from_the_cli_or_mcp_is_allowed_for_the_project_manager_role() {
    for face in [Face::Cli, Face::Mcp] {
        let c = caller(face, Some(Role::ProjectManager), false, false);
        assert_eq!(guard(&c, &merge()), Verdict::Allow, "{face:?}");
    }
}

#[test]
fn a_merge_from_the_cli_or_mcp_without_the_pm_role_is_refused_with_one_text() {
    let want = refuse("merge owner/app#12: only the project-manager role may merge");
    for face in [Face::Cli, Face::Mcp] {
        for role in [None, Some(Role::BudgetManager), Some(Role::Satellite)] {
            let c = caller(face, role.clone(), false, true);
            assert_eq!(guard(&c, &merge()), want, "{face:?} {role:?}");
        }
    }
}

#[test]
fn delete_and_restore_from_the_app_need_a_confirm_in_the_app() {
    let no = caller(Face::Gui, None, false, false);
    assert_eq!(
        guard(&no, &delete()),
        refuse("archive owner/app: needs a confirm in the app")
    );
    assert_eq!(
        guard(&no, &restore()),
        refuse("restore over owner/app: needs a confirm in the app")
    );
    let yes = caller(Face::Gui, None, false, true);
    assert_eq!(guard(&yes, &delete()), Verdict::Allow);
    assert_eq!(guard(&yes, &restore()), Verdict::Allow);
}

#[test]
fn delete_and_restore_from_the_cli_or_mcp_go_to_waiting_on_you_whatever_the_role() {
    for face in [Face::Cli, Face::Mcp] {
        for role in [None, Some(Role::ProjectManager)] {
            // confirmed_in_app is the GUI's alone; it changes nothing here.
            for confirmed in [false, true] {
                let c = caller(face, role.clone(), false, confirmed);
                assert_eq!(
                    guard(&c, &delete()),
                    Verdict::ToWaiting("archive owner/app: needs a confirm in the app".into())
                );
                assert_eq!(
                    guard(&c, &restore()),
                    Verdict::ToWaiting("restore over owner/app: needs a confirm in the app".into())
                );
            }
        }
    }
}

#[test]
fn guard_does_not_change_its_inputs() {
    let c = caller(Face::Cli, Some(Role::ProjectManager), false, false);
    let (c0, a0) = (c.clone(), merge());
    let a = merge();
    let _ = guard(&c, &a);
    assert_eq!((c, a), (c0, a0));
}

#[test]
fn roles_parse_from_their_flags_and_back() {
    for (flag, role) in [
        ("project-manager", Role::ProjectManager),
        ("budget-manager", Role::BudgetManager),
        ("satellite", Role::Satellite),
    ] {
        assert_eq!(Role::parse(flag), Some(role.clone()));
        assert_eq!(role.flag(), flag);
    }
    assert_eq!(Role::parse("pm"), None);
    assert_eq!(Role::parse("Project-Manager"), None);
}

/// A fresh folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("switchboard-rules-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_fleet_dash_folder_under_a_dash_wt_folder_is_a_job_worktree() {
    let base = scratch("named");
    let wt = base.join("switchboard-wt").join("fleet-abc123");
    assert!(is_job_worktree(&wt));
    assert!(is_job_worktree(&wt.join("crates").join("core")));
}

#[test]
fn plain_clones_and_other_folders_are_not_job_worktrees() {
    let base = scratch("plain");
    let clone = base.join("switchboard");
    std::fs::create_dir_all(clone.join(".git")).unwrap();
    assert!(!is_job_worktree(&clone));
    assert!(!is_job_worktree(&base.join("switchboard-wt")));
    assert!(!is_job_worktree(&base.join("other").join("fleet-abc123")));
}

fn worktree_with_gitdir(name: &str, gitdir: &str) -> PathBuf {
    let wt = scratch(name).join("checkout");
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {gitdir}\n")).unwrap();
    wt
}

#[test]
fn a_git_worktree_whose_gitdir_names_a_fleet_job_is_a_job_worktree() {
    let wt = worktree_with_gitdir(
        "gitdir-fleet",
        "/srv/switchboard/.git/worktrees/fleet-abc123",
    );
    assert!(is_job_worktree(&wt));
    assert!(is_job_worktree(&wt.join("src")));
    let win = worktree_with_gitdir(
        "gitdir-win",
        "C:\\Users\\someone\\agy-jobs\\x\\.git\\worktrees\\switchboard-fleet-abc123",
    );
    assert!(is_job_worktree(&win));
}

#[test]
fn a_git_worktree_for_anything_else_is_not_a_job_worktree() {
    let wt = worktree_with_gitdir("gitdir-other", "/srv/switchboard/.git/worktrees/feature-x");
    assert!(!is_job_worktree(&wt));
    assert!(!is_job_worktree(Path::new("/srv/nowhere/at/all")));
}

#[test]
fn the_code_map_shows_core_rules_present() {
    let map = include_str!("../../../docs/code-map.md");
    let line = map
        .lines()
        .skip_while(|l| *l != "### core-rules")
        .nth(1)
        .unwrap();
    assert!(line.contains("· present."), "{line}");
}

/// #426: a restore into a new private repo is the app's after my confirm, or the CLI's after the
/// repo's name was typed and with no role; never MCP, never a role, never in a job worktree.
#[test]
fn a_restore_into_a_new_repo_is_the_apps_or_the_clis_typed_confirm_only() {
    let needs = "restore over owner/app: needs a confirm in the app";
    let worktree = "restore over owner/app: refused inside a job worktree";
    let roles = [
        None,
        Some(Role::ProjectManager),
        Some(Role::BudgetManager),
        Some(Role::Satellite),
    ];
    for role in roles {
        for confirmed in [false, true] {
            let ask = |face| {
                guard_restore_new(&caller(face, role.clone(), false, confirmed), "owner/app")
            };
            // The app: only my confirm counts, whatever the role flag says.
            assert_eq!(
                ask(Face::Gui),
                if confirmed {
                    Verdict::Allow
                } else {
                    refuse(needs)
                }
            );
            // The CLI: the typed name, and no role.
            let cli = if confirmed && role.is_none() {
                Verdict::Allow
            } else {
                refuse(needs)
            };
            assert_eq!(ask(Face::Cli), cli);
            assert_eq!(ask(Face::Mcp), refuse(needs));
            for face in FACES {
                let got =
                    guard_restore_new(&caller(face, role.clone(), true, confirmed), "owner/app");
                assert_eq!(got, refuse(worktree));
            }
        }
    }
}
