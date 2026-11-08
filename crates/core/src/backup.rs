//! `backup`: one latest backup of each repo in `Backups/github/<repo>/` in NextCloud (BAK-1),
//! guarded against a bad pass overwriting the last good copy (BAK-2), restorable to a new
//! private repo (BAK-3).

use crate::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The manifest BAK-1 writes beside the bundle and the JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub repo: String,
    /// RFC 3339.
    pub taken_at: String,
    pub refs: u64,
    pub issues: u64,
    pub open_issues: u64,
    pub pulls: u64,
    pub open_pulls: u64,
    pub comments: u64,
    pub labels: u64,
    /// Label assignments summed over every issue.
    pub label_assignments: u64,
    pub milestones: u64,
    pub releases: u64,
    /// Closed issues no merged PR closed.
    pub closed_without_pr: u64,
    pub wiki: bool,
}

/// The recorded answers `write_json` writes out, as the REST JSON arrays GitHub returned. Issues
/// carry `number`, `state` (`open` or `closed`) and `labels` (`[{"name": ...}]`); pulls carry
/// `number`, `state`, `merged_at` (a string once merged, else null) and `body`. The issues array
/// holds issues only, not PRs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordedAnswers {
    /// `owner/name`.
    pub repo: String,
    pub issues: Vec<Value>,
    pub pulls: Vec<Value>,
    pub comments: Vec<Value>,
    pub labels: Vec<Value>,
    pub milestones: Vec<Value>,
}

/// BAK-1: issues, PRs, comments, labels, milestones as JSON plus the manifest counts. Each file
/// is written to `<name>.new` first; all six are renamed into place only once every one is
/// written, so a failure before that leaves the old files untouched. Reads nothing but `answers`.
pub fn write_json(
    answers: &RecordedAnswers,
    out_dir: &Path,
    taken_at: &str,
) -> Result<Manifest, CoreError> {
    let manifest = manifest_for(answers, taken_at);
    let files = [
        ("issues.json", pretty(&answers.issues)?),
        ("pulls.json", pretty(&answers.pulls)?),
        ("comments.json", pretty(&answers.comments)?),
        ("labels.json", pretty(&answers.labels)?),
        ("milestones.json", pretty(&answers.milestones)?),
        ("manifest.json", pretty(&manifest)?),
    ];
    std::fs::create_dir_all(out_dir).map_err(|e| io_error(out_dir, e))?;
    let mut written: Vec<PathBuf> = Vec::new();
    for (name, text) in &files {
        let tmp = out_dir.join(format!("{name}.new"));
        if let Err(e) = std::fs::write(&tmp, text) {
            let _ = std::fs::remove_file(&tmp);
            for done in &written {
                let _ = std::fs::remove_file(done);
            }
            return Err(io_error(&tmp, e));
        }
        written.push(tmp);
    }
    for (name, _) in &files {
        let tmp = out_dir.join(format!("{name}.new"));
        std::fs::rename(&tmp, out_dir.join(name)).map_err(|e| io_error(&tmp, e))?;
    }
    Ok(manifest)
}

/// The counts for `answers`. `refs`, `releases` and `wiki` are left at their defaults.
fn manifest_for(answers: &RecordedAnswers, taken_at: &str) -> Manifest {
    let closed_by_merged_pr: HashSet<u64> = answers
        .pulls
        .iter()
        .filter(|pull| pull["merged_at"].is_string())
        .flat_map(|pull| closes_of(pull["body"].as_str().unwrap_or("")))
        .collect();
    let closed_without_pr = answers
        .issues
        .iter()
        .filter(|issue| has_state(issue, "closed"))
        .filter(|issue| {
            !issue["number"]
                .as_u64()
                .is_some_and(|n| closed_by_merged_pr.contains(&n))
        })
        .count();
    Manifest {
        repo: answers.repo.clone(),
        taken_at: taken_at.to_string(),
        issues: answers.issues.len() as u64,
        open_issues: answers
            .issues
            .iter()
            .filter(|i| has_state(i, "open"))
            .count() as u64,
        pulls: answers.pulls.len() as u64,
        open_pulls: answers
            .pulls
            .iter()
            .filter(|p| has_state(p, "open"))
            .count() as u64,
        comments: answers.comments.len() as u64,
        labels: answers.labels.len() as u64,
        label_assignments: answers.issues.iter().map(label_count).sum(),
        milestones: answers.milestones.len() as u64,
        closed_without_pr: closed_without_pr as u64,
        ..Manifest::default()
    }
}

fn has_state(item: &Value, state: &str) -> bool {
    item["state"].as_str() == Some(state)
}

fn label_count(issue: &Value) -> u64 {
    issue["labels"]
        .as_array()
        .map_or(0, |labels| labels.len() as u64)
}

/// The issue numbers a PR body closes with `Closes #N` or `Fixes #N` (any case, optional colon).
fn closes_of(body: &str) -> Vec<u64> {
    let words: Vec<&str> = body.split_whitespace().collect();
    let mut numbers = Vec::new();
    for pair in words.windows(2) {
        let keyword = pair[0].trim_end_matches(':').to_ascii_lowercase();
        if keyword != "closes" && keyword != "fixes" {
            continue;
        }
        if let Some(rest) = pair[1].strip_prefix('#') {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse() {
                numbers.push(n);
            }
        }
    }
    numbers
}

fn pretty<T: Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string_pretty(value).map_err(|e| CoreError::Parse(e.to_string()))
}

fn io_error(path: &Path, e: std::io::Error) -> CoreError {
    CoreError::Io(format!("{}: {e}", path.display()))
}

/// BAK-1: `git bundle create <dir>/repo.bundle --all` from a mirror clone, then
/// `git bundle verify`.
pub fn bundle(mirror: &Path, out_dir: &Path) -> Result<u64, CoreError> {
    todo!("BAK-1")
}

/// BAK-2: why the new backup may not replace the old one, or `None` when it may. A change is
/// "far" when it is more than 10% of the old backup's count, or more than 5 (SPEC.md BAK-2).
/// Checked: fewer issues, PRs or labels in all; fewer open issues or open PRs; more closed
/// issues no merged PR closed; fewer label assignments.
pub fn refuse_reason(old: &Manifest, new: &Manifest) -> Option<String> {
    todo!("BAK-2")
}
