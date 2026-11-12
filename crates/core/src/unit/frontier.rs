//! unit-frontier (#402): the frontier is the open, unclaimed units whose blockers are all closed
//! with a resolution record; `next` is the one to take, ordered by the open units it unblocks
//! (transitively), then smaller size, then earlier roadmap line. Pure: the caller reads the
//! issues, the blocked-by links, the resolution records, the claims and the roadmap graph.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::github::snapshot::Issue;
use crate::roadmap::graph::{Graph, Node, Size};

/// The exit code of `switchboard unit frontier` and `unit next` when the frontier is empty.
pub const EMPTY_EXIT: i32 = 75;

/// What the frontier reads. `links` are native blocked-by links as (blocked, blocker) issue
/// numbers; `resolved` are the issue numbers closed with a resolution record; `claimed` are the
/// issue numbers a worker holds.
pub struct Inputs<'a> {
    pub graph: &'a Graph,
    pub issues: &'a [Issue],
    pub links: &'a [(u64, u64)],
    pub resolved: &'a [u64],
    pub claimed: &'a [u64],
}

/// One unit in the frontier: its issue, the roadmap node it is made from, and how many open
/// units wait on it, directly or through other units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    pub number: u64,
    pub node_id: String,
    pub size: Size,
    pub line: usize,
    pub unblocks: usize,
}

/// The open, unclaimed units whose every blocker is closed with a resolution record, by issue
/// number. A blocker missing from `issues` is not closed, so it still blocks.
pub fn frontier(inputs: &Inputs) -> Vec<Ready> {
    let states: HashMap<u64, &str> = inputs
        .issues
        .iter()
        .map(|issue| (issue.number, issue.state.as_str()))
        .collect();
    let resolved: HashSet<u64> = inputs.resolved.iter().copied().collect();
    let claimed: HashSet<u64> = inputs.claimed.iter().copied().collect();
    let units: Vec<(u64, &Node)> = inputs
        .issues
        .iter()
        .filter(|issue| issue.state == "open")
        .filter_map(|issue| unit_node(inputs.graph, issue).map(|node| (issue.number, node)))
        .collect();
    let open: HashSet<u64> = units.iter().map(|(number, _)| *number).collect();

    let mut ready: Vec<Ready> = units
        .iter()
        .filter(|(number, _)| !claimed.contains(number))
        .filter(|(number, _)| {
            blockers_of(inputs.links, *number).all(|blocker| {
                states.get(&blocker) == Some(&"closed") && resolved.contains(&blocker)
            })
        })
        .map(|(number, node)| Ready {
            number: *number,
            node_id: node.id.clone(),
            size: node.size,
            line: node.line,
            unblocks: open_dependents(inputs.links, &open, *number),
        })
        .collect();
    ready.sort_by_key(|unit| unit.number);
    ready
}

/// The unit `frontier` gives first: most units unblocked, then smaller size, then earlier
/// roadmap line, then lower issue number. `None` when the frontier is empty (exit `EMPTY_EXIT`).
pub fn next(inputs: &Inputs) -> Option<Ready> {
    let mut ready = frontier(inputs);
    ready.sort_by_key(|unit| {
        (
            Reverse(unit.unblocks),
            size_rank(unit.size),
            unit.line,
            unit.number,
        )
    });
    ready.into_iter().next()
}

/// The graph node an issue is a unit of: the one named by its `node:<id>` label.
fn unit_node<'a>(graph: &'a Graph, issue: &Issue) -> Option<&'a Node> {
    issue.labels.iter().find_map(|label| {
        let id = label.strip_prefix("node:")?;
        graph.nodes.iter().find(|node| node.id == id)
    })
}

fn blockers_of(links: &[(u64, u64)], blocked: u64) -> impl Iterator<Item = u64> + '_ {
    links
        .iter()
        .filter(move |(waiting, _)| *waiting == blocked)
        .map(|(_, blocker)| *blocker)
}

/// How many units in `open` wait on `start`, directly or through other units (a breadth-first
/// walk of the links from `start` to what is blocked by it).
fn open_dependents(links: &[(u64, u64)], open: &HashSet<u64>, start: u64) -> usize {
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(current) = queue.pop_front() {
        for (blocked, blocker) in links {
            if *blocker == current && seen.insert(*blocked) {
                queue.push_back(*blocked);
            }
        }
    }
    seen.iter()
        .filter(|number| **number != start && open.contains(number))
        .count()
}

fn size_rank(size: Size) -> u8 {
    match size {
        Size::Easy => 0,
        Size::Medium => 1,
        Size::Hard => 2,
    }
}
