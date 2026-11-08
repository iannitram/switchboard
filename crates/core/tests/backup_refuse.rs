//! Locked tests for BAK-2 (`refuse_reason`: the overwrite guard over `Manifest` values).

use switchboard_core::backup::{refuse_reason, Manifest};

/// A backup of a repo with 40 issues, 10 PRs, 5 labels and 30 label assignments.
fn old() -> Manifest {
    Manifest {
        repo: "switchboard".into(),
        taken_at: "2026-11-01T06:00:00Z".into(),
        refs: 12,
        issues: 40,
        open_issues: 12,
        pulls: 10,
        open_pulls: 3,
        comments: 200,
        labels: 5,
        label_assignments: 30,
        milestones: 2,
        releases: 4,
        closed_without_pr: 0,
        wiki: true,
    }
}

#[test]
fn shrinking_totals_are_refused() {
    let new = Manifest {
        issues: 20,
        pulls: 5,
        labels: 2,
        ..old()
    };
    assert!(refuse_reason(&old(), &new).is_some());
}

#[test]
fn same_totals_with_8_issues_closed_by_no_pr_are_refused() {
    let new = Manifest {
        closed_without_pr: 8,
        ..old()
    };
    assert!(refuse_reason(&old(), &new).is_some());
}

#[test]
fn same_totals_with_8_label_assignments_removed_are_refused() {
    let new = Manifest {
        label_assignments: 22,
        ..old()
    };
    assert!(refuse_reason(&old(), &new).is_some());
}

#[test]
fn twenty_issues_closed_by_merged_prs_gives_none() {
    // 20 of 40 open issues closed by merged PRs: the totals stay, closed_without_pr stays.
    let old = Manifest {
        open_issues: 40,
        ..old()
    };
    let new = Manifest {
        open_issues: 20,
        ..old.clone()
    };
    assert_eq!(refuse_reason(&old, &new), None);
}

#[test]
fn drop_of_5_on_100_gives_none() {
    let old = Manifest {
        issues: 100,
        ..old()
    };
    let new = Manifest {
        issues: 95,
        ..old.clone()
    };
    assert_eq!(refuse_reason(&old, &new), None);
}

#[test]
fn drop_of_6_on_100_is_refused() {
    let old = Manifest {
        issues: 100,
        ..old()
    };
    let new = Manifest {
        issues: 94,
        ..old.clone()
    };
    assert!(refuse_reason(&old, &new).is_some());
}

#[test]
fn drop_of_2_on_10_is_refused() {
    let old = Manifest {
        open_issues: 10,
        ..old()
    };
    let new = Manifest {
        open_issues: 8,
        ..old.clone()
    };
    assert!(refuse_reason(&old, &new).is_some());
}
