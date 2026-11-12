//! roadmap-units (#399): a node is made into an issue, and its `after` list into native
//! blocked-by links. `plan` gives the create-queue batch for the nodes that have no issue yet;
//! `wire` gives the blocked-by links still missing once every node has its issue number. Both
//! are pure: the caller sends the creates through the queue and the links to GitHub.

use std::collections::HashMap;

use crate::github::queue::Create;
use crate::github::snapshot::Issue;
use crate::roadmap::graph::{Graph, NodeType};
use crate::CoreError;

/// One native blocked-by link: issue `blocked` waits on issue `blocker`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub blocked: u64,
    pub blocker: u64,
}

fn node_label(id: &str) -> String {
    format!("node:{id}")
}

fn type_label(node_type: NodeType) -> &'static str {
    match node_type {
        NodeType::Build => "type:build",
        NodeType::Research => "type:research",
        NodeType::Prototype => "type:prototype",
        NodeType::Grilling => "type:grilling",
        NodeType::Task => "type:task",
    }
}

/// The create-queue issues for `graph`'s nodes that have no issue yet: a node is skipped when
/// an open or closed issue in `issues` carries its `node:<id>` label. Each new issue carries
/// that label, its type label and its `feature:` label.
pub fn plan(graph: &Graph, issues: &[Issue], repo: &str) -> Vec<Create> {
    let has_issue = |id: &str| {
        issues
            .iter()
            .any(|issue| issue.labels.iter().any(|label| *label == node_label(id)))
    };
    graph
        .nodes
        .iter()
        .filter(|node| !has_issue(&node.id))
        .map(|node| Create::Issue {
            repo: repo.to_string(),
            title: node.title.clone(),
            body: format!(
                "Node `{}` of ROADMAP.md (feature: `{}`). Its spec is `roadmap/nodes/{}.json`.",
                node.id, node.feature, node.id
            ),
            labels: vec![
                node_label(&node.id),
                type_label(node.node_type).to_string(),
                format!("feature:{}", node.feature),
            ],
            milestone: None,
        })
        .collect()
}

/// The blocked-by links each node's `after` list asks for, in graph order, that are not in
/// `linked` (the (blocked, blocker) pairs GitHub already has). `numbers` maps each node id to
/// its issue number. Refused when a node or one of its `after` ids has no issue number yet.
pub fn wire(
    graph: &Graph,
    numbers: &HashMap<String, u64>,
    linked: &[(u64, u64)],
) -> Result<Vec<Link>, CoreError> {
    let number_of = |id: &str| {
        numbers
            .get(id)
            .copied()
            .ok_or_else(|| CoreError::Refused(format!("wire: node `{id}` has no issue number yet")))
    };
    let mut links: Vec<Link> = Vec::new();
    for node in &graph.nodes {
        for after in &node.after {
            let link = Link {
                blocked: number_of(&node.id)?,
                blocker: number_of(after)?,
            };
            let present = linked.contains(&(link.blocked, link.blocker));
            if !present && !links.contains(&link) {
                links.push(link);
            }
        }
    }
    Ok(links)
}
