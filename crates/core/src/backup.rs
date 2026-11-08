//! `backup`: one latest backup of each repo in `Backups/github/<repo>/` in NextCloud (BAK-1),
//! guarded against a bad pass overwriting the last good copy (BAK-2), restorable to a new
//! private repo (BAK-3).

use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::path::Path;

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

/// BAK-1: `git bundle create <dir>/repo.bundle --all` from a mirror clone, then
/// `git bundle verify`.
pub fn bundle(mirror: &Path, out_dir: &Path) -> Result<u64, CoreError> {
    todo!("BAK-1")
}

/// BAK-2: why the new backup may not replace the old one, or `None` when it may. A change is
/// "far" when it is more than 10% of the old backup's count, or more than 5 (SPEC.md BAK-2).
/// Checked: fewer issues, PRs or labels in all; fewer open issues or open PRs; more closed
/// issues no merged PR closed; fewer label assignments.
pub fn refuse_reason(old: &Manifest, new: &Manifest) -> Option<String> {
    let all = |m: &Manifest| m.issues + m.pulls + m.labels;
    if fell_far(all(old), all(new)) {
        return Some(said("issues, PRs and labels in all", all(new), all(old)));
    }
    if fell_far(old.open_issues, new.open_issues) {
        return Some(said("open issues", new.open_issues, old.open_issues));
    }
    if fell_far(old.open_pulls, new.open_pulls) {
        return Some(said("open PRs", new.open_pulls, old.open_pulls));
    }
    if rose_far(old.closed_without_pr, new.closed_without_pr) {
        return Some(said(
            "issues closed by no merged PR",
            new.closed_without_pr,
            old.closed_without_pr,
        ));
    }
    if fell_far(old.label_assignments, new.label_assignments) {
        return Some(said(
            "label assignments",
            new.label_assignments,
            old.label_assignments,
        ));
    }
    None
}

/// A count fell far: more than 10% of the old count, or more than 5.
fn fell_far(old: u64, new: u64) -> bool {
    let drop = old.saturating_sub(new);
    drop > 5 || drop.saturating_mul(10) > old
}

/// A count rose far: more than 10% of the old count, or more than 5.
fn rose_far(old: u64, new: u64) -> bool {
    let rise = new.saturating_sub(old);
    rise > 5 || rise.saturating_mul(10) > old
}

fn said(what: &str, new: u64, old: u64) -> String {
    format!("the new backup has {new} {what}, the last had {old}")
}
