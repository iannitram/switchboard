//! unit-land (#406): the last steps of one unit. `open_pr` checks the code map on the branch head,
//! pushes the branch and opens the pull request; its body carries each verify command of the node
//! with its result from the done card. `resolve` writes the resolution record on the issue, closes
//! it and releases the claim. Git, GitHub and the claim reach this module through small traits and
//! it starts no process of its own.

use std::fmt::Write as _;

use serde_json::Value;

use crate::error::CoreError;
use crate::sky::codemap::{self, Touched};
use crate::unit::claim::{self, Mirror, Store};
use crate::unit::job::NodeSpec;

/// The first line of a resolution record. `resolve` writes one only when no comment starts with it.
pub const RECORD_MARK: &str = "<!-- switchboard:resolution -->";

/// The code map, read at both ends of the change.
const CODE_MAP: &str = "docs/code-map.md";

/// The local clone the branch is in.
pub trait Git {
    /// The files the change from `base` to `head` touches; `added` when it creates them.
    fn touched(&self, base: &str, head: &str) -> Result<Vec<Touched>, CoreError>;
    /// The text of `path` at `rev`.
    fn show(&self, rev: &str, path: &str) -> Result<String, CoreError>;
    /// Push `branch` to its remote.
    fn push(&self, branch: &str) -> Result<(), CoreError>;
}

/// The GitHub side: the pull request, and the issue's comments and state.
pub trait Forge {
    /// Open a pull request from `head` into `base`; its number.
    fn open_pr(
        &self,
        repo: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<u64, CoreError>;
    /// The bodies of the issue's comments, oldest first.
    fn comments(&self, repo: &str, issue: u64) -> Result<Vec<String>, CoreError>;
    fn comment(&self, repo: &str, issue: u64, body: &str) -> Result<(), CoreError>;
    fn close(&self, repo: &str, issue: u64) -> Result<(), CoreError>;
}

/// What `open_pr` needs of the unit: its repo and issue, its node, the branch and its base, and
/// the done card of the job that built it.
pub struct Pr<'a> {
    pub repo: &'a str,
    pub issue: u64,
    pub node: &'a NodeSpec,
    pub base: &'a str,
    pub head: &'a str,
    pub done_card: &'a Value,
}

/// What `resolve` did: whether it wrote the record and whether a claim file was there to release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub record_written: bool,
    pub claim_released: bool,
}

/// One verify command of the node with the result the done card gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifyResult {
    cmd: String,
    name: String,
    result: String,
}

/// Check the code map on the branch head, push the branch and open the pull request; its number.
/// Refused, with nothing pushed and no pull request opened, when the code map check fails or a
/// verify command has no result on the done card.
pub fn open_pr(pr: &Pr, git: &dyn Git, forge: &dyn Forge) -> Result<u64, CoreError> {
    check_code_map(pr, git)?;
    let body = pr_body(pr.issue, pr.node, pr.done_card)?;
    git.push(pr.head)?;
    let title = format!("Build {} (#{})", pr.node.id, pr.issue);
    forge.open_pr(pr.repo, pr.head, pr.base, &title, &body)
}

/// Write the resolution record on `issue` (unless one is there already), close it and release its
/// claim. Running it again changes nothing once each step has been done.
pub fn resolve(
    repo: &str,
    issue: u64,
    pr_number: u64,
    store: &Store,
    mirror: &dyn Mirror,
    forge: &dyn Forge,
) -> Result<Resolved, CoreError> {
    let recorded = forge
        .comments(repo, issue)?
        .iter()
        .any(|body| body.starts_with(RECORD_MARK));
    if !recorded {
        forge.comment(repo, issue, &resolution_record(issue, pr_number))?;
    }
    forge.close(repo, issue)?;
    let claim_released = release_claim(store, mirror, repo, issue)?;
    Ok(Resolved {
        record_written: !recorded,
        claim_released,
    })
}

/// The pull request body: the issue it closes, then each verify command of the node and its
/// result from the done card.
pub fn pr_body(issue: u64, node: &NodeSpec, done_card: &Value) -> Result<String, CoreError> {
    let mut out = format!("Closes #{issue}\n\n## Verify\n\n");
    for verify in verify_results(node, done_card)? {
        let _ = writeln!(
            out,
            "- `{}` ({}): {}",
            verify.cmd, verify.name, verify.result
        );
    }
    Ok(out)
}

/// The resolution record: the marker, then the pull request that resolved the issue.
pub fn resolution_record(issue: u64, pr_number: u64) -> String {
    format!("{RECORD_MARK}\nResolved #{issue} by pull request #{pr_number}.")
}

/// Each verify of the node with the done card's check of the same name. A verify the card has no
/// check for, or no status for, is refused.
fn verify_results(node: &NodeSpec, done_card: &Value) -> Result<Vec<VerifyResult>, CoreError> {
    let checks = done_card
        .get("checks")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    node.verify
        .iter()
        .map(|verify| {
            let name = verify
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| CoreError::Parse(format!("{}: a verify has no name", node.id)))?;
            let cmd = verify.get("cmd").and_then(Value::as_str).ok_or_else(|| {
                CoreError::Parse(format!("{}: verify {name} has no cmd", node.id))
            })?;
            let check = checks
                .iter()
                .find(|check| check.get("name").and_then(Value::as_str) == Some(name))
                .ok_or_else(|| {
                    CoreError::Refused(format!("the done card has no result for verify {name}"))
                })?;
            let status = check.get("status").and_then(Value::as_str).ok_or_else(|| {
                CoreError::Refused(format!("the done card's verify {name} has no status"))
            })?;
            let result = match check.get("note").and_then(Value::as_str) {
                Some(note) if !note.is_empty() => format!("{status} ({note})"),
                _ => status.to_string(),
            };
            Ok(VerifyResult {
                cmd: cmd.to_string(),
                name: name.to_string(),
                result,
            })
        })
        .collect()
}

/// The code map check on the branch head, read against the base: refused with its findings.
fn check_code_map(pr: &Pr, git: &dyn Git) -> Result<(), CoreError> {
    let touched = git.touched(pr.base, pr.head)?;
    let base_md = git.show(pr.base, CODE_MAP)?;
    let head_md = git.show(pr.head, CODE_MAP)?;
    let findings = codemap::check_modules(&pr.node.id, &touched, &base_md, &head_md);
    if findings.is_empty() {
        return Ok(());
    }
    let why: Vec<&str> = findings.iter().map(|f| f.message.as_str()).collect();
    Err(CoreError::Refused(format!(
        "the code map check fails on {}: {}",
        pr.head,
        why.join("; ")
    )))
}

/// The one place this module releases a claim. `claim::release` takes no repo yet; the claim
/// review (#480) adds it, and this is where the repo goes then.
fn release_claim(
    store: &Store,
    mirror: &dyn Mirror,
    _repo: &str,
    issue: u64,
) -> Result<bool, CoreError> {
    claim::release(store, mirror, issue)
}
