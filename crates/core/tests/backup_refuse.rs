//! Locked tests for BAK-2: `refuse_reason`, the overwrite guard, over `Manifest` values.

use switchboard_core::backup::{refuse_reason, Manifest};

/// The last good backup: 60 issues (40 open, 4 closed by no merged PR), 10 PRs (3 open),
/// 5 labels and 30 label assignments.
fn old() -> Manifest {
    Manifest {
        repo: "switchboard".into(),
        taken_at: "2026-11-01T06:00:00Z".into(),
        refs: 12,
        issues: 60,
        open_issues: 40,
        pulls: 10,
        open_pulls: 3,
        comments: 200,
        labels: 5,
        label_assignments: 30,
        milestones: 2,
        releases: 4,
        closed_without_pr: 4,
        wiki: true,
    }
}

fn a_week_later(new: Manifest) -> Manifest {
    Manifest {
        taken_at: "2026-11-08T06:00:00Z".into(),
        ..new
    }
}

#[test]
fn the_same_backup_gives_none() {
    assert_eq!(refuse_reason(&old(), &a_week_later(old())), None);
}

#[test]
fn growth_in_every_count_gives_none() {
    let new = a_week_later(Manifest {
        issues: 80,
        open_issues: 60,
        pulls: 20,
        open_pulls: 8,
        labels: 9,
        label_assignments: 50,
        ..old()
    });
    assert_eq!(refuse_reason(&old(), &new), None);
}

#[test]
fn shrinking_totals_are_refused() {
    let new = a_week_later(Manifest {
        issues: 30,
        open_issues: 20,
        pulls: 5,
        open_pulls: 1,
        labels: 2,
        ..old()
    });
    assert_eq!(
        refuse_reason(&old(), &new).as_deref(),
        Some("the new backup has 37 issues, PRs and labels in all, the last had 75")
    );
}

#[test]
fn the_same_totals_with_8_issues_closed_by_no_pr_are_refused() {
    let new = a_week_later(Manifest {
        open_issues: 32,
        closed_without_pr: 12,
        ..old()
    });
    assert!(refuse_reason(&old(), &new).is_some());
}

#[test]
fn more_closed_by_no_pr_alone_is_refused() {
    // 8 already-closed issues relabelled as closed by no PR: open issues stay at 40.
    let new = a_week_later(Manifest {
        closed_without_pr: 12,
        ..old()
    });
    assert_eq!(
        refuse_reason(&old(), &new).as_deref(),
        Some("the new backup has 12 issues closed by no merged PR, the last had 4")
    );
}

#[test]
fn the_same_totals_with_8_label_assignments_removed_are_refused() {
    let new = a_week_later(Manifest {
        label_assignments: 22,
        ..old()
    });
    assert_eq!(
        refuse_reason(&old(), &new).as_deref(),
        Some("the new backup has 22 label assignments, the last had 30")
    );
}

#[test]
fn twenty_issues_closed_by_merged_prs_gives_none() {
    // 20 PRs opened and merged in the week, each closing an issue: open issues fall from
    // 40 to 20, closed_without_pr stays at 4.
    let new = a_week_later(Manifest {
        open_issues: 20,
        pulls: 30,
        ..old()
    });
    assert_eq!(refuse_reason(&old(), &new), None);
}

#[test]
fn open_issues_wiped_by_no_pr_name_the_counts() {
    let new = a_week_later(Manifest {
        open_issues: 12,
        closed_without_pr: 32,
        ..old()
    });
    assert_eq!(
        refuse_reason(&old(), &new).as_deref(),
        Some("the new backup has 12 open issues, the last had 40")
    );
}

/// 85 issues, 10 PRs and 5 labels: a count of 100 in all.
fn hundred() -> Manifest {
    Manifest {
        issues: 85,
        ..old()
    }
}

#[test]
fn a_drop_of_5_on_100_gives_none() {
    let new = a_week_later(Manifest {
        issues: 80,
        ..hundred()
    });
    assert_eq!(refuse_reason(&hundred(), &new), None);
}

#[test]
fn a_drop_of_6_on_100_is_refused() {
    let new = a_week_later(Manifest {
        issues: 79,
        ..hundred()
    });
    assert_eq!(
        refuse_reason(&hundred(), &new).as_deref(),
        Some("the new backup has 94 issues, PRs and labels in all, the last had 100")
    );
}

#[test]
fn a_drop_of_2_on_10_is_refused() {
    let ten = Manifest {
        open_pulls: 10,
        ..old()
    };
    let new = a_week_later(Manifest {
        open_pulls: 8,
        ..ten.clone()
    });
    assert_eq!(
        refuse_reason(&ten, &new).as_deref(),
        Some("the new backup has 8 open PRs, the last had 10")
    );
}

#[test]
fn a_drop_of_1_on_10_gives_none() {
    let ten = Manifest {
        open_pulls: 10,
        ..old()
    };
    let new = a_week_later(Manifest {
        open_pulls: 9,
        ..ten.clone()
    });
    assert_eq!(refuse_reason(&ten, &new), None);
}
