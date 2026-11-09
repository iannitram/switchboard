//! GH-5: the merge path. Only `merge_pr.py` (R32), only when the PR's latest `VERDICT: PASS`
//! is on its head or carries over an update from main by patch-id (R25), and only once CI is
//! green on that head (R14). Never `gh pr merge` or the merge API.

use super::poll::Poller;
use super::snapshot::PullRequest;
use crate::platform::{current_os, python_launcher};
use crate::rules::{guard, Caller, GuardedAction};
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
    // The list is oldest first, so the last verdict is the latest one. Only a PASS counts.
    let latest = match pr.verdicts.last() {
        Some(v) if v.verdict == "PASS" => v,
        _ => return MergeEligibility::NoPass,
    };
    // An older PASS is good only when every later commit is a carried-over update from main.
    if latest.sha != pr.head_sha && !carried_over {
        return MergeEligibility::PassNotOnHead;
    }
    // CI is checked last, and `carried_over` never rescues it.
    if pr.ci_on_head == "success" {
        MergeEligibility::Eligible
    } else {
        MergeEligibility::CiNotGreen(pr.ci_on_head.clone())
    }
}

/// One merge: the pipeline and the clone it merges in, the PR, and whether a PASS on an older
/// head was carried over by patch-id (see `eligibility`).
pub struct MergeJob<'a> {
    /// `merge_pr.py` is `<pipeline_root>/tools/merge-pr/merge_pr.py`.
    pub pipeline_root: &'a std::path::Path,
    /// The local clone `merge_pr.py` merges in.
    pub clone: &'a std::path::Path,
    /// `owner/name`.
    pub repo: &'a str,
    pub pr: &'a PullRequest,
    pub carried_over: bool,
}

/// GH-5, AUTO-4: merge through `merge_pr.py merge <clone> main origin/<branch> --pr N --repo R`,
/// a `--dry-run` first and then `--push`, after CORE-3's guard allowed the caller. The repo is
/// re-read after the push (GH-3). Never `gh pr merge` or the merge API.
pub fn merge(caller: &Caller, job: &MergeJob<'_>, poller: &Poller<'_>) -> Result<(), CoreError> {
    guard(
        caller,
        &GuardedAction::Merge {
            repo: job.repo.to_string(),
            pr: job.pr.number,
        },
    )
    .into_result()?;
    match eligibility(job.pr, job.carried_over) {
        MergeEligibility::Eligible => {}
        MergeEligibility::NoPass => {
            return Err(CoreError::Refused(
                "no VERDICT: PASS on this PR".to_string(),
            ))
        }
        MergeEligibility::PassNotOnHead => {
            return Err(CoreError::Refused(
                "the latest VERDICT: PASS is not on the head".to_string(),
            ))
        }
        MergeEligibility::CiNotGreen(s) => {
            return Err(CoreError::Refused(format!("CI on the head is {s}")))
        }
    }
    run_merge_pr(job, "--dry-run")?;
    run_merge_pr(job, "--push")?;
    let _ = poller.after_write(job.repo);
    Ok(())
}

/// AUTO-4: the open PRs `eligibility` calls Eligible, smallest number first. `carried_over` is the caller's patch-id answer per PR.
pub fn ready_to_merge(
    prs: &[PullRequest],
    carried_over: &dyn Fn(&PullRequest) -> bool,
) -> Vec<u64> {
    ready_prs(prs, carried_over)
        .iter()
        .map(|(pr, _)| pr.number)
        .collect()
}

/// The open, Eligible PRs with their patch-id answer, smallest number first. The answer is
/// asked once per open PR.
fn ready_prs<'a>(
    prs: &'a [PullRequest],
    carried_over: &dyn Fn(&PullRequest) -> bool,
) -> Vec<(&'a PullRequest, bool)> {
    let mut ready = Vec::new();
    for pr in prs.iter().filter(|pr| pr.state == "open") {
        let carried = carried_over(pr);
        if eligibility(pr, carried) == MergeEligibility::Eligible {
            ready.push((pr, carried));
        }
    }
    ready.sort_by_key(|(pr, _)| pr.number);
    ready
}

/// AUTO-4: merge each ready PR one at a time through `merge`; stop at the first error. Returns
/// the PR numbers merged and the issue numbers their `closes` named (GitHub closes them on the
/// merge to main; the caller's re-read shows them closed).
pub fn merge_passed(
    caller: &Caller,
    clone: &std::path::Path,
    pipeline_root: &std::path::Path,
    repo: &str,
    prs: &[PullRequest],
    carried_over: &dyn Fn(&PullRequest) -> bool,
    poller: &Poller<'_>,
) -> Result<(Vec<u64>, Vec<u64>), CoreError> {
    let mut merged = Vec::new();
    let mut closed = Vec::new();
    for (pr, carried) in ready_prs(prs, carried_over) {
        let job = MergeJob {
            pipeline_root,
            clone,
            repo,
            pr,
            carried_over: carried,
        };
        merge(caller, &job, poller)?;
        merged.push(pr.number);
        closed.extend(pr.closes.iter().copied());
    }
    Ok((merged, closed))
}

/// Run `merge_pr.py merge ... <flag>` from the pipeline. A non-zero exit is a refusal.
fn run_merge_pr(job: &MergeJob<'_>, flag: &str) -> Result<(), CoreError> {
    let script = job
        .pipeline_root
        .join("tools")
        .join("merge-pr")
        .join("merge_pr.py");
    let (py, pre) = python_launcher(current_os());
    let out = std::process::Command::new(py)
        .args(pre)
        .arg(&script)
        .arg("merge")
        .arg(job.clone)
        .arg("main")
        .arg(format!("origin/{}", job.pr.head_ref))
        .arg("--pr")
        .arg(job.pr.number.to_string())
        .arg("--repo")
        .arg(job.repo)
        .arg(flag)
        .output()
        .map_err(|e| CoreError::Io(format!("{}: {e}", script.display())))?;
    if out.status.success() {
        return Ok(());
    }
    Err(CoreError::Refused(refusal(&out, flag)))
}

/// Why merge_pr.py refused: the text after the first `REFUSED:` line, else its last
/// non-empty line, else the exit code.
fn refusal(out: &std::process::Output, flag: &str) -> String {
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if let Some(why) = lines.iter().find_map(|l| l.strip_prefix("REFUSED:")) {
        return why.trim().to_string();
    }
    match lines.last() {
        Some(last) => last.to_string(),
        None => format!("merge_pr.py {flag} exited {:?}", out.status.code()),
    }
}
