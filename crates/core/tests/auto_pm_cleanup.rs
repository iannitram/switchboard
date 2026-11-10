//! #298: a Project Manager job that ends `infra` with nothing pushed leaves no worktree or branch.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::autopilot::passes::{
    pm_cleanup_target, remove_pm_worktree, FleetStarter, JobStarter,
};
use switchboard_core::fleet::FleetBridge;

fn out(worktree: &str, branch: &str) -> Value {
    json!({"outcome": "infra", "pushed": false, "worktree": worktree, "branch": branch})
}

#[test]
fn infra_not_pushed_with_the_pm_names_is_cleaned() {
    let target = pm_cleanup_target(&out("/x/fleet-pm-drift-a", "fleet/pm-drift-a-1")).unwrap();
    assert_eq!(target.worktree, PathBuf::from("/x/fleet-pm-drift-a"));
    assert_eq!(target.branch, "fleet/pm-drift-a-1");
}

#[test]
fn every_other_case_is_left_alone() {
    let mut pushed = out("/x/fleet-pm-a", "fleet/pm-a");
    pushed["pushed"] = json!(true);
    assert_eq!(pm_cleanup_target(&pushed), None, "pushed");
    let mut done = out("/x/fleet-pm-a", "fleet/pm-a");
    done["outcome"] = json!("done");
    assert_eq!(pm_cleanup_target(&done), None, "outcome not infra");
    assert_eq!(
        pm_cleanup_target(&out("/x/other-a", "fleet/pm-a")),
        None,
        "folder name"
    );
    assert_eq!(
        pm_cleanup_target(&out("/x/fleet-pm-a", "fleet/other")),
        None,
        "branch name"
    );
    for field in ["outcome", "pushed", "worktree", "branch"] {
        let mut v = out("/x/fleet-pm-a", "fleet/pm-a");
        v.as_object_mut().unwrap().remove(field);
        assert_eq!(pm_cleanup_target(&v), None, "missing {field}");
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-pmclean-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    out
}

#[test]
fn removing_deletes_the_worktree_and_the_branch_and_a_missing_folder_is_skipped() {
    let root = temp_dir("remove");
    let clone = root.join("clone");
    std::fs::create_dir_all(&clone).unwrap();
    git(&clone, &["init", "-q"]);
    git(&clone, &["commit", "-q", "--allow-empty", "-m", "start"]);
    let worktree = root.join("fleet-pm-drift-a");
    let branch = "fleet/pm-drift-a-1";
    git(
        &clone,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            branch,
            &worktree.to_string_lossy(),
        ],
    );
    let target = pm_cleanup_target(&out(&worktree.to_string_lossy(), branch)).unwrap();
    remove_pm_worktree(&target);
    assert!(!worktree.exists());
    let branches = git(&clone, &["branch", "--list", branch]);
    assert!(branches.stdout.is_empty(), "branch is gone");
    remove_pm_worktree(&target); // already gone: no panic
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn finished_cleans_up_but_keeps_every_job_file_and_its_answer_is_unchanged() {
    let root = temp_dir("finished");
    let jobs = root.join("jobs");
    std::fs::create_dir_all(&jobs).unwrap();
    let clone = root.join("clone");
    std::fs::create_dir_all(&clone).unwrap();
    git(&clone, &["init", "-q"]);
    git(&clone, &["commit", "-q", "--allow-empty", "-m", "start"]);
    let worktree = root.join("fleet-pm-drift-b");
    git(
        &clone,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "fleet/pm-drift-b-1",
            &worktree.to_string_lossy(),
        ],
    );
    let mut starter = FleetStarter {
        fleet: FleetBridge::with_root(&jobs),
        jobs_dir: jobs.clone(),
        dev_root: jobs.clone(),
    };
    for ext in ["json", "err", "md"] {
        std::fs::write(jobs.join(format!("j.{ext}")), "x").unwrap();
    }
    let text = out(&worktree.to_string_lossy(), "fleet/pm-drift-b-1").to_string();
    std::fs::write(jobs.join("j.out"), &text).unwrap();
    assert_eq!(starter.finished("j"), Ok(true));
    assert!(!worktree.exists());
    for ext in ["json", "out", "err", "md"] {
        assert!(jobs.join(format!("j.{ext}")).exists(), "{ext} kept");
    }
    let _ = std::fs::remove_dir_all(&root);
}
