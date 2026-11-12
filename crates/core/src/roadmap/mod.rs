//! The roadmap graph: ROADMAP.md's `nodes:` and `unspecified:` blocks and each node's spec file
//! (`roadmap/nodes/<id>.json`), read into a graph and linted. `lint` reports every problem by id;
//! `drift` reports the gaps between the roadmap and the issues. Neither runs a command.
//! `switchboard roadmap lint <clone> --json` is the lint's face.

pub mod drift;
pub mod graph;

use std::collections::HashMap;
use std::path::Path;

use crate::CoreError;

/// One problem, anchored to the node or unspecified id it is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The check that failed: `invalid_id`, `duplicate_id`, `unknown_feature`, `unknown_after`,
    /// `cycle`, `spec_file`, `missing_field`, `unknown_wait` or `feature_without_node`.
    pub check: &'static str,
    pub id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintReport {
    /// Problems that fail the lint.
    pub errors: Vec<Finding>,
    /// Problems that are reported but do not fail the lint: a feature with no node yet.
    pub warnings: Vec<Finding>,
    pub nodes: usize,
    pub unspecified: usize,
}

impl LintReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Lint the clone at `clone`: its ROADMAP.md and `roadmap/nodes/`. Refused outright only when
/// ROADMAP.md cannot be read or a line of its blocks or features block does not parse; every
/// other problem is a finding.
pub fn lint(clone: &Path) -> Result<LintReport, CoreError> {
    let roadmap_path = clone.join("ROADMAP.md");
    let roadmap_md = std::fs::read_to_string(&roadmap_path)
        .map_err(|error| CoreError::Io(format!("{}: {error}", roadmap_path.display())))?;
    let graph = graph::parse(&roadmap_md)?;
    let features: Vec<String> = crate::sky::features::parse(&roadmap_md)?
        .features
        .into_iter()
        .map(|feature| feature.id)
        .collect();
    let mut specs = HashMap::new();
    for node in &graph.nodes {
        if !graph::valid_id(&node.id) || specs.contains_key(&node.id) {
            continue;
        }
        specs.insert(node.id.clone(), graph::read_spec(clone, &node.id));
    }
    Ok(graph::check(&graph, &features, &specs))
}
