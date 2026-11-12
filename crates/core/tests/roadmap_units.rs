//! roadmap-units (#399): `plan` turns each node without an issue into one create-queue issue,
//! and `wire` turns each node's `after` list into native blocked-by links, once.

use std::collections::HashMap;

use switchboard_core::github::snapshot::Issue;
use switchboard_core::roadmap::graph::{self, Graph};
use switchboard_core::roadmap::units::{self, Link};

const ROADMAP: &str = "Updated: 2026-11-12\n\
\n\
features:\n\
- runner: The runner (release: after-1.0)\n\
\n\
nodes:\n\
- a: First (feature: runner, type: build, size: easy, after: none)\n\
- b: Second (feature: runner, type: research, size: easy, after: a)\n\
- c: Third (feature: runner, type: build, size: medium, after: a b)\n\
\n\
unspecified:\n";

fn graph() -> Graph {
    graph::parse(ROADMAP).expect("the fixture's nodes block parses")
}

/// An issue carrying the given labels (the node:<id> one, usually).
fn issue(number: u64, state: &str, labels: &[&str]) -> Issue {
    Issue {
        number,
        title: format!("issue {number}"),
        state: state.to_string(),
        labels: labels.iter().map(|label| label.to_string()).collect(),
        milestone: None,
        body: String::new(),
        closed_by_pr: None,
    }
}

fn labels_of(create: &switchboard_core::github::queue::Create) -> Vec<String> {
    match create {
        switchboard_core::github::queue::Create::Issue { labels, .. } => labels.clone(),
        other => panic!("expected an issue create, got {other:?}"),
    }
}

#[test]
fn plan_makes_one_issue_per_node_without_one() {
    let creates = units::plan(&graph(), &[], "owner/repo");
    assert_eq!(creates.len(), 3, "a, b and c have no issue yet");
}

#[test]
fn plan_skips_a_node_whose_label_an_open_or_closed_issue_carries() {
    let issues = [
        issue(10, "open", &["node:a", "type:build", "feature:runner"]),
        issue(11, "closed", &["node:b", "type:research", "feature:runner"]),
    ];
    let creates = units::plan(&graph(), &issues, "owner/repo");
    assert_eq!(creates.len(), 1, "only c still needs an issue");
    assert_eq!(
        labels_of(&creates[0])
            .iter()
            .filter(|l| l.starts_with("node:"))
            .count(),
        1
    );
    assert!(labels_of(&creates[0]).contains(&"node:c".to_string()));
}

#[test]
fn plan_labels_each_issue_with_one_node_one_type_and_its_feature() {
    let creates = units::plan(&graph(), &[], "owner/repo");
    let mut by_node = HashMap::new();
    for create in &creates {
        let labels = labels_of(create);
        let nodes: Vec<&String> = labels.iter().filter(|l| l.starts_with("node:")).collect();
        let types: Vec<&String> = labels.iter().filter(|l| l.starts_with("type:")).collect();
        let features: Vec<&String> = labels
            .iter()
            .filter(|l| l.starts_with("feature:"))
            .collect();
        assert_eq!(nodes.len(), 1, "exactly one node label in {labels:?}");
        assert_eq!(types.len(), 1, "exactly one type label in {labels:?}");
        assert_eq!(
            features,
            vec![&"feature:runner".to_string()],
            "its feature label in {labels:?}"
        );
        by_node.insert(nodes[0].clone(), types[0].clone());
    }
    assert_eq!(by_node["node:a"], "type:build");
    assert_eq!(by_node["node:b"], "type:research");
    assert_eq!(by_node["node:c"], "type:build");
}

#[test]
fn plan_is_a_queue_batch_of_issue_items() {
    let creates = units::plan(&graph(), &[], "owner/repo");
    let json = serde_json::to_value(&creates[0]).unwrap();
    assert_eq!(json["kind"], "issue");
    assert_eq!(json["repo"], "owner/repo");
}

fn numbers() -> HashMap<String, u64> {
    [("a", 20), ("b", 21), ("c", 22)]
        .into_iter()
        .map(|(id, number)| (id.to_string(), number))
        .collect()
}

#[test]
fn wire_links_every_after_id_as_a_blocker() {
    let links = units::wire(&graph(), &numbers(), &[]).expect("every node has an issue");
    assert_eq!(
        links,
        vec![
            Link {
                blocked: 21,
                blocker: 20
            },
            Link {
                blocked: 22,
                blocker: 20
            },
            Link {
                blocked: 22,
                blocker: 21
            },
        ]
    );
}

#[test]
fn wire_twice_changes_nothing() {
    let first = units::wire(&graph(), &numbers(), &[]).unwrap();
    let existing: Vec<(u64, u64)> = first.iter().map(|l| (l.blocked, l.blocker)).collect();
    let second = units::wire(&graph(), &numbers(), &existing).unwrap();
    assert!(second.is_empty(), "the links are there already: {second:?}");
}

#[test]
fn wire_refuses_an_after_id_with_no_issue() {
    let mut partial = numbers();
    partial.remove("a");
    let error = units::wire(&graph(), &partial, &[]).expect_err("a has no issue to block on");
    assert!(
        error.to_string().contains("`a`"),
        "names the missing node: {error}"
    );
}
