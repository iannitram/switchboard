//! ROADMAP.md's `nodes:` and `unspecified:` blocks, each node's spec file, and the checks over
//! them. Reading never runs a command.
//!
//! The format (ROADMAP.md, "The roadmap graph"):
//!
//! ```text
//! nodes:
//! - <id>: <title> (feature: <id>, type: build|research|prototype|grilling|task, size: easy|medium|hard, kind: <k>, after: <ids or none>)
//!
//! unspecified:
//! - <id>: <gist> (waits on: <ids or none>)
//! ```
//!
//! `kind` is optional. A block ends at the first line that does not start with `- `.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use serde::Deserialize;

use super::{Finding, LintReport};
use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Build,
    Research,
    Prototype,
    Grilling,
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Easy,
    Medium,
    Hard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub title: String,
    pub feature: String,
    pub node_type: NodeType,
    pub size: Size,
    pub kind: Option<String>,
    pub after: Vec<String>,
    /// 1-based line in ROADMAP.md.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unspecified {
    pub id: String,
    pub gist: String,
    pub waits_on: Vec<String>,
    /// 1-based line in ROADMAP.md.
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub unspecified: Vec<Unspecified>,
}

/// `roadmap/nodes/<id>.json`. A missing or empty list is what the lint reports.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct SpecFile {
    pub id: Option<String>,
    #[serde(default)]
    pub verify: Vec<serde_json::Value>,
    #[serde(default)]
    pub files_allowed: Vec<String>,
    #[serde(default)]
    pub test_files: Vec<String>,
    #[serde(default)]
    pub code_map_entries: Vec<String>,
}

#[derive(Clone, Copy)]
enum Block {
    Nodes,
    Unspecified,
}

/// Parse the `nodes:` and `unspecified:` blocks of ROADMAP.md text. A malformed line is
/// `CoreError::Parse` naming the line number (1-based).
pub fn parse(roadmap_md: &str) -> Result<Graph, CoreError> {
    let mut graph = Graph::default();
    let mut block: Option<Block> = None;
    for (index, line) in roadmap_md.lines().enumerate() {
        let line_no = index + 1;
        match line.trim_end() {
            "nodes:" => {
                block = Some(Block::Nodes);
                continue;
            }
            "unspecified:" => {
                block = Some(Block::Unspecified);
                continue;
            }
            _ => {}
        }
        let Some(current) = block else { continue };
        let Some(item) = line.strip_prefix("- ") else {
            block = None;
            continue;
        };
        match current {
            Block::Nodes => graph.nodes.push(parse_node(item, line_no)?),
            Block::Unspecified => graph.unspecified.push(parse_unspecified(item, line_no)?),
        }
    }
    Ok(graph)
}

fn parse_node(item: &str, line: usize) -> Result<Node, CoreError> {
    let (id, rest) = split_id(item, line)?;
    let (title, attrs) = split_attrs(rest, line)?;
    let mut attrs = attributes(attrs, line)?;
    let feature = required(&mut attrs, "feature", line)?;
    let node_type = match required(&mut attrs, "type", line)?.as_str() {
        "build" => NodeType::Build,
        "research" => NodeType::Research,
        "prototype" => NodeType::Prototype,
        "grilling" => NodeType::Grilling,
        "task" => NodeType::Task,
        other => return Err(parse_error(line, format!("unknown type `{other}`"))),
    };
    let size = match required(&mut attrs, "size", line)?.as_str() {
        "easy" => Size::Easy,
        "medium" => Size::Medium,
        "hard" => Size::Hard,
        other => return Err(parse_error(line, format!("unknown size `{other}`"))),
    };
    let kind = attrs.remove("kind").map(str::to_string);
    let after = ids(&required(&mut attrs, "after", line)?);
    if let Some(key) = attrs.keys().next() {
        return Err(parse_error(line, format!("unknown attribute `{key}`")));
    }
    Ok(Node {
        id: id.to_string(),
        title: title.to_string(),
        feature,
        node_type,
        size,
        kind,
        after,
        line,
    })
}

fn parse_unspecified(item: &str, line: usize) -> Result<Unspecified, CoreError> {
    let (id, rest) = split_id(item, line)?;
    let (gist, attrs) = split_attrs(rest, line)?;
    let waits = attrs
        .strip_prefix("waits on: ")
        .ok_or_else(|| parse_error(line, "expected `(waits on: <ids>)`"))?;
    Ok(Unspecified {
        id: id.to_string(),
        gist: gist.to_string(),
        waits_on: ids(waits),
        line,
    })
}

fn parse_error(line: usize, message: impl std::fmt::Display) -> CoreError {
    CoreError::Parse(format!("ROADMAP.md line {line}: {message}"))
}

/// `<id>: <rest>`, the id being everything before the first `: `.
fn split_id(item: &str, line: usize) -> Result<(&str, &str), CoreError> {
    match item.split_once(": ") {
        Some((id, rest)) if !id.is_empty() => Ok((id, rest)),
        _ => Err(parse_error(line, "expected `- <id>: <text>`")),
    }
}

/// `text (attrs)`: the text before the last ` (` and the attributes inside the closing `)`.
fn split_attrs(text: &str, line: usize) -> Result<(&str, &str), CoreError> {
    let split = text
        .strip_suffix(')')
        .and_then(|head| head.rfind(" (").map(|open| (head, open)));
    match split {
        Some((head, open)) => Ok((head[..open].trim(), &head[open + 2..])),
        None => Err(parse_error(line, "expected a `(...)` list at the end")),
    }
}

/// `key: value, key: value` as a map; a key given twice is refused.
fn attributes(attrs: &str, line: usize) -> Result<HashMap<&str, &str>, CoreError> {
    let mut map = HashMap::new();
    for pair in attrs.split(", ") {
        let (key, value) = pair
            .split_once(": ")
            .ok_or_else(|| parse_error(line, format!("`{pair}` is not `key: value`")))?;
        if map.insert(key, value).is_some() {
            return Err(parse_error(
                line,
                format!("attribute `{key}` is given twice"),
            ));
        }
    }
    Ok(map)
}

fn required(attrs: &mut HashMap<&str, &str>, key: &str, line: usize) -> Result<String, CoreError> {
    attrs
        .remove(key)
        .map(str::to_string)
        .ok_or_else(|| parse_error(line, format!("missing attribute `{key}`")))
}

/// Space-separated ids; `none` is no ids.
fn ids(value: &str) -> Vec<String> {
    if value == "none" {
        return Vec::new();
    }
    value
        .split(' ')
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect()
}

/// Ids are lowercase letters, digits and hyphens: they name files under `roadmap/nodes/`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Read `roadmap/nodes/<id>.json`; the error text names the file and what is wrong with it.
/// Call only for an id that `valid_id` accepts.
pub fn read_spec(clone: &Path, id: &str) -> Result<SpecFile, String> {
    let path = clone
        .join("roadmap")
        .join("nodes")
        .join(format!("{id}.json"));
    let text = std::fs::read_to_string(&path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => format!("no spec file roadmap/nodes/{id}.json"),
        _ => format!("spec file roadmap/nodes/{id}.json: {error}"),
    })?;
    serde_json::from_str(&text)
        .map_err(|error| format!("spec file roadmap/nodes/{id}.json does not parse: {error}"))
}

fn finding(check: &'static str, id: &str, message: impl Into<String>) -> Finding {
    Finding {
        check,
        id: id.to_string(),
        message: message.into(),
    }
}

/// Every check over the graph. `features` are the ids of ROADMAP's features block; `specs` holds
/// one read result per node id that `valid_id` accepts.
pub fn check(
    graph: &Graph,
    features: &[String],
    specs: &HashMap<String, Result<SpecFile, String>>,
) -> LintReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let node_ids: HashSet<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    let feature_ids: HashSet<&str> = features.iter().map(String::as_str).collect();

    let listed = graph
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .chain(graph.unspecified.iter().map(|item| item.id.as_str()));
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for id in listed.clone() {
        *counts.entry(id).or_default() += 1;
    }
    let mut reported = HashSet::new();
    for id in listed {
        if counts[id] > 1 && reported.insert(id) {
            errors.push(finding(
                "duplicate_id",
                id,
                format!("`{id}` is listed {} times in ROADMAP.md", counts[id]),
            ));
        }
    }

    for node in &graph.nodes {
        let id = node.id.as_str();
        if !valid_id(id) {
            errors.push(finding(
                "invalid_id",
                id,
                "ids are lowercase letters, digits and hyphens",
            ));
            continue;
        }
        if !feature_ids.contains(node.feature.as_str()) {
            errors.push(finding(
                "unknown_feature",
                id,
                format!("feature `{}` is not in the features block", node.feature),
            ));
        }
        for dep in &node.after {
            if !node_ids.contains(dep.as_str()) {
                errors.push(finding(
                    "unknown_after",
                    id,
                    format!("after `{dep}` is not a node"),
                ));
            }
        }
        match specs.get(id) {
            Some(Ok(spec)) => check_spec(node, spec, &mut errors),
            Some(Err(why)) => errors.push(finding("spec_file", id, why.clone())),
            None => errors.push(finding(
                "spec_file",
                id,
                format!("no spec file roadmap/nodes/{id}.json"),
            )),
        }
    }

    errors.extend(cycles(graph, &node_ids));

    for item in &graph.unspecified {
        for wait in &item.waits_on {
            if !node_ids.contains(wait.as_str()) {
                errors.push(finding(
                    "unknown_wait",
                    &item.id,
                    format!("waits on `{wait}`, which is not a node"),
                ));
            }
        }
    }

    for feature in features {
        if !graph.nodes.iter().any(|node| &node.feature == feature) {
            warnings.push(finding(
                "feature_without_node",
                feature,
                format!("feature `{feature}` has no node yet"),
            ));
        }
    }

    LintReport {
        errors,
        warnings,
        nodes: graph.nodes.len(),
        unspecified: graph.unspecified.len(),
    }
}

fn check_spec(node: &Node, spec: &SpecFile, errors: &mut Vec<Finding>) {
    let id = node.id.as_str();
    if let Some(file_id) = &spec.id {
        if file_id != id {
            errors.push(finding(
                "spec_file",
                id,
                format!("spec file says id `{file_id}`"),
            ));
        }
    }
    if node.node_type != NodeType::Build {
        return;
    }
    let missing = [
        ("verify", spec.verify.is_empty()),
        ("files_allowed", spec.files_allowed.is_empty()),
        ("test_files", spec.test_files.is_empty()),
        ("code_map_entries", spec.code_map_entries.is_empty()),
    ];
    for (field, empty) in missing {
        if empty {
            errors.push(finding(
                "missing_field",
                id,
                format!("build node has no `{field}`"),
            ));
        }
    }
}

/// Every node that can reach itself through `after`, once per id, with the chain that loops.
fn cycles(graph: &Graph, node_ids: &HashSet<&str>) -> Vec<Finding> {
    let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in &graph.nodes {
        let deps = edges.entry(node.id.as_str()).or_default();
        deps.extend(
            node.after
                .iter()
                .map(String::as_str)
                .filter(|dep| node_ids.contains(dep)),
        );
    }
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for node in &graph.nodes {
        let start = node.id.as_str();
        if !seen.insert(start) {
            continue;
        }
        if let Some(path) = path_back_to(start, &edges) {
            found.push(finding(
                "cycle",
                start,
                format!("in a dependency cycle: {}", path.join(" after ")),
            ));
        }
    }
    found
}

/// The shortest `after` chain from `start` back to `start`, as ids: `start, dep, ..., start`.
fn path_back_to<'a>(
    start: &'a str,
    edges: &HashMap<&'a str, Vec<&'a str>>,
) -> Option<Vec<&'a str>> {
    let mut parent: HashMap<&str, &str> = HashMap::new();
    let mut visited: HashSet<&str> = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(current) = queue.pop_front() {
        for &next in edges.get(current).into_iter().flatten() {
            if next == start {
                let mut back = Vec::new();
                let mut at = current;
                while at != start {
                    back.push(at);
                    match parent.get(at) {
                        Some(&prev) => at = prev,
                        None => break,
                    }
                }
                let mut path = vec![start];
                path.extend(back.iter().rev());
                path.push(start);
                return Some(path);
            }
            if visited.insert(next) {
                parent.insert(next, current);
                queue.push_back(next);
            }
        }
    }
    None
}
