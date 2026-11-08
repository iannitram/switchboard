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
///
/// Open issues count only the ones lost to something other than a merged PR: issues newly
/// closed and not added to `closed_without_pr` were closed by merged PRs, which BAK-2 lets
/// through ("a week of merged PRs closing their issues can").
pub fn refuse_reason(old: &Manifest, new: &Manifest) -> Option<String> {
    let all = |m: &Manifest| i128::from(m.issues) + i128::from(m.pulls) + i128::from(m.labels);
    if far(all(old), all(old) - all(new)) {
        return Some(said("issues, PRs and labels in all", all(new), all(old)));
    }

    let open = |m: &Manifest| i128::from(m.open_issues);
    let closed = |m: &Manifest| i128::from(m.issues) - open(m);
    let without_pr = |m: &Manifest| i128::from(m.closed_without_pr);
    let closed_by_merged_pr =
        ((closed(new) - closed(old)) - (without_pr(new) - without_pr(old))).max(0);
    if far(open(old), open(old) - open(new) - closed_by_merged_pr) {
        return Some(said("open issues", open(new), open(old)));
    }

    let open_pulls = |m: &Manifest| i128::from(m.open_pulls);
    if far(open_pulls(old), open_pulls(old) - open_pulls(new)) {
        return Some(said("open PRs", open_pulls(new), open_pulls(old)));
    }

    if far(without_pr(old), without_pr(new) - without_pr(old)) {
        return Some(said(
            "issues closed by no merged PR",
            without_pr(new),
            without_pr(old),
        ));
    }

    let assignments = |m: &Manifest| i128::from(m.label_assignments);
    if far(assignments(old), assignments(old) - assignments(new)) {
        return Some(said(
            "label assignments",
            assignments(new),
            assignments(old),
        ));
    }

    None
}

/// A change of `by` (in the bad direction) on a count of `old` is far: more than 5, or more
/// than 10% of `old`.
fn far(old: i128, by: i128) -> bool {
    by > 5 || by * 10 > old
}

/// The refusal in my words: "the new backup has 12 open issues, the last had 40".
fn said(what: &str, new: i128, old: i128) -> String {
    format!("the new backup has {new} {what}, the last had {old}")
}
