//! bug-sweep (#410): `bug sweep` searches each sister app for a bug class's search patterns (the
//! `bug_class` signatures) and gives one create-queue issue per sister app with a hit. A sister
//! app that already carries a `sibling-sweep` issue for the class, open or closed, gets none, so
//! a second sweep of the class queues nothing new. The search sits behind `Search`; the rest is
//! a plain function that returns the queue batch for the caller to send.

use crate::github::queue::Create;
use crate::github::snapshot::Issue;
use crate::CoreError;

/// The label every sweep issue carries.
pub const SWEEP_LABEL: &str = "sibling-sweep";

/// Searches one sister app's default branch for an extended regular expression.
pub trait Search {
    /// Whether `pattern` matches anywhere in `repo`.
    fn hits(&self, repo: &str, pattern: &str) -> Result<bool, CoreError>;
}

/// What the sweep found in one sister app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSweep {
    pub repo: String,
    /// How many of the class's patterns match in the repo.
    pub hits: usize,
    /// Whether the batch queues an issue for the repo.
    pub queued: bool,
}

/// The sweep's result: every sister app searched, and the create-queue batch for the hits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sweep {
    pub repos: Vec<RepoSweep>,
    pub queue: Vec<Create>,
}

/// `bug sweep <class>`: search each sister app in `siblings` for `signatures`. A repo with at
/// least one hit gets one issue, unless `issues` (the snapshot's issues, each with its repo)
/// already holds a `sibling-sweep` issue for `class_id` there. A repo named twice is searched
/// once. An empty pattern list is refused.
pub fn sweep(
    search: &dyn Search,
    class_id: &str,
    signatures: &[String],
    siblings: &[String],
    issues: &[(String, Issue)],
) -> Result<Sweep, CoreError> {
    if signatures.is_empty() {
        return Err(CoreError::Refused(format!(
            "sweep: bug class `{class_id}` has no search patterns"
        )));
    }
    let class_label = format!("bug-class:{class_id}");
    let swept = |repo: &str| {
        issues.iter().any(|(issue_repo, issue)| {
            issue_repo == repo
                && issue.labels.iter().any(|label| label == SWEEP_LABEL)
                && issue.labels.contains(&class_label)
        })
    };

    let mut repos: Vec<RepoSweep> = Vec::new();
    let mut queue: Vec<Create> = Vec::new();
    for repo in siblings {
        if repos.iter().any(|done| done.repo == *repo) {
            continue;
        }
        let mut matched: Vec<&String> = Vec::new();
        for pattern in signatures {
            if search.hits(repo, pattern)? {
                matched.push(pattern);
            }
        }
        let queued = !matched.is_empty() && !swept(repo);
        if queued {
            queue.push(sweep_issue(repo, class_id, &matched));
        }
        repos.push(RepoSweep {
            repo: repo.clone(),
            hits: matched.len(),
            queued,
        });
    }
    Ok(Sweep { repos, queue })
}

/// The issue for one sister app: the class, and the patterns that hit there.
fn sweep_issue(repo: &str, class_id: &str, matched: &[&String]) -> Create {
    let patterns: String = matched
        .iter()
        .map(|pattern| format!("- `{pattern}`\n"))
        .collect();
    Create::Issue {
        repo: repo.to_string(),
        title: format!("Check for the `{class_id}` bug class"),
        body: format!(
            "A bug fixed in another app belongs to the class `{class_id}`. These search \
             patterns of the class match in this repo:\n\n{patterns}\nCheck whether the same \
             bug is here.\n"
        ),
        labels: vec![SWEEP_LABEL.to_string(), format!("bug-class:{class_id}")],
        milestone: None,
    }
}
