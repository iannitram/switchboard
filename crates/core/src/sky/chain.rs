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
/// the chain whose first issue has the lower number.
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

    let mut walk = Walk {
        arrows: &arrows,
        visited: BTreeSet::new(),
        stack: BTreeSet::new(),
        kept: BTreeMap::new(),
    };
    for &n in &open {
        if !walk.visited.contains(&n) {
            walk.visit(n);
        }
    }
    let kept = walk.kept;

    let mut memo = BTreeMap::new();
    let mut best: Vec<u64> = Vec::new();
    for &n in &open {
        let path = best_from(n, &kept, &mut memo);
        if better(&path, &best) {
            best = path;
        }
    }
    if best.len() < 2 {
        Vec::new()
    } else {
        best
    }
}

/// The depth-first walk that decides which arrows are kept.
struct Walk<'a> {
    arrows: &'a BTreeMap<u64, BTreeSet<u64>>,
    visited: BTreeSet<u64>,
    stack: BTreeSet<u64>,
    kept: BTreeMap<u64, Vec<u64>>,
}

impl Walk<'_> {
    fn visit(&mut self, node: u64) {
        self.visited.insert(node);
        self.stack.insert(node);
        let targets: Vec<u64> = self
            .arrows
            .get(&node)
            .map(|t| t.iter().copied().collect())
            .unwrap_or_default();
        for target in targets {
            if self.stack.contains(&target) {
                continue;
            }
            self.kept.entry(node).or_default().push(target);
            if !self.visited.contains(&target) {
                self.visit(target);
            }
        }
        self.stack.remove(&node);
    }
}

/// The best path starting at `node`: most issues, then the smaller as a list of numbers.
fn best_from(
    node: u64,
    kept: &BTreeMap<u64, Vec<u64>>,
    memo: &mut BTreeMap<u64, Vec<u64>>,
) -> Vec<u64> {
    if let Some(path) = memo.get(&node) {
        return path.clone();
    }
    let mut tail: Vec<u64> = Vec::new();
    if let Some(next_nodes) = kept.get(&node) {
        for &next in next_nodes {
            let candidate = best_from(next, kept, memo);
            if better(&candidate, &tail) {
                tail = candidate;
            }
        }
    }
    let mut path = vec![node];
    path.extend(tail);
    memo.insert(node, path.clone());
    path
}

/// Longer wins; on equal length the smaller list of numbers wins.
fn better(a: &[u64], b: &[u64]) -> bool {
    a.len() > b.len() || (a.len() == b.len() && a < b)
}
