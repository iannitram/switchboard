//! #342 (SKY-8, GH-5, WAIT-1): a verdict on an older commit than the PR's head is not shown as
//! the PR's review, and a FAIL on an older commit is not a Waiting on you item.

use switchboard_core::github::merge::review_text;
use switchboard_core::github::{PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::waiting::{from_snapshots, Source};

fn pr(head: &str, verdicts: &[(&str, &str, bool)]) -> PullRequest {
    PullRequest {
        number: 10,
        title: "PR 10".to_string(),
        state: "open".to_string(),
        head_sha: head.to_string(),
        verdicts: verdicts
            .iter()
            .map(|(v, sha, needs_me)| ReviewVerdict {
                verdict: v.to_string(),
                sha: sha.to_string(),
                needs_me: *needs_me,
            })
            .collect(),
        ci_on_head: "success".to_string(),
        ..Default::default()
    }
}

#[test]
fn a_pass_on_the_head_is_pass() {
    assert_eq!(
        review_text(&pr("bbbb", &[("PASS", "bbbb", false)])).as_deref(),
        Some("PASS")
    );
}

#[test]
fn a_pass_on_an_older_commit_says_so() {
    assert_eq!(
        review_text(&pr("bbbb", &[("PASS", "aaaa", false)])).as_deref(),
        Some("PASS on an older commit")
    );
}

#[test]
fn a_fail_on_an_older_commit_says_so() {
    assert_eq!(
        review_text(&pr(
            "bbbb",
            &[("PASS", "bbbb", false), ("FAIL", "aaaa", true)]
        ))
        .as_deref(),
        Some("FAIL on an older commit")
    );
}

#[test]
fn only_the_latest_verdict_counts_and_no_verdict_is_none() {
    assert_eq!(
        review_text(&pr(
            "bbbb",
            &[("FAIL", "aaaa", false), ("PASS", "bbbb", false)]
        ))
        .as_deref(),
        Some("PASS")
    );
    assert_eq!(review_text(&pr("bbbb", &[])), None);
}

fn snap(pull: PullRequest) -> RepoSnapshot {
    RepoSnapshot {
        repo: "owner/alpha".to_string(),
        pulls: vec![pull],
        ..Default::default()
    }
}

#[test]
fn a_fail_without_a_fix_on_an_older_commit_is_not_waiting_on_me() {
    let items = from_snapshots(&[snap(pr("bbbb", &[("FAIL", "aaaa", true)]))]);
    assert!(items.is_empty(), "{items:?}");
}

#[test]
fn a_fail_without_a_fix_on_the_head_is_still_waiting_on_me() {
    let items = from_snapshots(&[snap(pr("bbbb", &[("FAIL", "bbbb", true)]))]);
    assert_eq!(
        items.iter().map(|i| i.source.clone()).collect::<Vec<_>>(),
        vec![Source::ReviewFail {
            repo: "owner/alpha".to_string(),
            pr: 10
        }]
    );
}
