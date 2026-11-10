//! SKY-3, SKY-7: the blocked-by lines and the longest chain of them.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::github::Issue;

const BLOCKED_BY: &str = "blocked by";

/// SKY-3: every `Blocked by #N` in an issue body, as (blocked, blocker) pairs, in issue order.
/// `#N` may repeat on one line (`Blocked by #3, #4`).
pub fn edges(issues: &[Issue]) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for issue in issues {
        for line in issue.body.lines() {
            let line = line.trim_start();
            let line = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .unwrap_or(line);
            if !line.to_ascii_lowercase().starts_with(BLOCKED_BY) {
                continue;
            }
            // The ten bytes of "blocked by" are ASCII, so byte 10 starts a character.
            for blocker in hash_numbers(&line[BLOCKED_BY.len()..]) {
                if blocker != issue.number && seen.insert((issue.number, blocker)) {
                    out.push((issue.number, blocker));
                }
            }
        }
    }
    out
}

/// The numbers written as `#` followed by digits, left to right.
fn hash_numbers(text: &str) -> Vec<u64> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start {
                if let Ok(n) = text[start..end].parse::<u64>() {
                    out.push(n);
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// SKY-7: the longest chain over the open issues, as issue numbers from the first blocker to
/// the last blocked. A cycle is broken where a depth-first walk from each open issue, in
/// ascending order, reaches an issue still on its own stack: that arrow is dropped. Ties go to
/// the chain whose first issue has the lower number, then the lower next issue.
///
/// Neither the walk nor the path search recurses, so a chain of any length fits on the stack
/// (#337).
pub fn longest(issues: &[Issue]) -> Vec<u64> {
    let open: BTreeSet<u64> = issues
        .iter()
        .filter(|i| i.state == "open")
        .map(|i| i.number)
        .collect();
    // blocker -> the open issues it blocks
    let mut arrows: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for (blocked, blocker) in edges(issues) {
        if open.contains(&blocked) && open.contains(&blocker) {
            arrows.entry(blocker).or_default().insert(blocked);
        }
    }

    let (kept, finished) = walk(&open, &arrows);

    // The kept arrows have no cycle, and the walk finishes an issue only after every issue a
    // kept arrow leads to, so one pass in finishing order sees each next issue settled. The
    // best path from an issue is the issue, then the best path of its next issue with the most
    // issues; on a tie the lower next issue (two paths from different issues differ in their
    // first number, so that is the smaller list).
    let mut length: BTreeMap<u64, usize> = BTreeMap::new();
    let mut next: BTreeMap<u64, u64> = BTreeMap::new();
    for &node in &finished {
        let mut best: Option<(usize, u64)> = None;
        for &target in kept.get(&node).map(Vec::as_slice).unwrap_or_default() {
            let len = length[&target];
            if best.map_or(true, |(l, t)| len > l || (len == l && target < t)) {
                best = Some((len, target));
            }
        }
        length.insert(node, 1 + best.map_or(0, |(l, _)| l));
        if let Some((_, target)) = best {
            next.insert(node, target);
        }
    }

    // The longest path over all open issues; ascending order keeps the lower first issue.
    let mut start: Option<(usize, u64)> = None;
    for &n in &open {
        let len = length[&n];
        if start.map_or(true, |(l, _)| len > l) {
            start = Some((len, n));
        }
    }
    let Some((len, first)) = start else {
        return Vec::new();
    };
    if len < 2 {
        return Vec::new();
    }
    let mut path = Vec::with_capacity(len);
    let mut node = Some(first);
    while let Some(n) = node {
        path.push(n);
        node = next.get(&n).copied();
    }
    path
}

/// The depth-first walk from each open issue in ascending order, on a stack of its own: the
/// arrows it keeps (every arrow except one back to an issue still on the walk's stack), each
/// issue's in the order it met them, and the issues in the order the walk finished them.
fn walk(
    open: &BTreeSet<u64>,
    arrows: &BTreeMap<u64, BTreeSet<u64>>,
) -> (BTreeMap<u64, Vec<u64>>, Vec<u64>) {
    let mut visited: HashSet<u64> = HashSet::new();
    let mut on_stack: HashSet<u64> = HashSet::new();
    let mut kept: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut finished: Vec<u64> = Vec::with_capacity(open.len());
    let targets_of = |node: u64| -> Vec<u64> {
        arrows
            .get(&node)
            .map(|t| t.iter().copied().collect())
            .unwrap_or_default()
    };

    for &root in open {
        if visited.contains(&root) {
            continue;
        }
        visited.insert(root);
        on_stack.insert(root);
        // Each frame: the issue, its targets, and how many of them were looked at.
        let mut frames: Vec<(u64, Vec<u64>, usize)> = vec![(root, targets_of(root), 0)];
        while let Some((node, targets, i)) = frames.last_mut() {
            let node = *node;
            let Some(&target) = targets.get(*i) else {
                on_stack.remove(&node);
                finished.push(node);
                frames.pop();
                continue;
            };
            *i += 1;
            if on_stack.contains(&target) {
                continue;
            }
            kept.entry(node).or_default().push(target);
            if visited.insert(target) {
                on_stack.insert(target);
                frames.push((target, targets_of(target), 0));
            }
        }
    }
    (kept, finished)
}
