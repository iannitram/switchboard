//! Locked tests for #32 (SKY-5: find drift). Written before the build (R28); the builder never
//! edits this file.

use switchboard_core::github::Issue;
use switchboard_core::sky::drift::{find, Drift, STALE_DAYS};
use switchboard_core::sky::features::{Feature, FeatureBlock, Release};

fn issue(number: u64, state: &str, labels: &[&str]) -> Issue {
    Issue {
        number,
        title: format!("issue {number}"),
        state: state.to_string(),
        labels: labels.iter().map(|s| s.to_string()).collect(),
        milestone: Some("1.0".to_string()),
        ..Default::default()
    }
}

fn block(updated: Option<&str>, features: &[(&str, Release)]) -> FeatureBlock {
    FeatureBlock {
        updated: updated.map(str::to_string),
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

fn orphan(number: u64, features: &[&str]) -> Drift {
    Drift::Orphan {
        number,
        features: features.iter().map(|s| s.to_string()).collect(),
    }
}

fn dark(feature: &str) -> Drift {
    Drift::Dark {
        feature: feature.to_string(),
    }
}

const TODAY: &str = "2026-11-08";

#[test]
fn stale_days_is_fourteen() {
    assert_eq!(STALE_DAYS, 14);
}

#[test]
fn a_tidy_project_has_no_drift() {
    let issues = vec![
        issue(1, "open", &["feature:core", "size:easy"]),
        issue(2, "closed", &["feature:sky"]),
    ];
    let b = block(
        Some("2026-11-01"),
        &[("core", Release::V1), ("sky", Release::V1)],
    );
    assert_eq!(find(&issues, &b, TODAY), vec![]);
}

#[test]
fn an_issue_on_no_feature_is_an_orphan() {
    let issues = vec![
        issue(7, "open", &["size:easy"]),
        issue(8, "open", &["feature:core"]),
    ];
    let b = block(Some(TODAY), &[("core", Release::V1)]);
    assert_eq!(find(&issues, &b, TODAY), vec![orphan(7, &[])]);
}

#[test]
fn an_issue_on_two_features_is_an_orphan_listing_both_in_label_order() {
    let issues = vec![issue(
        3,
        "open",
        &["feature:sky", "kind:ui", "feature:core"],
    )];
    let b = block(Some(TODAY), &[("core", Release::V1), ("sky", Release::V1)]);
    assert_eq!(find(&issues, &b, TODAY), vec![orphan(3, &["sky", "core"])]);
}

#[test]
fn an_issue_on_a_feature_not_in_the_block_is_an_orphan() {
    let issues = vec![
        issue(4, "open", &["feature:ghost"]),
        issue(5, "open", &["feature:core"]),
    ];
    let b = block(Some(TODAY), &[("core", Release::V1)]);
    assert_eq!(find(&issues, &b, TODAY), vec![orphan(4, &["ghost"])]);
}

#[test]
fn closed_issues_can_be_orphans_too() {
    let issues = vec![issue(6, "closed", &[]), issue(9, "open", &["feature:core"])];
    let b = block(Some(TODAY), &[("core", Release::V1)]);
    assert_eq!(find(&issues, &b, TODAY), vec![orphan(6, &[])]);
}

#[test]
fn orphans_come_in_issue_number_order() {
    let issues = vec![
        issue(12, "open", &[]),
        issue(2, "open", &[]),
        issue(5, "open", &["feature:core"]),
    ];
    let b = block(Some(TODAY), &[("core", Release::V1)]);
    assert_eq!(
        find(&issues, &b, TODAY),
        vec![orphan(2, &[]), orphan(12, &[])]
    );
}

#[test]
fn a_release_1_0_feature_with_no_issue_is_a_dark_star_in_block_order() {
    let issues = vec![issue(1, "open", &["feature:core"])];
    let b = block(
        Some(TODAY),
        &[
            ("sky", Release::V1),
            ("core", Release::V1),
            ("budget", Release::V1),
        ],
    );
    assert_eq!(find(&issues, &b, TODAY), vec![dark("sky"), dark("budget")]);
}

#[test]
fn a_project_whose_only_dark_stars_are_after_1_0_has_no_drift() {
    let issues = vec![issue(1, "open", &["feature:core"])];
    let b = block(
        Some(TODAY),
        &[("core", Release::V1), ("relay", Release::After1)],
    );
    assert_eq!(find(&issues, &b, TODAY), vec![]);
}

#[test]
fn an_after_2_0_dark_star_is_not_drift() {
    let issues = vec![issue(1, "open", &["feature:core"])];
    let b = block(
        Some(TODAY),
        &[("core", Release::V1), ("gamification", Release::After2)],
    );
    assert_eq!(find(&issues, &b, TODAY), vec![]);
}

#[test]
fn a_feature_label_on_an_orphan_still_keeps_its_feature_from_being_dark() {
    // #3 is on two features, so it is an orphan, but core and sky each have an issue.
    let issues = vec![issue(3, "open", &["feature:core", "feature:sky"])];
    let b = block(Some(TODAY), &[("core", Release::V1), ("sky", Release::V1)]);
    assert_eq!(find(&issues, &b, TODAY), vec![orphan(3, &["core", "sky"])]);
}

#[test]
fn fourteen_days_is_not_stale_and_fifteen_is() {
    let b14 = block(Some("2026-10-25"), &[]);
    assert_eq!(find(&[], &b14, TODAY), vec![]);
    let b15 = block(Some("2026-10-24"), &[]);
    assert_eq!(
        find(&[], &b15, TODAY),
        vec![Drift::Stale {
            updated: Some("2026-10-24".to_string()),
            age_days: Some(15),
        }]
    );
}

#[test]
fn the_age_counts_across_months_years_and_leap_days() {
    let b = block(Some("2024-02-28"), &[]);
    assert_eq!(
        find(&[], &b, "2024-03-31"),
        vec![Drift::Stale {
            updated: Some("2024-02-28".to_string()),
            age_days: Some(32),
        }]
    );
    let y = block(Some("2025-12-20"), &[]);
    assert_eq!(
        find(&[], &y, "2026-01-05"),
        vec![Drift::Stale {
            updated: Some("2025-12-20".to_string()),
            age_days: Some(16),
        }]
    );
}

#[test]
fn a_missing_updated_line_is_stale_with_no_age() {
    let b = block(None, &[]);
    assert_eq!(
        find(&[], &b, TODAY),
        vec![Drift::Stale {
            updated: None,
            age_days: None,
        }]
    );
}

#[test]
fn an_updated_date_that_does_not_parse_is_stale_with_no_age() {
    for bad in ["8 Nov 2026", "2026-11-8", "2026-13-01", "soon"] {
        let b = block(Some(bad), &[]);
        assert_eq!(
            find(&[], &b, TODAY),
            vec![Drift::Stale {
                updated: Some(bad.to_string()),
                age_days: None,
            }],
            "{bad}"
        );
    }
}

#[test]
fn orphans_then_dark_stars_then_the_stale_date() {
    let issues = vec![issue(9, "open", &[]), issue(4, "open", &["feature:core"])];
    let b = block(
        Some("2026-10-01"),
        &[("core", Release::V1), ("sky", Release::V1)],
    );
    assert_eq!(
        find(&issues, &b, TODAY),
        vec![
            orphan(9, &[]),
            dark("sky"),
            Drift::Stale {
                updated: Some("2026-10-01".to_string()),
                age_days: Some(38),
            },
        ]
    );
}
