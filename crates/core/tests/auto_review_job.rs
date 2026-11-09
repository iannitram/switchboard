//! AUTO-3: the review job the core launches for one try of R25's ladder. Pure: no I/O.

use serde_json::json;
use switchboard_core::autopilot::review::{review_job, rung_label, Rung};

#[test]
fn review_job_carries_the_brief_and_the_reviewer_is_not_the_builder() {
    let job = review_job("acme/app", 42, 7, Rung::Hard, 2, "builder-box");
    assert_eq!(
        job,
        json!({
            "kind": "review",
            "repo": "acme/app",
            "pr": 42,
            "issue": 7,
            "rung": "hard",
            "try": 2,
            "not_service": "builder-box",
            "brief": ["diff", "issue"],
        })
    );
}

#[test]
fn try_3_at_opus_builds_an_opus_job() {
    let job = review_job("acme/app", 1, 1, Rung::Opus, 3, "svc");
    assert_eq!(job["rung"], "opus");
    assert_eq!(job["try"], 3);
}

#[test]
fn fable_job_has_rung_fable() {
    let job = review_job("acme/app", 1, 1, Rung::Fable, 1, "svc");
    assert_eq!(job["rung"], "fable");
}

#[test]
fn rung_labels_cover_the_ladder() {
    assert_eq!(rung_label(Rung::Easy), "easy");
    assert_eq!(rung_label(Rung::Medium), "medium");
    assert_eq!(rung_label(Rung::Hard), "hard");
    assert_eq!(rung_label(Rung::Opus), "opus");
    assert_eq!(rung_label(Rung::Fable), "fable");
}
