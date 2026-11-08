//! `backup`: one latest backup of each repo in `Backups/github/<repo>/` in NextCloud (BAK-1),
//! guarded against a bad pass overwriting the last good copy (BAK-2), restorable to a new
//! private repo (BAK-3).

use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// The manifest BAK-1 writes beside the bundle and the JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub repo: String,
    /// RFC 3339.
    pub taken_at: String,
    pub refs: u64,
    pub issues: u64,
    pub open_issues: u64,
    pub pulls: u64,
    pub open_pulls: u64,
    pub comments: u64,
    pub labels: u64,
    /// Label assignments summed over every issue.
    pub label_assignments: u64,
    pub milestones: u64,
    pub releases: u64,
    /// Closed issues no merged PR closed.
    pub closed_without_pr: u64,
    pub wiki: bool,
}

/// BAK-1: `git bundle create <dir>/repo.bundle.new --all` from a mirror clone, then
/// `git bundle verify`; the verified file replaces `<dir>/repo.bundle`. Returns the ref count.
/// A failed step removes the new file and leaves any older bundle in place.
pub fn bundle(mirror: &Path, out_dir: &Path) -> Result<u64, CoreError> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| CoreError::Io(format!("{}: {e}", out_dir.display())))?;
    // `join` keeps an absolute `out_dir` as it is. Git runs inside the mirror, so the bundle
    // path must be absolute.
    let dir = std::env::current_dir()
        .map_err(|e| CoreError::Io(format!("current directory: {e}")))?
        .join(out_dir);
    let new = dir.join("repo.bundle.new");
    let old = dir.join("repo.bundle");
    let _ = std::fs::remove_file(&new);
    let refs = match create_and_verify(mirror, &new) {
        Ok(refs) => refs,
        Err(e) => {
            let _ = std::fs::remove_file(&new);
            return Err(e);
        }
    };
    if let Err(e) = std::fs::rename(&new, &old) {
        let _ = std::fs::remove_file(&new);
        return Err(CoreError::Io(format!("{}: {e}", old.display())));
    }
    Ok(refs)
}

/// Creates the bundle at `new` and verifies it. Returns the refs it holds.
fn create_and_verify(mirror: &Path, new: &Path) -> Result<u64, CoreError> {
    run(git_in(mirror).args(["bundle", "create"]).arg(new).arg("--all"), "bundle create")?;
    run(git_in(mirror).args(["bundle", "verify"]).arg(new), "bundle verify")?;
    let heads = run(
        git_in(mirror).args(["bundle", "list-heads"]).arg(new),
        "bundle list-heads",
    )?;
    let refs = count_refs(&heads.stdout);
    if refs == 0 {
        return Err(CoreError::Io(format!("{}: no refs to bundle", mirror.display())));
    }
    Ok(refs)
}

/// `git -C <mirror>`, to be given the rest of the arguments.
fn git_in(mirror: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(mirror);
    cmd
}

/// Runs a git command. A failure is `CoreError::Io` with the step's name and git's stderr.
fn run(cmd: &mut Command, step: &str) -> Result<std::process::Output, CoreError> {
    let out = cmd
        .output()
        .map_err(|e| CoreError::Io(format!("git {step}: {e}")))?;
    if out.status.success() {
        return Ok(out);
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(CoreError::Io(format!("git {step} failed: {}", err.trim())))
}

/// Counts the refs in `git bundle list-heads` output (`<oid> <refname>` per line). A `HEAD`
/// line is not a ref of its own, so it is not counted.
fn count_refs(heads: &[u8]) -> u64 {
    String::from_utf8_lossy(heads)
        .lines()
        .filter(|line| line.split_whitespace().nth(1).is_some_and(|name| name != "HEAD"))
        .count() as u64
}

/// BAK-2: why the new backup may not replace the old one, or `None` when it may. A change is
/// "far" when it is more than 10% of the old backup's count, or more than 5 (SPEC.md BAK-2).
/// Checked: fewer issues, PRs or labels in all; fewer open issues or open PRs; more closed
/// issues no merged PR closed; fewer label assignments.
pub fn refuse_reason(old: &Manifest, new: &Manifest) -> Option<String> {
    todo!("BAK-2")
}
