//! Locked tests for #49 (WAIT-1: items from the snapshot). Written before the build (R28); the
//! builder never edits this file.

use switchboard_core::github::{Issue, PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::waiting::{from_snapshots, Item, Source, AT_FABLE_LABEL, NEEDS_ME_LABEL};

/// 2026-11-08T07:00:00Z
const READ_AT: i64 = 1_794_121_200;
const SB: &str = "iannitram/switchboard";

fn issue(number: u64, state: &str, labels: &[&str]) -> Issue {
    Issue {
        number,
        title: format!("Issue {number}"),
        state: state.to_string(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        ..Issue::default()
    }
}

fn verdict(v: &str, needs_me: bool) -> ReviewVerdict {
    ReviewVerdict {
        verdict: v.to_string(),
        sha: "abc123".to_string(),
        needs_me,
    }
}

fn pull(number: u64, state: &str, labels: &[&str], verdicts: Vec<ReviewVerdict>) -> PullRequest {
    PullRequest {
        number,
        title: format!("PR {number}"),
        state: state.to_string(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        verdicts,
        ..PullRequest::default()
    }
}

fn snap(repo: &str, issues: Vec<Issue>, pulls: Vec<PullRequest>) -> RepoSnapshot {
    RepoSnapshot {
        repo: repo.to_string(),
        read_at: READ_AT,
        issues,
        pulls,
        ..RepoSnapshot::default()
    }
}

fn sources(items: &[Item]) -> Vec<Source> {
    items.iter().map(|i| i.source.clone()).collect()
}

#[test]
fn the_label_names() {
    assert_eq!(NEEDS_ME_LABEL, "needs-ian");
    assert_eq!(AT_FABLE_LABEL, "review:fable");
}

#[test]
fn no_snapshots_give_no_items() {
    assert!(from_snapshots(&[]).is_empty());
    assert!(from_snapshots(&[snap(SB, vec![], vec![])]).is_empty());
}

#[test]
fn an_open_issue_labelled_needs_ian_is_an_item() {
    let mut i = issue(7, "open", &["size:easy", "needs-ian"]);
    i.title = "Pick the icon".to_string();
    let items = from_snapshots(&[snap(SB, vec![i], vec![])]);
    assert_eq!(
        items,
        vec![Item {
            what: "iannitram/switchboard#7: Pick the icon".to_string(),
            why: "Labelled needs-ian.".to_string(),
            source: Source::NeedsMe {
                repo: SB.to_string(),
                number: 7
            },
            link: "https://github.com/iannitram/switchboard/issues/7".to_string(),
            action: "Open issue".to_string(),
            since: READ_AT,
        }]
    );
}

#[test]
fn a_closed_issue_or_one_without_the_label_is_not_an_item() {
    let items = from_snapshots(&[snap(
        SB,
        vec![
            issue(1, "closed", &["needs-ian"]),
            issue(2, "open", &["size:easy"]),
            issue(3, "open", &["needs-ian-later"]),
        ],
        vec![],
    )]);
    assert!(items.is_empty(), "{items:?}");
}

#[test]
fn an_open_pr_labelled_needs_ian_is_an_item_with_its_pull_link() {
    let mut p = pull(12, "open", &["needs-ian"], vec![]);
    p.title = "Snapshot store".to_string();
    let items = from_snapshots(&[snap(SB, vec![], vec![p])]);
    assert_eq!(
        items,
        vec![Item {
            what: "iannitram/switchboard#12: Snapshot store".to_string(),
            why: "Labelled needs-ian.".to_string(),
            source: Source::NeedsMe {
                repo: SB.to_string(),
                number: 12
            },
            link: "https://github.com/iannitram/switchboard/pull/12".to_string(),
            action: "Open PR".to_string(),
            since: READ_AT,
        }]
    );
}

#[test]
fn a_pr_whose_newest_verdict_is_a_fail_without_a_fix_is_an_item() {
    let p = pull(
        20,
        "open",
        &[],
        vec![verdict("PASS", false), verdict("FAIL", true)],
    );
    let items = from_snapshots(&[snap(SB, vec![], vec![p])]);
    assert_eq!(
        items,
        vec![Item {
            what: "iannitram/switchboard#20: PR 20".to_string(),
            why: "The review posted a FAIL without a fix: a design question or a decision of mine."
                .to_string(),
            source: Source::ReviewFail {
                repo: SB.to_string(),
                pr: 20
            },
            link: "https://github.com/iannitram/switchboard/pull/20".to_string(),
            action: "Open PR".to_string(),
            since: READ_AT,
        }]
    );
}

#[test]
fn only_the_newest_verdict_counts_and_only_a_fail_that_needs_me() {
    let items = from_snapshots(&[snap(
        SB,
        vec![],
        vec![
            // An older FAIL that needed me, then a PASS: nothing waits.
            pull(
                21,
                "open",
                &[],
                vec![verdict("FAIL", true), verdict("PASS", false)],
            ),
            // A FAIL the reviewer fixed (needs_me false): not mine.
            pull(22, "open", &[], vec![verdict("FAIL", false)]),
            // No verdict at all.
            pull(23, "open", &[], vec![]),
        ],
    )]);
    assert!(items.is_empty(), "{items:?}");
}

#[test]
fn a_pr_labelled_review_fable_is_an_item() {
    let p = pull(30, "open", &["review:fable"], vec![]);
    let items = from_snapshots(&[snap(SB, vec![], vec![p])]);
    assert_eq!(
        items,
        vec![Item {
            what: "iannitram/switchboard#30: PR 30".to_string(),
            why: "The review ladder reached Fable.".to_string(),
            source: Source::LadderAtFable {
                repo: SB.to_string(),
                pr: 30
            },
            link: "https://github.com/iannitram/switchboard/pull/30".to_string(),
            action: "Open PR".to_string(),
            since: READ_AT,
        }]
    );
}

#[test]
fn a_merged_or_closed_pr_gives_nothing() {
    let all = ["needs-ian", "review:fable"];
    let items = from_snapshots(&[snap(
        SB,
        vec![],
        vec![
            pull(40, "merged", &all, vec![verdict("FAIL", true)]),
            pull(41, "closed", &all, vec![verdict("FAIL", true)]),
        ],
    )]);
    assert!(items.is_empty(), "{items:?}");
}

#[test]
fn one_pr_can_give_all_three_in_order_needs_me_fail_fable() {
    let p = pull(
        50,
        "open",
        &["review:fable", "needs-ian"],
        vec![verdict("FAIL", true)],
    );
    let items = from_snapshots(&[snap(SB, vec![], vec![p])]);
    assert_eq!(
        sources(&items),
        vec![
            Source::NeedsMe {
                repo: SB.to_string(),
                number: 50
            },
            Source::ReviewFail {
                repo: SB.to_string(),
                pr: 50
            },
            Source::LadderAtFable {
                repo: SB.to_string(),
                pr: 50
            },
        ]
    );
}

#[test]
fn order_is_snapshot_order_then_issues_then_pulls_each_as_given() {
    let a = snap(
        "iannitram/wavcraft",
        vec![
            issue(9, "open", &["needs-ian"]),
            issue(3, "open", &["needs-ian"]),
        ],
        vec![pull(5, "open", &["needs-ian"], vec![])],
    );
    let b = snap(
        "iannitram/aaa",
        vec![issue(1, "open", &["needs-ian"])],
        vec![],
    );
    let items = from_snapshots(&[a, b]);
    assert_eq!(
        sources(&items),
        vec![
            Source::NeedsMe {
                repo: "iannitram/wavcraft".to_string(),
                number: 9
            },
            Source::NeedsMe {
                repo: "iannitram/wavcraft".to_string(),
                number: 3
            },
            Source::NeedsMe {
                repo: "iannitram/wavcraft".to_string(),
                number: 5
            },
            Source::NeedsMe {
                repo: "iannitram/aaa".to_string(),
                number: 1
            },
        ]
    );
    assert_eq!(
        items[3].link,
        "https://github.com/iannitram/aaa/issues/1".to_string()
    );
}

#[test]
fn since_is_each_snapshot_read_time() {
    let mut a = snap(SB, vec![issue(1, "open", &["needs-ian"])], vec![]);
    a.read_at = 100;
    let mut b = snap(
        "iannitram/other",
        vec![issue(1, "open", &["needs-ian"])],
        vec![],
    );
    b.read_at = 200;
    let items = from_snapshots(&[a, b]);
    assert_eq!(items[0].since, 100);
    assert_eq!(items[1].since, 200);
}

#[test]
fn an_archived_repo_gives_nothing() {
    let mut s = snap(
        SB,
        vec![issue(1, "open", &["needs-ian"])],
        vec![pull(
            2,
            "open",
            &["review:fable"],
            vec![verdict("FAIL", true)],
        )],
    );
    s.archived = true;
    assert!(from_snapshots(&[s]).is_empty());
}

#[test]
fn the_snapshots_are_not_changed() {
    let snaps = vec![snap(
        SB,
        vec![issue(1, "open", &["needs-ian"])],
        vec![pull(
            2,
            "open",
            &["review:fable", "needs-ian"],
            vec![verdict("FAIL", true)],
        )],
    )];
    let before = snaps.clone();
    let _ = from_snapshots(&snaps);
    assert_eq!(snaps, before);
}

#[test]
fn the_code_map_marks_the_snapshot_items_present() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md");
    let map = std::fs::read_to_string(path).unwrap();
    assert!(map.contains("- WAIT-1 snapshot items (`from_snapshots`): present."));
}
