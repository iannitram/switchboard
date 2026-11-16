//! unit-job (#404): the fleet job and its brief for a claimed unit, written from the unit's node
//! spec file. The job carries the node's files, locked tests and verify commands; the brief
//! carries its what, its expectations and the code map lines of its entries, and nothing else.
//! A unit with no claim is refused.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::autopilot::passes::write_job;
use crate::error::CoreError;
use crate::roadmap::graph::Size;
use crate::unit::claim::Claim;

/// The fleet job kind of a unit: the brief names the files and the change, so the job is an
/// exact-brief change (`fleet`'s actions.json).
const KIND: &str = "small-code-change-exact-brief";

/// `roadmap/nodes/<id>.json`, the fields a job and its brief are made from.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeSpec {
    pub id: String,
    pub what: String,
    pub expectations: Vec<String>,
    #[serde(default)]
    pub files_allowed: Vec<String>,
    #[serde(default)]
    pub test_files: Vec<String>,
    #[serde(default)]
    pub verify: Vec<Value>,
    #[serde(default)]
    pub code_map_entries: Vec<String>,
}

/// A fleet job's JSON and the brief it points at.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub json: Value,
    pub brief: String,
}

/// Read `roadmap/nodes/<id>.json` under `clone`. The file must name the same id.
pub fn read_node(clone: &Path, id: &str) -> Result<NodeSpec, CoreError> {
    let path = clone
        .join("roadmap")
        .join("nodes")
        .join(format!("{id}.json"));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| CoreError::Io(format!("{}: {e}", path.display())))?;
    let node: NodeSpec = serde_json::from_str(&text)
        .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))?;
    if node.id != id {
        return Err(CoreError::Parse(format!(
            "{} names node {:?}, not {id:?}",
            path.display(),
            node.id
        )));
    }
    Ok(node)
}

/// The line of `code_map_md` for each entry, in entry order. An entry with no line is refused.
pub fn code_map_lines(code_map_md: &str, entries: &[String]) -> Result<Vec<String>, CoreError> {
    entries
        .iter()
        .map(|entry| {
            let start = format!("`{entry}`");
            code_map_md
                .lines()
                .find(|line| line.starts_with(start.as_str()))
                .map(str::to_string)
                .ok_or_else(|| {
                    CoreError::Refused(format!("docs/code-map.md has no line for {entry}"))
                })
        })
        .collect()
}

/// The job and brief for `node`, a unit claimed by `claim`; `size` is its size on the roadmap and
/// `repo` the repo its issue lives in. Refused without a claim.
pub fn build(
    node: &NodeSpec,
    size: Size,
    claim: Option<&Claim>,
    repo: &str,
    code_map_md: &str,
) -> Result<Job, CoreError> {
    let claim = claim.ok_or_else(|| {
        CoreError::Refused(format!("unit {} has no claim; claim it first", node.id))
    })?;
    let lines = code_map_lines(code_map_md, &node.code_map_entries)?;
    let issue = claim.issue;
    let json = json!({
        "id": format!("unit-{issue}-{}", node.id),
        "title": format!("Build {} (#{issue})", node.id),
        "kind": KIND,
        "size": size_letter(size),
        "repo": repo,
        "base": "main",
        "est_minutes": est_minutes(size),
        "issue": issue,
        "lane": null,
        "verify": node.verify,
        "files_allowed": node.files_allowed,
        "test_files": node.test_files,
        "brief": "exact",
    });
    Ok(Job {
        json,
        brief: brief(node, &lines),
    })
}

/// Write the job as `<jobs_dir>/<id>.json` and its brief as `<jobs_dir>/<id>.md`, the paths
/// `fleet run` reads. Answers the job file's path.
pub fn write(job: &Job, jobs_dir: &Path, dev_root: &Path) -> Result<PathBuf, CoreError> {
    write_job(jobs_dir, dev_root, &job.json, &job.brief)
}

fn size_letter(size: Size) -> &'static str {
    match size {
        Size::Easy => "S",
        Size::Medium => "M",
        Size::Hard => "L",
    }
}

fn est_minutes(size: Size) -> u64 {
    match size {
        Size::Easy => 20,
        Size::Medium => 45,
        Size::Hard => 90,
    }
}

/// The brief: the node's what, its expectations, its locked test files and its code map lines.
fn brief(node: &NodeSpec, code_map_lines: &[String]) -> String {
    let mut out = format!(
        "# {}\n\n## What\n\n{}\n\n## Expectations\n\n",
        node.id, node.what
    );
    for expectation in &node.expectations {
        let _ = writeln!(out, "- {expectation}");
    }
    out.push_str("\n## Locked test files\n\n");
    for test in &node.test_files {
        let _ = writeln!(out, "- {test}");
    }
    out.push_str("\n## Code map\n\n");
    for line in code_map_lines {
        let _ = writeln!(out, "{line}");
    }
    out
}
