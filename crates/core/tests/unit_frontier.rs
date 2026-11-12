//! unit-frontier (#402): `frontier` lists the open, unclaimed units whose blockers are all closed
//! with a resolution record; `next` orders that list by open transitive dependents, then smaller
//! size, then earlier roadmap line.

use switchboard_core::github::snapshot::Issue;
use switchboard_core::roadmap::graph::{self, Graph};
use switchboard_core::unit::frontier::{self, Inputs, EMPTY_EXIT};

const ROADMAP: &str = "Updated: 2026-11-12\n\
\n\
features:\n\
- runner: The runner (release: after-1.0)\n\
\n\
nodes:\n\
- a: First (feature: runner, type: build, size: hard, after: none)\n\
- b: Second (feature: runner, type: build, size: medium, after: none)\n\
- c: Third (feature: runner, type: build, size: easy, after: none)\n\
- d: Fourth (feature: runner, type: build, size: easy, after: none)\n\
- e: Fifth (feature: runner, type: build, size: easy, after: none)\n\
\n\
unspecified:\n";

fn graph() -> Graph {
    graph::parse(ROADMAP).expect("the fixture's nodes block parses")
}

/// A unit: the issue `number` made from node `node`, in `state`.
fn unit(number: u64, state: &str, node: &str) -> Issue {
    Issue {
        number,
        title: format!("unit {number}"),
        state: state.to_string(),
        labels: vec![format!("node:{node}")],
        ..Default::default()
    }
}

/// The frontier's issue numbers, sorted.
fn numbers(ready: &[frontier::Ready]) -> Vec<u64> {
    let mut found: Vec<u64> = ready.iter().map(|r| r.number).collect();
    found.sort_unstable();
    found
}

#[test]
fn a_unit_with_one_open_blocker_is_never_in_the_frontier() {
    let graph = graph();
    // 11 is blocked by 10 (closed with a resolution record) and by 12 (still open).
    let issues = [
        unit(10, "closed", "a"),
        unit(11, "open", "b"),
        unit(12, "open", "c"),
    ];
    let inputs = Inputs {
        graph: &graph,
        issues: &issues,
        links: &[(11, 10), (11, 12)],
        resolved: &[10],
        claimed: &[],
    };
    assert_eq!(numbers(&frontier::frontier(&inputs)), vec![12]);
}

#[test]
fn a_blocker_closed_without_a_resolution_record_still_blocks() {
    let graph = graph();
    let issues = [unit(10, "closed", "a"), unit(11, "open", "b")];
    let inputs = Inputs {
        graph: &graph,
        issues: &issues,
        links: &[(11, 10)],
        resolved: &[],
        claimed: &[],
    };
    assert!(frontier::frontier(&inputs).is_empty());
}

#[test]
fn a_blocker_missing_from_the_issues_still_blocks() {
    let graph = graph();
    let issues = [unit(11, "open", "b")];
    let inputs = Inputs {
        graph: &graph,
        issues: &issues,
        links: &[(11, 10)],
        resolved: &[10],
        claimed: &[],
    };
    assert!(frontier::frontier(&inputs).is_empty());
}

#[test]
fn a_claimed_unit_is_not_in_the_frontier() {
    let graph = graph();
    let issues = [unit(10, "open", "a")];
    let inputs = Inputs {
        graph: &graph,
        issues: &issues,
        links: &[],
        resolved: &[],
        claimed: &[10],
    };
    assert!(frontier::frontier(&inputs).is_empty());
}

#[test]
fn a_closed_issue_or_one_without_a_node_is_not_a_unit() {
    let graph = graph();
    let issues = [
        unit(10, "closed", "a"),
        Issue {
            number: 13,
            state: "open".to_string(),
            labels: vec!["bug".to_string()],
            ..Default::default()
        },
        unit(14, "open", "d"),
    ];
    let inputs = Inputs {
        graph: &graph,
        issues: &issues,
        links: &[],
        resolved: &[10],
        claimed: &[],
    };
    assert_eq!(numbers(&frontier::frontier(&inputs)), vec![14]);
}

#[test]
fn of_two_ready_units_the_one_that_unblocks_more_is_first() {
    let graph = graph();
    // 21 (medium, line 2) blocks 22 and 23; 20 (hard, line 1) blocks nothing.
    let issues = [
        unit(20, "open", "a"),
        unit(21, "open", "b"),
        unit(22, "open", "c"),
        unit(23, "open", "d"),
    ];
    let inputs = Inputs {
        graph: &graph,
        issues: &issues,
        links: &[(22, 21), (23, 21)],
        resolved: &[],
        claimed: &[],
    };
    assert_eq!(numbers(&frontier::frontier(&inputs)), vec![20, 21]);
    assert_eq!(frontier::next(&inputs).expect("a unit is ready").number, 21);
}

#[test]
fn unblocking_counts_open_units_transitively() {
    let graph = graph();
    // 30 (hard) blocks 31, which blocks 32: two units behind it. 33 (easy) blocks only 34.
    // Counted directly both have one dependent and the easy one would win.
    let issues = [
        unit(30, "open", "a"),
        unit(31, "open", "e"),
        unit(32, "open", "c"),
        unit(33, "open", "d"),
        unit(34, "open", "e"),
    ];
    let inputs = Inputs {
        graph: &graph,
        issues: &issues,
        links: &[(31, 30), (32, 31), (34, 33)],
        resolved: &[],
        claimed: &[],
    };
    assert_eq!(frontier::next(&inputs).expect("a unit is ready").number, 30);
}

#[test]
fn a_closed_dependent_does_not_count_as_unblocked() {
    let graph = graph();
    // 40 (hard) blocks only 41, which is closed; 42 (easy) blocks 43, which is open.
    let issues = [
        unit(40, "open", "a"),
        unit(41, "closed", "b"),
        unit(42, "open", "c"),
        unit(43, "open", "d"),
    ];
    let inputs = Inputs {
        graph: &graph,
        issues: &issues,
        links: &[(41, 40), (43, 42)],
        resolved: &[],
        claimed: &[],
    };
    assert_eq!(frontier::next(&inputs).expect("a unit is ready").number, 42);
}

#[test]
fn a_tie_goes_to_the_smaller_size_then_the_earlier_line() {
    let graph = graph();
    // Nothing unblocks anything. 51 (medium, line 2) loses to 52 (easy, line 3): size first.
    // 52 (easy, line 3) beats 53 (easy, line 4): the earlier line.
    let issues = [
        unit(51, "open", "b"),
        unit(52, "open", "c"),
        unit(53, "open", "d"),
    ];
    let inputs = Inputs {
        graph: &graph,
        issues: &issues,
        links: &[],
        resolved: &[],
        claimed: &[],
    };
    assert_eq!(frontier::next(&inputs).expect("a unit is ready").number, 52);
}

#[test]
fn an_empty_frontier_has_no_next_unit_and_exits_75() {
    let graph = graph();
    let issues = [unit(10, "closed", "a")];
    let inputs = Inputs {
        graph: &graph,
        issues: &issues,
        links: &[],
        resolved: &[],
        claimed: &[],
    };
    assert!(frontier::frontier(&inputs).is_empty());
    assert!(frontier::next(&inputs).is_none());
    assert_eq!(EMPTY_EXIT, 75);
}
