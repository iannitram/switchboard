//! Locked tests for #21 (GH-5: merge eligibility from the PASS on head and green CI).
//! Qwen never edits this file (R28).

use switchboard_core::github::merge::{eligibility, MergeEligibility};
use switchboard_core::github::{PullRequest, ReviewVerdict};

const OLD: &str = "1111111111111111111111111111111111111111";
const HEAD: &str = "2222222222222222222222222222222222222222";

fn verdict(v: &str, sha: &str) -> ReviewVerdict {
    ReviewVerdict {
        verdict: v.to_string(),
        sha: sha.to_string(),
        needs_me: false,
    }
}

fn pr(verdicts: Vec<ReviewVerdict>, ci: &str) -> PullRequest {
    PullRequest {
        number: 61,
        title: "Merge gate".to_string(),
        state: "open".to_string(),
        head_sha: HEAD.to_string(),
        head_ref: "qwen/21-merge-gate".to_string(),
        labels: vec![],
        closes: vec![21],
        verdicts,
        ci_on_head: ci.to_string(),
    }
}

#[test]
fn pass_on_head_and_green_ci_is_eligible() {
    let p = pr(vec![verdict("PASS", HEAD)], "success");
    assert_eq!(eligibility(&p, false), MergeEligibility::Eligible);
}

#[test]
fn no_verdict_at_all_is_no_pass() {
    let p = pr(vec![], "success");
    assert_eq!(eligibility(&p, false), MergeEligibility::NoPass);
}

#[test]
fn only_fails_is_no_pass() {
    let p = pr(vec![verdict("FAIL", HEAD)], "success");
    assert_eq!(eligibility(&p, false), MergeEligibility::NoPass);
}

#[test]
fn a_fail_after_the_pass_is_no_pass_even_on_head() {
    let p = pr(
        vec![verdict("PASS", HEAD), verdict("FAIL", HEAD)],
        "success",
    );
    assert_eq!(eligibility(&p, false), MergeEligibility::NoPass);
}

#[test]
fn a_fail_then_a_pass_on_head_is_eligible() {
    let p = pr(vec![verdict("FAIL", OLD), verdict("PASS", HEAD)], "success");
    assert_eq!(eligibility(&p, false), MergeEligibility::Eligible);
}

#[test]
fn pass_on_an_old_head_after_a_new_code_commit_is_pass_not_on_head() {
    let p = pr(vec![verdict("PASS", OLD)], "success");
    assert_eq!(eligibility(&p, false), MergeEligibility::PassNotOnHead);
}

#[test]
fn pass_on_an_old_head_carried_over_by_an_update_from_main_is_eligible() {
    let p = pr(vec![verdict("PASS", OLD)], "success");
    assert_eq!(eligibility(&p, true), MergeEligibility::Eligible);
}

#[test]
fn pass_on_head_with_ci_pending_is_ci_not_green_pending() {
    let p = pr(vec![verdict("PASS", HEAD)], "pending");
    assert_eq!(
        eligibility(&p, false),
        MergeEligibility::CiNotGreen("pending".to_string())
    );
}

#[test]
fn pass_on_head_with_ci_red_is_ci_not_green_failure() {
    let p = pr(vec![verdict("PASS", HEAD)], "failure");
    assert_eq!(
        eligibility(&p, false),
        MergeEligibility::CiNotGreen("failure".to_string())
    );
}

#[test]
fn pass_on_head_with_no_checks_is_ci_not_green_none() {
    let p = pr(vec![verdict("PASS", HEAD)], "none");
    assert_eq!(
        eligibility(&p, false),
        MergeEligibility::CiNotGreen("none".to_string())
    );
}

#[test]
fn the_review_is_checked_before_ci() {
    let p = pr(vec![verdict("PASS", OLD)], "failure");
    assert_eq!(eligibility(&p, false), MergeEligibility::PassNotOnHead);
    let q = pr(vec![], "pending");
    assert_eq!(eligibility(&q, true), MergeEligibility::NoPass);
}

#[test]
fn carried_over_does_not_rescue_ci_that_is_not_green() {
    let p = pr(vec![verdict("PASS", OLD)], "pending");
    assert_eq!(
        eligibility(&p, true),
        MergeEligibility::CiNotGreen("pending".to_string())
    );
}
