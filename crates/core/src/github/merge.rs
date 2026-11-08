//! GH-5: the merge path. Only `merge_pr.py` (R32), only when the PR's latest `VERDICT: PASS`
//! is on its head or carries over an update from main by patch-id (R25), and only once CI is
//! green on that head (R14). Never `gh pr merge` or the merge API.

use super::snapshot::PullRequest;
use crate::rules::Caller;
use crate::CoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeEligibility {
    Eligible,
    /// No PASS at all.
    NoPass,
    /// The latest PASS is on an older head and a code commit came after it.
    PassNotOnHead,
    /// CI on the head is red, pending or missing. Holds its state.
    CiNotGreen(String),
}

/// GH-5: decide from the PR and, for a PASS on an older head, whether every commit after it
/// is an update from main with the same patch-id (`carried_over`, worked out by the caller
/// with `git patch-id`).
pub fn eligibility(pr: &PullRequest, carried_over: bool) -> MergeEligibility {
    todo!("GH-5")
}

/// GH-5, AUTO-4: merge through `merge_pr.py merge <clone> main origin/<branch> --pr N --repo R`,
/// a `--dry-run` first and then `--push`, after CORE-3's guard allowed the caller.
pub fn merge(
    caller: &Caller,
    clone: &std::path::Path,
    repo: &str,
    pr: &PullRequest,
) -> Result<(), CoreError> {
    todo!("GH-5")
}
