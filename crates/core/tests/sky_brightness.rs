//! Locked tests for #31 (SKY-2: constellation brightness from progress to 1.0). Written before
//! the build (R28); the builder never edits this file.

use switchboard_core::github::Issue;
use switchboard_core::sky::features::{Feature, FeatureBlock, Release};
use switchboard_core::sky::layout::brightness;

const EPS: f64 = 1e-9;

fn issue(number: u64, state: &str, milestone: Option<&str>, labels: &[&str]) -> Issue {
    Issue {
        number,
        title: format!("issue {number}"),
        state: state.to_string(),
        labels: labels.iter().map(|s| s.to_string()).collect(),
        milestone: milestone.map(str::to_string),
        ..Default::default()
    }
}

fn block(features: &[(&str, Release)]) -> FeatureBlock {
    FeatureBlock {
        updated: Some("2026-11-08".to_string()),
        features: features
            .iter()
            .map(|(id, release)| Feature {
                id: id.to_string(),
                title: id.to_string(),
                release: *release,
            })
            .collect(),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

#[test]
fn closed_stars_in_the_1_0_milestone_over_all_stars_in_it() {
    let issues = vec![
        issue(1, "closed", Some("1.0"), &["feature:core"]),
        issue(2, "closed", Some("1.0"), &["feature:core"]),
        issue(3, "open", Some("1.0"), &["feature:core"]),
        issue(4, "open", Some("1.0"), &["feature:sky"]),
    ];
    let b = block(&[("core", Release::V1), ("sky", Release::V1)]);
    assert!(close(brightness(&issues, &b), 0.5));
}

#[test]
fn stars_outside_the_1_0_milestone_do_not_count() {
    let issues = vec![
        issue(1, "closed", Some("1.0"), &["feature:core"]),
        issue(2, "open", Some("After 1.0"), &["feature:core"]),
        issue(3, "open", None, &["feature:core"]),
        issue(4, "closed", None, &["feature:core"]),
    ];
    let b = block(&[("core", Release::V1)]);
    assert!(close(brightness(&issues, &b), 1.0));
}

#[test]
fn each_release_1_0_dark_star_counts_as_one_open_star() {
    let issues = vec![
        issue(1, "closed", Some("1.0"), &["feature:core"]),
        issue(2, "closed", Some("1.0"), &["feature:core"]),
    ];
    let b = block(&[
        ("core", Release::V1),
        ("sky", Release::V1),
        ("budget", Release::V1),
    ]);
    // 2 closed over 2 stars + 2 dark stars
    assert!(close(brightness(&issues, &b), 0.5));
}

#[test]
fn after_1_0_dark_stars_are_not_counted() {
    let issues = vec![issue(1, "closed", Some("1.0"), &["feature:core"])];
    let b = block(&[
        ("core", Release::V1),
        ("relay", Release::After1),
        ("platforms", Release::After1),
    ]);
    assert!(close(brightness(&issues, &b), 1.0));
}

#[test]
fn after_2_0_dark_stars_do_not_dim() {
    let issues = vec![issue(1, "closed", Some("1.0"), &["feature:core"])];
    let b = block(&[
        ("core", Release::V1),
        ("relay", Release::After1),
        ("gamification", Release::After2),
    ]);
    assert!(close(brightness(&issues, &b), 1.0));
}

#[test]
fn a_feature_whose_only_issues_are_outside_1_0_is_not_dark() {
    let issues = vec![
        issue(1, "closed", Some("1.0"), &["feature:core"]),
        issue(2, "open", None, &["feature:sky"]),
    ];
    let b = block(&[("core", Release::V1), ("sky", Release::V1)]);
    assert!(close(brightness(&issues, &b), 1.0));
}

#[test]
fn a_feature_whose_only_issue_is_closed_is_not_dark() {
    let issues = vec![
        issue(1, "open", Some("1.0"), &["feature:core"]),
        issue(2, "closed", Some("1.0"), &["feature:sky"]),
    ];
    let b = block(&[("core", Release::V1), ("sky", Release::V1)]);
    assert!(close(brightness(&issues, &b), 0.5));
}

#[test]
fn only_dark_stars_is_zero() {
    let b = block(&[("core", Release::V1)]);
    assert!(close(brightness(&[], &b), 0.0));
}

#[test]
fn no_stars_and_no_dark_stars_is_zero() {
    assert!(close(brightness(&[], &block(&[])), 0.0));
    let b = block(&[("relay", Release::After1)]);
    assert!(close(brightness(&[], &b), 0.0));
}
