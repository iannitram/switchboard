//! bug-sweep (#410): `sweep` searches each sister app for a bug class's search patterns and gives
//! one create-queue issue per sister app with a hit; a second sweep of the same class queues
//! nothing new; an empty pattern list is refused.

use std::collections::HashSet;

use switchboard_core::bug::sweep::{sweep, Search};
use switchboard_core::github::queue::Create;
use switchboard_core::github::snapshot::Issue;
use switchboard_core::CoreError;

/// Hits are `(repo, pattern)` pairs; every other search misses.
struct FakeSearch {
    hits: HashSet<(String, String)>,
}

impl FakeSearch {
    fn new(hits: &[(&str, &str)]) -> Self {
        FakeSearch {
            hits: hits
                .iter()
                .map(|(repo, pattern)| (repo.to_string(), pattern.to_string()))
                .collect(),
        }
    }
}

impl Search for FakeSearch {
    fn hits(&self, repo: &str, pattern: &str) -> Result<bool, CoreError> {
        Ok(self.hits.contains(&(repo.to_string(), pattern.to_string())))
    }
}

fn patterns() -> Vec<String> {
    vec!["index out of range".into(), "slice_at\\(".into()]
}

fn siblings() -> Vec<String> {
    vec!["o/app-a".into(), "o/app-b".into(), "o/app-c".into()]
}

/// The create-queue issues as the snapshot would hold them once made: numbered, open.
fn made(queue: &[Create]) -> Vec<(String, Issue)> {
    queue
        .iter()
        .enumerate()
        .map(|(i, create)| match create {
            Create::Issue {
                repo,
                title,
                body,
                labels,
                ..
            } => (
                repo.clone(),
                Issue {
                    number: 100 + i as u64,
                    title: title.clone(),
                    state: "open".into(),
                    labels: labels.clone(),
                    milestone: None,
                    body: body.clone(),
                    closed_by_pr: None,
                },
            ),
            other => panic!("expected an issue, got {other:?}"),
        })
        .collect()
}

fn issue_repo(create: &Create) -> &str {
    match create {
        Create::Issue { repo, .. } => repo,
        other => panic!("expected an issue, got {other:?}"),
    }
}

#[test]
fn a_pattern_that_matches_in_a_sister_app_queues_exactly_one_issue_there() {
    // app-a hits both patterns, app-c one, app-b none: two issues, one each in app-a and app-c.
    let search = FakeSearch::new(&[
        ("o/app-a", "index out of range"),
        ("o/app-a", "slice_at\\("),
        ("o/app-c", "slice_at\\("),
    ]);
    let result = sweep(&search, "slice-bounds", &patterns(), &siblings(), &[]).unwrap();

    let repos: Vec<&str> = result.queue.iter().map(issue_repo).collect();
    assert_eq!(repos, vec!["o/app-a", "o/app-c"]);
    assert_eq!(
        result.repos.iter().map(|r| r.hits).collect::<Vec<_>>(),
        vec![2, 0, 1]
    );
    assert_eq!(
        result.repos.iter().map(|r| r.queued).collect::<Vec<_>>(),
        vec![true, false, true]
    );
}

#[test]
fn the_queued_issue_carries_the_sweep_and_class_labels() {
    let search = FakeSearch::new(&[("o/app-a", "index out of range")]);
    let result = sweep(&search, "slice-bounds", &patterns(), &siblings(), &[]).unwrap();

    match &result.queue[..] {
        [Create::Issue { labels, body, .. }] => {
            assert!(labels.contains(&"sibling-sweep".to_string()));
            assert!(labels.contains(&"bug-class:slice-bounds".to_string()));
            assert!(body.contains("index out of range"));
            assert!(!body.contains("slice_at"));
        }
        other => panic!("expected one issue, got {other:?}"),
    }
}

#[test]
fn a_second_sweep_of_the_same_class_queues_nothing_new() {
    let search = FakeSearch::new(&[
        ("o/app-a", "index out of range"),
        ("o/app-c", "slice_at\\("),
    ]);
    let first = sweep(&search, "slice-bounds", &patterns(), &siblings(), &[]).unwrap();
    assert_eq!(first.queue.len(), 2);

    let issues = made(&first.queue);
    let second = sweep(&search, "slice-bounds", &patterns(), &siblings(), &issues).unwrap();
    assert!(second.queue.is_empty());
    assert!(second.repos.iter().all(|r| !r.queued));
    // The hits are still counted: the sweep reports what it found, it just queues nothing.
    assert_eq!(
        second.repos.iter().map(|r| r.hits).collect::<Vec<_>>(),
        vec![1, 0, 1]
    );
}

#[test]
fn a_closed_sweep_issue_also_counts_as_already_swept() {
    let search = FakeSearch::new(&[("o/app-a", "index out of range")]);
    let issues = vec![(
        "o/app-a".to_string(),
        Issue {
            number: 7,
            title: "old".into(),
            state: "closed".into(),
            labels: vec!["sibling-sweep".into(), "bug-class:slice-bounds".into()],
            milestone: None,
            body: String::new(),
            closed_by_pr: None,
        },
    )];
    let result = sweep(&search, "slice-bounds", &patterns(), &siblings(), &issues).unwrap();
    assert!(result.queue.is_empty());
}

#[test]
fn another_class_in_the_same_app_does_not_count_as_swept() {
    let search = FakeSearch::new(&[("o/app-a", "index out of range")]);
    let issues = vec![(
        "o/app-a".to_string(),
        Issue {
            number: 7,
            title: "other class".into(),
            state: "open".into(),
            labels: vec!["sibling-sweep".into(), "bug-class:other".into()],
            milestone: None,
            body: String::new(),
            closed_by_pr: None,
        },
    )];
    let result = sweep(&search, "slice-bounds", &patterns(), &siblings(), &issues).unwrap();
    assert_eq!(result.queue.len(), 1);
    assert_eq!(issue_repo(&result.queue[0]), "o/app-a");
}

#[test]
fn no_hit_queues_nothing() {
    let search = FakeSearch::new(&[]);
    let result = sweep(&search, "slice-bounds", &patterns(), &siblings(), &[]).unwrap();
    assert!(result.queue.is_empty());
    assert!(result.repos.iter().all(|r| r.hits == 0 && !r.queued));
}

#[test]
fn an_empty_pattern_list_is_refused() {
    let search = FakeSearch::new(&[("o/app-a", "index out of range")]);
    let err = sweep(&search, "slice-bounds", &[], &siblings(), &[]).unwrap_err();
    assert!(matches!(err, CoreError::Refused(_)), "got {err:?}");
}
