//! `backup`: one latest backup of each repo in `Backups/github/<repo>/` in NextCloud (BAK-1),
//! guarded against a bad pass overwriting the last good copy (BAK-2), restorable to a new
//! private repo (BAK-3).

use crate::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    /// PRs merged, of `pulls`.
    #[serde(default)]
    pub merged_pulls: u64,
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
        merged_pulls: answers
            .pulls
            .iter()
            .filter(|p| p["merged_at"].is_string())
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

/// BAK-1: a release file of this many bytes or more is listed but not fetched.
pub const RELEASE_FILE_LIMIT: u64 = 1_000_000_000;

/// BAK-1: the releases as `releases.json`, every asset of 1 GB or more marked skipped; sets
/// `manifest.releases`.
pub fn write_release_list(
    releases: &[Value],
    out_dir: &Path,
    manifest: &mut Manifest,
) -> Result<(), CoreError> {
    let mut list = releases.to_vec();
    for release in &mut list {
        if let Some(assets) = release.get_mut("assets").and_then(Value::as_array_mut) {
            for asset in assets {
                if asset["size"]
                    .as_u64()
                    .is_some_and(|s| s >= RELEASE_FILE_LIMIT)
                {
                    if let Some(asset) = asset.as_object_mut() {
                        asset.insert("skipped".to_string(), Value::from("over 1 GB"));
                    }
                }
            }
        }
    }
    let text = pretty(&list)?;
    std::fs::create_dir_all(out_dir).map_err(|e| io_error(out_dir, e))?;
    let tmp = out_dir.join("releases.json.new");
    if let Err(e) = std::fs::write(&tmp, text) {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_error(&tmp, e));
    }
    if let Err(e) = std::fs::rename(&tmp, out_dir.join("releases.json")) {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_error(&tmp, e));
    }
    manifest.releases = releases.len() as u64;
    Ok(())
}

fn pretty<T: Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string_pretty(value).map_err(|e| CoreError::Parse(e.to_string()))
}

fn io_error(path: &Path, e: std::io::Error) -> CoreError {
    CoreError::Io(format!("{}: {e}", path.display()))
}

/// BAK-1: bundle every ref of the mirror; verified; returns the ref count.
///
/// Runs `git bundle create <out_dir>/repo.bundle.new --all` in the mirror clone, then
/// `git bundle verify` on it, then renames it over `repo.bundle`. `out_dir` is made if missing.
/// A mirror with no refs, a failed create or a failed verify is an error; the half-written
/// `.new` file is removed and any older `repo.bundle` is left as it was.
pub fn bundle(mirror: &Path, out_dir: &Path) -> Result<u64, CoreError> {
    let refs = git(mirror, &["for-each-ref", "--format=%(refname)"])?;
    if refs.lines().all(|line| line.trim().is_empty()) {
        return Err(CoreError::Io(format!(
            "{}: the mirror has no refs to bundle",
            mirror.display()
        )));
    }
    // git runs with `-C <mirror>`, so a relative `out_dir` would land inside the mirror.
    let out_dir = &std::path::absolute(out_dir).map_err(|e| io_error(out_dir, e))?;
    std::fs::create_dir_all(out_dir).map_err(|e| io_error(out_dir, e))?;
    let tmp = out_dir.join("repo.bundle.new");
    let _ = std::fs::remove_file(&tmp);
    let made = bundle_new(mirror, &tmp);
    let count = match made {
        Ok(count) => count,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    let file = out_dir.join("repo.bundle");
    if let Err(e) = std::fs::rename(&tmp, &file) {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_error(&tmp, e));
    }
    Ok(count)
}

/// Writes and verifies the bundle at `tmp`; returns the number of refs it holds (`HEAD` aside).
fn bundle_new(mirror: &Path, tmp: &Path) -> Result<u64, CoreError> {
    let tmp_arg = tmp.to_string_lossy();
    git(mirror, &["bundle", "create", "-q", &tmp_arg, "--all"])?;
    git(mirror, &["bundle", "verify", "-q", &tmp_arg])?;
    let heads = git(mirror, &["bundle", "list-heads", &tmp_arg])?;
    let count = heads
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter(|name| *name != "HEAD")
        .collect::<HashSet<_>>()
        .len();
    if count == 0 {
        return Err(CoreError::Io(format!(
            "{}: the bundle holds no refs",
            tmp.display()
        )));
    }
    Ok(count as u64)
}

/// Runs `git -C <dir> <args>`; its stdout, or the command and its stderr as the error.
fn git(dir: &Path, args: &[&str]) -> Result<String, CoreError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| CoreError::Io(format!("git {}: {e}", args.join(" "))))?;
    if !out.status.success() {
        return Err(CoreError::Io(format!(
            "git {} in {}: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// BAK-2: why the new backup may not replace the old one, or `None` when it may. A change is
/// "far" when it is more than 10% of the old backup's count, or more than 5 (SPEC.md BAK-2).
/// Checked: fewer issues, PRs or labels in all; fewer open issues or open PRs; more closed
/// issues no merged PR closed; fewer label assignments.
///
/// Merges are not losses (owner ruling on #92, 2026-11-08): open issues count only the ones
/// lost to something other than a merged PR (issues newly closed and not added to
/// `closed_without_pr` were closed by merged PRs), and open PRs count only the ones lost to
/// something other than a merge (PRs newly merged left the open count by merging).
pub fn refuse_reason(old: &Manifest, new: &Manifest) -> Option<String> {
    let all = |m: &Manifest| i128::from(m.issues) + i128::from(m.pulls) + i128::from(m.labels);
    if far(all(old), all(old) - all(new)) {
        return Some(said("issues, PRs and labels in all", all(new), all(old)));
    }

    let open = |m: &Manifest| i128::from(m.open_issues);
    let closed = |m: &Manifest| i128::from(m.issues) - open(m);
    let without_pr = |m: &Manifest| i128::from(m.closed_without_pr);
    let closed_by_merged_pr =
        ((closed(new) - closed(old)) - (without_pr(new) - without_pr(old))).max(0);
    if far(open(old), open(old) - open(new) - closed_by_merged_pr) {
        return Some(said("open issues", open(new), open(old)));
    }

    let open_pulls = |m: &Manifest| i128::from(m.open_pulls);
    let merged = |m: &Manifest| i128::from(m.merged_pulls);
    let newly_merged = (merged(new) - merged(old)).max(0);
    if far(
        open_pulls(old),
        open_pulls(old) - open_pulls(new) - newly_merged,
    ) {
        return Some(said("open PRs", open_pulls(new), open_pulls(old)));
    }

    if far(without_pr(old), without_pr(new) - without_pr(old)) {
        return Some(said(
            "issues closed by no merged PR",
            without_pr(new),
            without_pr(old),
        ));
    }

    let assignments = |m: &Manifest| i128::from(m.label_assignments);
    if far(assignments(old), assignments(old) - assignments(new)) {
        return Some(said(
            "label assignments",
            assignments(new),
            assignments(old),
        ));
    }

    None
}

/// A change of `by` (in the bad direction) on a count of `old` is far: more than 5, or more
/// than 10% of `old`.
fn far(old: i128, by: i128) -> bool {
    by > 5 || by * 10 > old
}

/// The refusal in my words: "the new backup has 12 open issues, the last had 40".
fn said(what: &str, new: i128, old: i128) -> String {
    format!("the new backup has {new} {what}, the last had {old}")
}
