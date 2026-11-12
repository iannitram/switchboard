//! roadmap-drift (#400): the gaps between ROADMAP.md and the issues, each named by kind and id.
//!
//! - `node-without-unit`: a node with no issue. An issue belongs to a node through its
//!   `node:<id>` label; a node with no such issue is drift. `ref` is the node id.
//! - `unit-without-node`: an issue with no `node:<id>` label that names a node. `ref` is `#<n>`.
//! - `stale-date`: ROADMAP's `Updated:` date is missing or more than 14 days old (SKY-5's rule,
//!   read from `sky::drift`). `ref` is `ROADMAP.md`.
//! - `fog-ready`: an unspecified item whose `waits on` nodes are all done (a node is done when
//!   its issue is closed), so it can be specified now. `ref` is the item id.
//!
//! `find` is pure: it reads no file and runs no command. The caller passes the roadmap text,
//! the repo's issues and today's date (`YYYY-MM-DD`).

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::graph;
use crate::github::Issue;
use crate::sky::drift::{self as sky_drift, Drift as SkyDrift};
use crate::sky::features;
use crate::CoreError;

const NODE_PREFIX: &str = "node:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    NodeWithoutUnit,
    UnitWithoutNode,
    StaleDate,
    FogReady,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DriftItem {
    pub kind: Kind,
    /// The node id, the issue as `#<number>`, `ROADMAP.md`, or the unspecified item's id.
    #[serde(rename = "ref")]
    pub reference: String,
    pub message: String,
}

/// Every drift item for one project: nodes with no issue (in ROADMAP order), issues with no
/// node (by number), the stale date, then unspecified items ready to specify (in ROADMAP order).
/// Refused only when ROADMAP.md does not parse.
pub fn find(roadmap_md: &str, issues: &[Issue], today: &str) -> Result<Vec<DriftItem>, CoreError> {
    let graph = graph::parse(roadmap_md)?;
    let block = features::parse(roadmap_md)?;

    let node_ids: HashSet<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    let mut linked: HashMap<&str, Vec<&Issue>> = HashMap::new();
    for issue in issues {
        for id in issue
            .labels
            .iter()
            .filter_map(|label| label.strip_prefix(NODE_PREFIX))
        {
            linked.entry(id).or_default().push(issue);
        }
    }
    let done: HashSet<&str> = node_ids
        .iter()
        .copied()
        .filter(|id| {
            linked
                .get(id)
                .is_some_and(|found| found.iter().any(|issue| issue.state == "closed"))
        })
        .collect();

    let mut out = Vec::new();

    for node in &graph.nodes {
        if !linked.contains_key(node.id.as_str()) {
            out.push(drift_item(
                Kind::NodeWithoutUnit,
                &node.id,
                format!(
                    "node `{}` has no issue labelled `{NODE_PREFIX}{}`",
                    node.id, node.id
                ),
            ));
        }
    }

    let mut by_number: Vec<&Issue> = issues.iter().collect();
    by_number.sort_by_key(|issue| issue.number);
    for issue in by_number {
        let names_node = issue
            .labels
            .iter()
            .filter_map(|label| label.strip_prefix(NODE_PREFIX))
            .any(|id| node_ids.contains(id));
        if !names_node {
            out.push(drift_item(
                Kind::UnitWithoutNode,
                &format!("#{}", issue.number),
                format!(
                    "issue #{} has no `{NODE_PREFIX}<id>` label naming a node in ROADMAP.md",
                    issue.number
                ),
            ));
        }
    }

    let stale = sky_drift::find(&[], &block, today)
        .into_iter()
        .find_map(|drift| match drift {
            SkyDrift::Stale { updated, age_days } => Some((updated, age_days)),
            _ => None,
        });
    if let Some((updated, age_days)) = stale {
        let message = match (updated, age_days) {
            (None, _) => "ROADMAP.md has no `Updated:` date".to_string(),
            (Some(date), None) => {
                format!("ROADMAP.md's `Updated:` date `{date}` is not YYYY-MM-DD")
            }
            (Some(date), Some(days)) => {
                format!("ROADMAP.md was last updated {date}, {days} days ago")
            }
        };
        out.push(drift_item(Kind::StaleDate, "ROADMAP.md", message));
    }

    for item in &graph.unspecified {
        if item.waits_on.iter().all(|id| done.contains(id.as_str())) {
            out.push(drift_item(
                Kind::FogReady,
                &item.id,
                format!("unspecified `{}` waits only on done nodes", item.id),
            ));
        }
    }

    Ok(out)
}

fn drift_item(kind: Kind, reference: &str, message: String) -> DriftItem {
    DriftItem {
        kind,
        reference: reference.to_string(),
        message,
    }
}
