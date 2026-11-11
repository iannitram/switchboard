//! `backup`: one latest backup of each repo in `Backups/github/<repo>/` in NextCloud (BAK-1),
//! guarded against a bad pass overwriting the last good copy (BAK-2), restorable to a new
//! private repo (BAK-3: history, labels and milestones in `restore_history`, issues and comments
//! in `restore_issues`).

use crate::fleet::FleetBridge;
use crate::github::queue::{self, Create, QueueStatus, Queued};
use crate::github::transport::{GitHubTransport, HttpTransport};
use crate::renumber::{renumber, Now};
use crate::rules::{self, Caller, GuardedAction, Verdict};
use crate::settings::Settings;
use crate::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

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
    /// `git hash-object` of `repo.bundle`; empty in a backup written before the manifest had it.
    #[serde(default)]
    pub bundle_hash: String,
    /// The repo GitHub answered for, when it is not `repo`: the repo was renamed or moved before
    /// this backup, and GitHub answered its old name with the items of the new one. A restore
    /// accepts items of either name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_repo: Option<String>,
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
    let mut manifest = manifest_for(answers, taken_at);
    manifest.resolved_repo = resolved_repo(answers)?;
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

/// The repo the API addresses in `answers` are for, when it is not `answers.repo`: GitHub answers
/// the old name of a renamed or moved repo with the items of the new one. Items of more than one
/// other repo are an error, because no restore could tell which repo the backup is of.
fn resolved_repo(answers: &RecordedAnswers) -> Result<Option<String>, CoreError> {
    let mut others: Vec<String> = [
        &answers.issues,
        &answers.pulls,
        &answers.comments,
        &answers.labels,
        &answers.milestones,
    ]
    .into_iter()
    .flatten()
    .filter_map(repo_of)
    .filter(|of| !of.eq_ignore_ascii_case(&answers.repo))
    .collect();
    others.sort();
    others.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    if others.len() > 1 {
        return Err(CoreError::GitHub(format!(
            "{}: GitHub answered with items of more than one other repo ({})",
            answers.repo,
            others.join(", ")
        )));
    }
    Ok(others.pop())
}

/// The counts for `answers`. `refs`, `releases`, `wiki` and `resolved_repo` are left at their
/// defaults.
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
                    .as_f64()
                    .is_some_and(|s| s >= RELEASE_FILE_LIMIT as f64)
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

/// Where release file bytes come from: the network in the app, a stub in tests.
pub trait FileSource {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, CoreError>;
}

/// A tag or file name as one safe path part: `/` and `\` become `_`, and a result that is
/// empty, `.` or `..` becomes `_`.
fn safe_part(text: &str) -> String {
    let s = text.replace(['/', '\\'], "_");
    if s.is_empty() || s == "." || s == ".." {
        "_".to_string()
    } else {
        s
    }
}

/// Fetches every asset under 1 GB into `staging/<tag>/<name>`; returns the count.
fn fetch_into(
    releases: &[Value],
    files: &dyn FileSource,
    staging: &Path,
) -> Result<u64, CoreError> {
    let mut count = 0;
    for release in releases {
        let Some(assets) = release.get("assets").and_then(Value::as_array) else {
            continue;
        };
        let tag = release["tag_name"]
            .as_str()
            .ok_or_else(|| CoreError::Parse("a release has no tag_name".to_string()))?;
        for asset in assets {
            if asset["size"]
                .as_f64()
                .is_some_and(|s| s >= RELEASE_FILE_LIMIT as f64)
            {
                continue;
            }
            let name = asset["name"]
                .as_str()
                .ok_or_else(|| CoreError::Parse(format!("release {tag}: an asset has no name")))?;
            // The API address reads a private repo's file with the token; the browser one may not.
            let url = asset["url"]
                .as_str()
                .or_else(|| asset["browser_download_url"].as_str())
                .ok_or_else(|| {
                    CoreError::Parse(format!("release {tag}: asset {name} has no download URL"))
                })?;
            let bytes = files.fetch(url)?;
            // A download cut short must not be kept as if it were the whole file.
            if let Some(size) = asset["size"].as_u64() {
                if bytes.len() as u64 != size {
                    return Err(CoreError::GitHub(format!(
                        "release {tag}: {name} came as {} bytes, the release says {size}",
                        bytes.len()
                    )));
                }
            }
            let dir = staging.join(safe_part(tag));
            std::fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
            let path = dir.join(safe_part(name));
            std::fs::write(&path, bytes).map_err(|e| io_error(&path, e))?;
            count += 1;
        }
    }
    Ok(count)
}

/// BAK-1: every release file under 1 GB into `releases/<tag>/<name>`; returns the count.
/// Files go to `releases.new/` first and replace `releases/` only when all of them landed.
pub fn fetch_release_files(
    releases: &[Value],
    files: &dyn FileSource,
    out_dir: &Path,
) -> Result<u64, CoreError> {
    let staging = out_dir.join("releases.new");
    let live = out_dir.join("releases");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| io_error(&staging, e))?;
    let result = fetch_into(releases, files, &staging).and_then(|count| {
        if live.exists() {
            std::fs::remove_dir_all(&live).map_err(|e| io_error(&live, e))?;
        }
        std::fs::rename(&staging, &live).map_err(|e| io_error(&staging, e))?;
        Ok(count)
    });
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// BAK-1: releases as `releases.json` plus their files under 1 GB; sets `manifest.releases`.
/// The files are fetched first, so a failed fetch changes neither.
pub fn write_releases(
    releases: &[Value],
    files: &dyn FileSource,
    out_dir: &Path,
    manifest: &mut Manifest,
) -> Result<(), CoreError> {
    fetch_release_files(releases, files, out_dir)?;
    write_release_list(releases, out_dir, manifest)
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

/// BAK-1: the wiki as a bare mirror in `wiki.git`; sets `manifest.wiki`.
///
/// `wiki_mirror` is a bare mirror of `<repo>.wiki.git` the caller already cloned, or `None` for a
/// repo with no wiki: then `manifest.wiki` is false and nothing is written, not even `out_dir`.
/// Otherwise `git clone --mirror` copies it into `wiki.git.new`, which is renamed over
/// `wiki.git`. A failed copy removes the `.new` copy, returns the error and leaves the old
/// `wiki.git` and `manifest.wiki` as they were. Makes no network calls.
pub fn copy_wiki(
    wiki_mirror: Option<&Path>,
    out_dir: &Path,
    manifest: &mut Manifest,
) -> Result<(), CoreError> {
    let Some(mirror) = wiki_mirror else {
        manifest.wiki = false;
        return Ok(());
    };
    // git runs with `-C <out_dir>`, so both paths must be absolute.
    let mirror = std::path::absolute(mirror).map_err(|e| io_error(mirror, e))?;
    let out_dir = std::path::absolute(out_dir).map_err(|e| io_error(out_dir, e))?;
    std::fs::create_dir_all(&out_dir).map_err(|e| io_error(&out_dir, e))?;
    let tmp = out_dir.join("wiki.git.new");
    let _ = std::fs::remove_dir_all(&tmp);
    let mirror_arg = mirror.to_string_lossy();
    let tmp_arg = tmp.to_string_lossy();
    if let Err(e) = git(
        &out_dir,
        &["clone", "-q", "--mirror", &mirror_arg, &tmp_arg],
    ) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let copy = out_dir.join("wiki.git");
    if copy.exists() {
        std::fs::remove_dir_all(&copy).map_err(|e| io_error(&copy, e))?;
    }
    std::fs::rename(&tmp, &copy).map_err(|e| io_error(&tmp, e))?;
    manifest.wiki = true;
    Ok(())
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

/// BAK-1 and BAK-2: what one backup did. `Refused` holds the reason for the Waiting on you item
/// (`refuse_reason`'s text, a wiki the last backup had and this one could not read, or a last
/// backup whose manifest is damaged or missing); the last backup was kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupOutcome {
    Written(Manifest),
    Refused(String),
}

/// Where a backup reads a repo from: git clones `<git_base>/<repo>.git` and
/// `<git_base>/<repo>.wiki.git`, with each `git_config` entry passed as `-c`, the REST API
/// through `api`, and release files through `files`. The app uses `https://github.com`,
/// `HttpTransport` and `HttpFiles`; tests use a folder of bare repos, recorded answers and a
/// stub file source.
pub struct Remote<'a> {
    pub git_base: String,
    pub git_config: Vec<String>,
    pub api: &'a dyn GitHubTransport,
    pub files: &'a dyn FileSource,
}

/// Release files over HTTPS. The GitHub token goes to GitHub's own two hosts only, and is not
/// sent on to the storage host a download redirects to.
pub struct HttpFiles {
    token: String,
    agent: ureq::Agent,
}

impl HttpFiles {
    /// The file source for the token the platform's store holds for GitHub.
    pub fn new(tokens: &dyn crate::platform::TokenStore) -> Result<Self, CoreError> {
        let token = tokens
            .get("github")
            .ok_or_else(|| CoreError::GitHub("no GitHub token: run `gh auth login`".to_string()))?;
        Ok(Self {
            token,
            agent: ureq::AgentBuilder::new()
                .timeout_connect(std::time::Duration::from_secs(30))
                .timeout_read(std::time::Duration::from_secs(120))
                .build(),
        })
    }
}

impl FileSource for HttpFiles {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, CoreError> {
        use std::io::Read;
        let failed = |why: String| CoreError::GitHub(format!("GET {url}: {why}"));
        let mut request = self
            .agent
            .get(url)
            .set("User-Agent", "switchboard")
            .set("Accept", "application/octet-stream");
        if url.starts_with("https://api.github.com/") || url.starts_with("https://github.com/") {
            request = request.set("Authorization", &format!("Bearer {}", self.token));
        }
        let answer = request.call().map_err(|e| failed(e.to_string()))?;
        let mut bytes = Vec::new();
        answer
            .into_reader()
            .take(RELEASE_FILE_LIMIT)
            .read_to_end(&mut bytes)
            .map_err(|e| failed(e.to_string()))?;
        Ok(bytes)
    }
}

/// Runs `job` against GitHub: git over `https://github.com` with `gh` as the credential helper,
/// the API and the release files with `gh auth token`.
fn with_github<T>(job: impl FnOnce(&Remote<'_>) -> Result<T, CoreError>) -> Result<T, CoreError> {
    let tokens = crate::platform::GhAuthToken::default();
    let api = HttpTransport::new(&tokens)?;
    let files = HttpFiles::new(&tokens)?;
    job(&Remote {
        git_base: "https://github.com".to_string(),
        git_config: vec![
            "credential.helper=".to_string(),
            "credential.helper=!gh auth git-credential".to_string(),
        ],
        api: &api,
        files: &files,
    })
}

/// BAK-1: back up `repo` (`owner/name`) from GitHub into
/// `<nextcloud_backups>/github/<owner>/<name>/`, replacing the last backup unless BAK-2 refuses.
/// Git reads use `gh` as the credential helper and the API uses `gh auth token`.
pub fn backup_repo(settings: &Settings, repo: &str) -> Result<BackupOutcome, CoreError> {
    backups_dir(settings)?;
    with_github(|remote| backup_repo_from(settings, repo, remote, &now_rfc3339()))
}

/// One repo of a "back up all" and what its backup did, or the error that stopped it.
pub type RepoBackup = (String, Result<BackupOutcome, CoreError>);

/// BAK-1: back up every repo in `repos`, in order, each as `backup_repo` does. Every repo is
/// tried: one that fails holds its error, which names the repo, and the others still run. The
/// outer error is for what stops them all (no backups folder, no GitHub token).
pub fn backup_all(settings: &Settings, repos: &[String]) -> Result<Vec<RepoBackup>, CoreError> {
    backups_dir(settings)?;
    with_github(|remote| backup_all_from(settings, repos, remote, &now_rfc3339()))
}

/// `backup_all` against `remote`, every manifest stamped `taken_at`.
pub fn backup_all_from(
    settings: &Settings,
    repos: &[String],
    remote: &Remote<'_>,
    taken_at: &str,
) -> Result<Vec<RepoBackup>, CoreError> {
    backups_dir(settings)?;
    Ok(repos
        .iter()
        .map(|repo| {
            let outcome =
                backup_repo_from(settings, repo, remote, taken_at).map_err(|e| about(repo, e));
            (repo.clone(), outcome)
        })
        .collect())
}

/// The lock that keeps two backups of one folder in this process apart.
fn repo_lock(folder: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    locks.entry(folder.to_path_buf()).or_default().clone()
}

/// `backup_repo` against `remote`, the manifest stamped `taken_at` (RFC 3339).
///
/// One backup of a repo runs at a time in this process: a second one waits for the first. A last
/// backup whose manifest is damaged or missing is `Refused` before anything is read. Then the
/// repo is mirrored into a temp folder and the new backup is built beside the old one in a
/// staging folder of this run's own (`.<name>.new-<run>`, so a backup in another process cannot
/// clear it): `bundle`, `write_json`, `write_releases`, `copy_wiki`, and the manifest with the
/// ref count, the bundle's hash and the wiki. Asks `refuse_reason` against the old folder's
/// manifest, and refuses a new backup with no wiki when the old one has one; a refusal removes
/// the new folder and leaves the old one untouched. Otherwise the old folder moves to
/// `.<name>.old-<run>`, the new one takes its place, and the old one is removed. Any error before
/// the swap leaves the old folder as it was.
pub fn backup_repo_from(
    settings: &Settings,
    repo: &str,
    remote: &Remote<'_>,
    taken_at: &str,
) -> Result<BackupOutcome, CoreError> {
    let backups = backups_dir(settings)?;
    let (owner, name) = split_repo(repo)?;
    let parent = backups.join("github").join(owner);
    let folder = parent.join(name);
    let lock = repo_lock(&folder);
    let _held = lock.lock().unwrap_or_else(PoisonError::into_inner);
    let old = match last_backup(&folder)? {
        LastBackup::None => None,
        LastBackup::Good(old) => Some(old),
        LastBackup::Damaged(why) => return Ok(BackupOutcome::Refused(why)),
    };
    clear_stale(&parent, name)?;
    let run = run_tag();
    let staging = parent.join(format!(".{name}.new-{run}"));
    let work = work_dir(owner, name);
    let built = build(repo, remote, taken_at, &work, &staging);
    let _ = std::fs::remove_dir_all(&work);
    let manifest = match built {
        Ok(manifest) => manifest,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let refusal = old.and_then(|old| {
        refuse_reason(&old, &manifest).or_else(|| {
            // A wiki that could not be read looks the same as a wiki that is gone.
            (old.wiki && !manifest.wiki).then(|| {
                "the new backup has no wiki, the last had one; if the wiki was removed on \
                 purpose, move the last backup's folder away to let a new backup be written"
                    .to_string()
            })
        })
    });
    if let Some(reason) = refusal {
        remove_dir(&staging)?;
        return Ok(BackupOutcome::Refused(reason));
    }
    swap_in(
        &staging,
        &folder,
        &parent.join(format!(".{name}.old-{run}")),
    )?;
    Ok(BackupOutcome::Written(manifest))
}

/// A staging folder left by a run that died is this old before a later backup removes it.
const STALE_STAGING: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Removes what dead runs left in `parent` for the repo `name`: the fixed `.<name>.new` of
/// earlier versions, and any `.<name>.new-<run>` older than a day. A younger one may belong to
/// a backup running in another process, so it stays.
fn clear_stale(parent: &Path, name: &str) -> Result<(), CoreError> {
    remove_dir(&parent.join(format!(".{name}.new")))?;
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Ok(());
    };
    let prefix = format!(".{name}.new-");
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age > STALE_STAGING);
        if old && entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    Ok(())
}

/// The new backup, written into `staging`; the mirrors go into `work`.
fn build(
    repo: &str,
    remote: &Remote<'_>,
    taken_at: &str,
    work: &Path,
    staging: &Path,
) -> Result<Manifest, CoreError> {
    remove_dir(work)?;
    std::fs::create_dir_all(work).map_err(|e| io_error(work, e))?;
    let mirror = work.join("repo.git");
    let url = format!("{}/{repo}.git", remote.git_base);
    clone_mirror(remote, &url, &mirror)?;
    let refs = bundle(&mirror, staging)?;

    let answers = RecordedAnswers {
        repo: repo.to_string(),
        issues: rest_all(remote.api, &format!("/repos/{repo}/issues?state=all"))?
            .into_iter()
            .filter(|item| item.get("pull_request").is_none())
            .collect(),
        pulls: rest_all(remote.api, &format!("/repos/{repo}/pulls?state=all"))?,
        comments: rest_all(remote.api, &format!("/repos/{repo}/issues/comments?"))?,
        labels: rest_all(remote.api, &format!("/repos/{repo}/labels?"))?,
        milestones: rest_all(remote.api, &format!("/repos/{repo}/milestones?state=all"))?,
    };
    let mut manifest = write_json(&answers, staging, taken_at)?;
    manifest.refs = refs;
    manifest.bundle_hash = bundle_hash(staging)?;

    let releases = rest_all(remote.api, &format!("/repos/{repo}/releases?"))?;
    write_releases(&releases, remote.files, staging, &mut manifest)?;

    // A repo with no wiki (or no wiki pages yet) has nothing to clone; any other failed clone
    // is an error, so a wiki that could not be read is never written down as "no wiki".
    let wiki = work.join("wiki.git");
    let wiki_url = format!("{}/{repo}.wiki.git", remote.git_base);
    let wiki_mirror = match clone_mirror(remote, &wiki_url, &wiki) {
        Ok(()) => Some(wiki),
        Err(e) if no_such_repo(&e) => None,
        Err(e) => return Err(e),
    };
    copy_wiki(wiki_mirror.as_deref(), staging, &mut manifest)?;

    let path = staging.join("manifest.json");
    std::fs::write(&path, pretty(&manifest)?).map_err(|e| io_error(&path, e))?;
    Ok(manifest)
}

/// `git hash-object` of `<dir>/repo.bundle`.
fn bundle_hash(dir: &Path) -> Result<String, CoreError> {
    let dir = std::path::absolute(dir).map_err(|e| io_error(dir, e))?;
    Ok(git(&dir, &["hash-object", "--no-filters", "repo.bundle"])?
        .trim()
        .to_string())
}

/// Whether a failed clone says the repo is not there (GitHub: "Repository not found"; a local
/// path: "does not appear to be a git repository", "does not exist"), not that the read failed.
fn no_such_repo(e: &CoreError) -> bool {
    let CoreError::Io(why) = e else {
        return false;
    };
    let why = why.to_ascii_lowercase();
    [
        "not found",
        "does not appear to be a git repository",
        "does not exist",
    ]
    .iter()
    .any(|said| why.contains(said))
}

/// Every page of a REST list, 100 at a time, until a short page. `path` ends in `?` or carries
/// a query already.
fn rest_all(api: &dyn GitHubTransport, path: &str) -> Result<Vec<Value>, CoreError> {
    let sep = if path.ends_with('?') { "" } else { "&" };
    let mut items = Vec::new();
    for page in 1..=1000 {
        let url = format!("{path}{sep}per_page=100&page={page}");
        let answer = api.rest_get(&url, None)?;
        if answer.status != 200 {
            return Err(CoreError::GitHub(format!(
                "GET {url}: status {}",
                answer.status
            )));
        }
        let Value::Array(batch) = answer.body else {
            return Err(CoreError::GitHub(format!("GET {url}: not a JSON array")));
        };
        let full = batch.len() >= 100;
        items.extend(batch);
        if !full {
            return Ok(items);
        }
    }
    Err(CoreError::GitHub(format!(
        "GET {path}: more than 1000 pages"
    )))
}

/// `git clone --mirror <url> <into>`, with the remote's `-c` settings and no prompts.
fn clone_mirror(remote: &Remote<'_>, url: &str, into: &Path) -> Result<(), CoreError> {
    let mut cmd = Command::new("git");
    for config in &remote.git_config {
        cmd.arg("-c").arg(config);
    }
    let out = cmd
        .args(["clone", "-q", "--mirror", url])
        .arg(into)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| CoreError::Io(format!("git clone --mirror {url}: {e}")))?;
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(into);
        return Err(CoreError::Io(format!(
            "git clone --mirror {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// The manifest of the backup in `folder`, or `None` when it has none.
fn read_manifest(folder: &Path) -> Result<Option<Manifest>, CoreError> {
    let path = folder.join("manifest.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(&path, e)),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))
}

/// The backup a new one would replace.
enum LastBackup {
    /// No folder, or an empty one.
    None,
    Good(Manifest),
    /// The folder holds files but no manifest that can be read; the text says which and what to
    /// do, for the Waiting on you item.
    Damaged(String),
}

/// BAK-2: the last backup in `folder`. A folder with a damaged manifest, or with files and no
/// manifest, cannot be compared with a new backup, so it is never replaced silently. A backup in
/// another process may be swapping its folder in between this one's two reads, which looks
/// damaged for a moment, so a damaged folder is looked at twice more before it is called that.
fn last_backup(folder: &Path) -> Result<LastBackup, CoreError> {
    for _ in 0..2 {
        match last_backup_now(folder)? {
            LastBackup::Damaged(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
            other => return Ok(other),
        }
    }
    last_backup_now(folder)
}

/// `last_backup`, one look.
fn last_backup_now(folder: &Path) -> Result<LastBackup, CoreError> {
    const TO_DO: &str = "check the folder, then move it away or remove it to let a new backup \
                         be written";
    let path = folder.join("manifest.json");
    match std::fs::read(&path) {
        Ok(bytes) => Ok(match serde_json::from_slice(&bytes) {
            Ok(manifest) => LastBackup::Good(manifest),
            Err(e) => LastBackup::Damaged(format!(
                "the last backup's manifest.json is damaged ({e}), so the new backup cannot be \
                 compared with it; {TO_DO}"
            )),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut holds: Vec<String> = match std::fs::read_dir(folder) {
                Ok(entries) => entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(e) => return Err(io_error(folder, e)),
            };
            if holds.is_empty() {
                return Ok(LastBackup::None);
            }
            holds.sort();
            Ok(LastBackup::Damaged(format!(
                "the last backup has no manifest.json but still holds {}, so the new backup \
                 cannot be compared with it; {TO_DO}",
                holds.join(", ")
            )))
        }
        Err(e) => Err(io_error(&path, e)),
    }
}

/// Moves `staging` to `folder`, the old `folder` aside to `aside` first and removed after. A
/// failed move puts the old folder back. When a backup in another process took the place in
/// between, its folder stays and this run's two folders are removed.
fn swap_in(staging: &Path, folder: &Path, aside: &Path) -> Result<(), CoreError> {
    let had_old = folder.exists();
    if had_old {
        if let Err(e) = std::fs::rename(folder, aside) {
            let _ = std::fs::remove_dir_all(staging);
            return Err(io_error(folder, e));
        }
    }
    if let Err(e) = std::fs::rename(staging, folder) {
        let put_back = had_old && std::fs::rename(aside, folder).is_ok();
        let _ = std::fs::remove_dir_all(staging);
        if had_old && !put_back && folder.join("manifest.json").exists() {
            let _ = std::fs::remove_dir_all(aside);
        }
        return Err(io_error(staging, e));
    }
    if had_old {
        remove_dir(aside)?;
    }
    Ok(())
}

fn remove_dir(dir: &Path) -> Result<(), CoreError> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_error(dir, e)),
        _ => Ok(()),
    }
}

/// The backups folder, or `MissingPath("nextcloud_backups")` when unset or empty.
fn backups_dir(settings: &Settings) -> Result<&Path, CoreError> {
    match settings.nextcloud_backups.as_deref() {
        Some(p) if !p.as_os_str().is_empty() => Ok(p),
        _ => Err(CoreError::MissingPath("nextcloud_backups")),
    }
}

/// `owner/name` split in two; anything else (an empty part, a third part, `.` or `..`) is an
/// error, so the folder never lands outside `github/`.
fn split_repo(repo: &str) -> Result<(&str, &str), CoreError> {
    let ok = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    match repo.split_once('/') {
        Some((owner, name)) if ok(owner) && ok(name) => Ok((owner, name)),
        _ => Err(CoreError::Parse(format!("not an owner/name repo: {repo}"))),
    }
}

/// A tag no other run has: the process, the clock and a counter. The counter keeps two runs in
/// one process apart when the clock does not tick between them (macOS reads it in whole
/// microseconds).
fn run_tag() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{}-{nanos}-{seq}", std::process::id())
}

/// A fresh temp folder for this backup's mirrors.
fn work_dir(owner: &str, name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("switchboard-backup-{owner}-{name}-{}", run_tag()))
}

/// `e` with `repo` in front of its text.
fn about(repo: &str, e: CoreError) -> CoreError {
    match e {
        CoreError::GitHub(why) => CoreError::GitHub(format!("{repo}: {why}")),
        CoreError::Io(why) => CoreError::Io(format!("{repo}: {why}")),
        CoreError::Parse(why) => CoreError::Parse(format!("{repo}: {why}")),
        other => other,
    }
}

fn now_rfc3339() -> String {
    use crate::clock::Clock;
    rfc3339(crate::clock::SystemClock.now())
}

/// `secs` since the epoch as `YYYY-MM-DDTHH:MM:SSZ`.
fn rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// BAK-3: what `restore_issues` did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssuesReport {
    /// Issues sent through the create queue.
    pub issues: u64,
    /// Issues the repo already had (the same title), not created again.
    pub issues_there: u64,
    /// Comments sent through the create queue, each on the number its issue has now.
    pub comments: u64,
    /// Comments the repo already had on their issue, not created again.
    pub comments_there: u64,
    /// Comments held back because their issue is not in the repo yet (the queue stopped on a
    /// rate limit, or kept one of two issues with the same title). Run `restore_issues` again
    /// once the issues are there.
    pub comments_waiting: u64,
    /// Issues closed because they were closed in the backup (#365).
    pub closed: u64,
    /// Issue bodies whose `#N` were rewritten to the numbers the issues have now (#365).
    pub renumbered: u64,
    /// Milestones closed because they were closed in the backup (#365).
    pub milestones_closed: u64,
    /// Closes and body changes held back: their issue (or, for a milestone, some issue) is not
    /// in the repo yet, or GitHub answered a rate limit. Run `restore_issues` again.
    pub state_waiting: u64,
    /// What the backup holds that was not put back, one plain line each.
    pub left_out: Vec<String>,
}

impl IssuesReport {
    /// True when something was held back for a second call.
    pub fn held(&self) -> bool {
        self.comments_waiting > 0 || self.state_waiting > 0
    }
}

/// BAK-3: what `restore_history` put back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreReport {
    /// Refs pushed from the bundle.
    pub refs: u64,
    /// Labels sent through the create queue.
    pub labels: u64,
    /// Milestones sent through the create queue.
    pub milestones: u64,
    /// The issues and comments (`restore_over_live` only; `restore_history` leaves them to
    /// `restore_issues`).
    pub issues: IssuesReport,
    /// What the backup holds that was not put back, one plain line each.
    pub left_out: Vec<String>,
    /// What a restore over a live repo made, as far as the create queue has said
    /// (`restore_over_live` and its held pass; empty for a new repo). The held pass changes
    /// only these.
    pub made: Made,
}

/// BAK-3: restore the history, labels and milestones of the backup in `backup_dir` to the new
/// private repo `new_repo` (`owner/name`) on GitHub, as `restore_history_from` does. Git pushes
/// use `gh` as the credential helper and the API uses `gh auth token`.
pub fn restore_history(
    fleet: &FleetBridge,
    backup_dir: &Path,
    new_repo: &str,
) -> Result<RestoreReport, CoreError> {
    with_github(|remote| restore_history_from(fleet, backup_dir, new_repo, remote))
}

/// `restore_history` against `remote`.
///
/// Reads and checks the whole backup first (`read_backup`, then `repo.bundle` cloned into a temp
/// mirror, which must hold the manifest's count of refs). Then asks the API for
/// `/repos/<new_repo>`: a repo that is there is `Refused` with nothing written. Otherwise the
/// repo is created private through the create queue (an `exists` answer is `Refused` too), the
/// API must then show it private, every ref in the bundle is pushed to
/// `<git_base>/<new_repo>.git`, and the labels (with colour and description) and milestones
/// (with description and due date) go through the queue in one batch.
/// `refs/pull/*` are not pushed: GitHub keeps them read-only. The report's `left_out` names what
/// the backup holds that is not put back; a milestone that was closed in the backup is closed by
/// `restore_issues`, after the issues that name it. Never restores over a live repo; that needs
/// `rules::guard` (#101).
pub fn restore_history_from(
    fleet: &FleetBridge,
    backup_dir: &Path,
    new_repo: &str,
    remote: &Remote<'_>,
) -> Result<RestoreReport, CoreError> {
    let (owner, name) = split_repo(new_repo)?;
    let backup = read_backup(backup_dir)?;
    let creates: Vec<Create> = label_creates(&backup, new_repo)
        .into_iter()
        .chain(milestone_creates(&backup, new_repo))
        .collect();
    // Cloning the bundle checks it before anything is written.
    let work = work_dir(owner, name);
    let restored = clone_mirror(remote, &backup.bundle.to_string_lossy(), &work)
        .and_then(|()| check_refs(&work, &backup))
        .and_then(|()| restore_into(fleet, &work, new_repo, remote, &creates));
    let _ = std::fs::remove_dir_all(&work);
    let refs = restored?;
    let mut left_out = left_out(&backup);
    let closed = backup
        .milestones
        .iter()
        .filter(|m| has_state(m, "closed"))
        .count();
    if closed > 0 {
        left_out.push(format!(
            "{closed} closed milestones are open until the issues are restored: \
             restore_issues closes them"
        ));
    }
    Ok(RestoreReport {
        refs,
        labels: backup.labels.len() as u64,
        milestones: backup.milestones.len() as u64,
        issues: IssuesReport::default(),
        left_out,
        made: Made::default(),
    })
}

/// The writes of `restore_history_from`, the bundle already cloned into `work`; returns how many
/// refs it pushed.
fn restore_into(
    fleet: &FleetBridge,
    work: &Path,
    new_repo: &str,
    remote: &Remote<'_>,
    creates: &[Create],
) -> Result<u64, CoreError> {
    let taken = format!("{new_repo} already exists; restore goes to a new repo only");
    let path = format!("/repos/{new_repo}");
    match remote.api.rest_get(&path, None)?.status {
        404 => {}
        200 => return Err(CoreError::Refused(taken)),
        other => return Err(CoreError::GitHub(format!("GET {path}: status {other}"))),
    }
    let before = queue_counts(fleet)?;
    let made = queue::send(
        fleet,
        &[Create::Repo {
            repo: new_repo.to_string(),
            private: true,
        }],
    )?;
    if made.exists > before.exists {
        return Err(CoreError::Refused(taken));
    }
    if made.failed > before.failed || made.pending > before.pending {
        return Err(CoreError::GitHub(format!(
            "{new_repo}: the create queue did not create the repo (pending {}, failed {})",
            made.pending, made.failed
        )));
    }
    let answer = remote.api.rest_get(&path, None)?;
    if answer.status != 200 || answer.body["private"] != Value::Bool(true) {
        return Err(CoreError::GitHub(format!(
            "{new_repo}: not there as a private repo after the create (status {})",
            answer.status
        )));
    }

    let refs = push_refs(remote, work, new_repo).map_err(|e| about(new_repo, e))?;
    queue_labels(fleet, new_repo, creates, &made)?;
    Ok(refs)
}

/// A backup folder read and checked against its manifest, before a restore writes anything.
struct Backup {
    manifest: Manifest,
    /// `labels.json`: each item has a `name`.
    labels: Vec<Value>,
    /// `milestones.json`: each item has a `title`.
    milestones: Vec<Value>,
    /// `repo.bundle`, as an absolute path.
    bundle: PathBuf,
}

/// BAK-3: the backup in `dir`, checked against its manifest: the manifest names an `owner/name`
/// repo, the one this folder is the backup of when the folder is `.../github/<owner>/<name>`,
/// and the one every API address in the JSON files is for (so the check holds for a copy kept
/// anywhere), or the manifest's `resolved_repo` when the backup followed a rename; `labels.json` and `milestones.json` hold the manifest's counts;
/// `repo.bundle` has the manifest's hash (a manifest written before it held one has none to
/// check; `check_refs` still counts the refs).
fn read_backup(dir: &Path) -> Result<Backup, CoreError> {
    let manifest = manifest_in(dir)?;
    let dir = &std::path::absolute(dir).map_err(|e| io_error(dir, e))?;
    let wrong = |why: String| CoreError::Parse(format!("{}: {why}", dir.display()));
    if split_repo(&manifest.repo).is_err() {
        return Err(wrong(format!(
            "the manifest's repo is not an owner/name repo: {}",
            manifest.repo
        )));
    }
    let part = |path: Option<&Path>| {
        path.and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
    };
    let owner_dir = dir.parent();
    let github_dir = owner_dir.and_then(Path::parent);
    if let (Some(name), Some(owner), Some("github")) = (
        part(Some(dir)),
        part(owner_dir),
        part(github_dir).as_deref(),
    ) {
        let of = format!("{owner}/{name}");
        if !of.eq_ignore_ascii_case(&manifest.repo) {
            return Err(wrong(format!(
                "the manifest is for {}, the folder is the backup of {of}",
                manifest.repo
            )));
        }
    }
    let labels = keyed_items(dir, "labels.json", "name", manifest.labels)?;
    let milestones = keyed_items(dir, "milestones.json", "title", manifest.milestones)?;
    // Wherever the folder sits, what it holds says which repo it is the backup of.
    let mut items: Vec<Value> = labels.iter().chain(&milestones).cloned().collect();
    for file in ["issues.json", "pulls.json", "comments.json"] {
        let more = std::fs::read_to_string(dir.join(file))
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok());
        items.extend(more.unwrap_or_default());
    }
    // A repo renamed or moved before the backup: the manifest holds the name GitHub answered for.
    let resolved = manifest.resolved_repo.as_deref();
    let other = items.iter().filter_map(repo_of).find(|of| {
        !of.eq_ignore_ascii_case(&manifest.repo)
            && !resolved.is_some_and(|name| of.eq_ignore_ascii_case(name))
    });
    if let Some(other) = other {
        return Err(wrong(match resolved {
            Some(name) => format!(
                "the manifest is for {} (GitHub answered for {name}), the backup holds items \
                 of {other}",
                manifest.repo
            ),
            None => format!(
                "the manifest is for {}, the backup holds items of {other}; if the repo was \
                 renamed or moved to {other} before this backup was taken, add \
                 \"resolved_repo\": \"{other}\" to its manifest.json and restore again",
                manifest.repo
            ),
        }));
    }
    if !manifest.bundle_hash.is_empty() {
        let hash = bundle_hash(dir)?;
        if hash != manifest.bundle_hash {
            return Err(wrong(format!(
                "repo.bundle is not the one the manifest was written for (hash {hash}, the \
                 manifest says {})",
                manifest.bundle_hash
            )));
        }
    }
    Ok(Backup {
        manifest,
        labels,
        milestones,
        bundle: dir.join("repo.bundle"),
    })
}

/// The `owner/name` in a backed-up item's API address (`repository_url`, `issue_url` or `url`),
/// when it has one.
fn repo_of(item: &Value) -> Option<String> {
    ["repository_url", "issue_url", "url"]
        .iter()
        .find_map(|key| {
            let rest = item[*key]
                .as_str()?
                .strip_prefix("https://api.github.com/repos/")?;
            let mut parts = rest.split('/');
            Some(format!("{}/{}", parts.next()?, parts.next()?))
        })
}

/// BAK-3: the bundle, cloned into `work`, holds as many refs as the manifest says.
fn check_refs(work: &Path, backup: &Backup) -> Result<(), CoreError> {
    let refs = git(work, &["for-each-ref", "--format=%(refname)"])?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count() as u64;
    if refs != backup.manifest.refs {
        return Err(CoreError::Parse(format!(
            "{}: {refs} refs, the manifest says {}",
            backup.bundle.display(),
            backup.manifest.refs
        )));
    }
    Ok(())
}

/// What a restore of `backup` does not put back, one plain line each.
fn left_out(backup: &Backup) -> Vec<String> {
    let manifest = &backup.manifest;
    let mut lines = Vec::new();
    if manifest.wiki {
        lines.push("the wiki (wiki.git) is not restored".to_string());
    }
    if manifest.pulls > 0 {
        lines.push(format!(
            "{} PRs (pulls.json) are not restored",
            manifest.pulls
        ));
    }
    if manifest.releases > 0 {
        lines.push(format!(
            "{} releases (releases.json, releases/) are not restored; their tags are",
            manifest.releases
        ));
    }
    lines
}

/// The items of the JSON array `<dir>/<file>`; an error when there are not `expected` of them, as
/// the manifest says.
fn items_in(dir: &Path, file: &str, expected: u64) -> Result<Vec<Value>, CoreError> {
    let path = dir.join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| io_error(&path, e))?;
    let items: Vec<Value> = serde_json::from_str(&text)
        .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))?;
    if items.len() as u64 != expected {
        return Err(CoreError::Parse(format!(
            "{}: {} items, the manifest says {expected}",
            path.display(),
            items.len()
        )));
    }
    Ok(items)
}

/// `items_in`, each item with a `key` string.
fn keyed_items(dir: &Path, file: &str, key: &str, expected: u64) -> Result<Vec<Value>, CoreError> {
    let items = items_in(dir, file, expected)?;
    if items.iter().any(|item| !item[key].is_string()) {
        return Err(CoreError::Parse(format!(
            "{}: an item with no {key}",
            dir.join(file).display()
        )));
    }
    Ok(items)
}

/// The backup's manifest; an error when `dir` has none.
fn manifest_in(dir: &Path) -> Result<Manifest, CoreError> {
    read_manifest(dir)?
        .ok_or_else(|| CoreError::Io(format!("{}: no manifest.json, not a backup", dir.display())))
}

/// The string at `key`, when it is not blank.
fn filled(item: &Value, key: &str) -> Option<String> {
    item[key]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(String::from)
}

/// The backup's labels as creates on `repo`, each with its colour and description.
fn label_creates(backup: &Backup, repo: &str) -> Vec<Create> {
    backup
        .labels
        .iter()
        .map(|label| Create::Label {
            repo: repo.to_string(),
            name: label["name"].as_str().unwrap_or_default().to_string(),
            color: filled(label, "color")
                .filter(|c| c.len() == 6 && c.chars().all(|ch| ch.is_ascii_hexdigit())),
            // The queue takes a label's description as one line.
            description: filled(label, "description")
                .map(|d| d.split_whitespace().collect::<Vec<_>>().join(" ")),
        })
        .collect()
}

/// The backup's milestones as creates on `repo`, each with its description and due date.
fn milestone_creates(backup: &Backup, repo: &str) -> Vec<Create> {
    backup
        .milestones
        .iter()
        .map(|milestone| Create::Milestone {
            repo: repo.to_string(),
            title: milestone["title"].as_str().unwrap_or_default().to_string(),
            description: filled(milestone, "description"),
            // `due_on` is a time (`2026-12-01T08:00:00Z`); the queue takes the date.
            due: filled(milestone, "due_on")
                .and_then(|at| at.get(..10).map(String::from))
                .filter(|date| {
                    date.bytes().enumerate().all(|(i, b)| match i {
                        4 | 7 => b == b'-',
                        _ => b.is_ascii_digit(),
                    })
                }),
        })
        .collect()
}

/// Label and milestone creates in one batch through the queue; `base` is the queue's counts
/// before it. A label or milestone the queue fails is an error.
fn queue_labels(
    fleet: &FleetBridge,
    repo: &str,
    creates: &[Create],
    base: &QueueStatus,
) -> Result<Vec<Queued>, CoreError> {
    let (sent, keyed) = queue::send_keyed(fleet, creates)?;
    if sent.failed > base.failed {
        return Err(CoreError::GitHub(format!(
            "{repo}: {} label or milestone creates failed in the queue",
            sent.failed - base.failed
        )));
    }
    Ok(keyed)
}

/// The queue's counts now (`fleet ghq status`).
fn queue_counts(fleet: &FleetBridge) -> Result<QueueStatus, CoreError> {
    let answer = fleet
        .run_json(&["ghq", "status"])
        .map_err(CoreError::Fleet)?;
    let count = |name: &str| answer["counts"][name].as_u64().unwrap_or(0);
    Ok(QueueStatus {
        pending: count("pending"),
        done: count("done"),
        exists: count("exists"),
        failed: count("failed"),
    })
}

/// Pushes each ref of the mirror `work` but `refs/pull/*` to `<git_base>/<repo>.git`; returns
/// how many it pushed.
fn push_refs(remote: &Remote<'_>, work: &Path, repo: &str) -> Result<u64, CoreError> {
    let url = format!("{}/{repo}.git", remote.git_base);
    let listed = git(work, &["for-each-ref", "--format=%(refname)"])?;
    let refspecs: Vec<String> = listed
        .lines()
        .map(str::trim)
        .filter(|r| !r.is_empty() && !r.starts_with("refs/pull/"))
        .map(|r| format!("+{r}:{r}"))
        .collect();
    if refspecs.is_empty() {
        return Err(CoreError::Io(
            "the bundle holds no refs to push".to_string(),
        ));
    }
    let mut cmd = Command::new("git");
    for config in &remote.git_config {
        cmd.arg("-c").arg(config);
    }
    let out = cmd
        .arg("-C")
        .arg(work)
        .args(["push", "-q", "--atomic", &url])
        .args(&refspecs)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| CoreError::Io(format!("git push {url}: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::Io(format!(
            "git push {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(refspecs.len() as u64)
}

/// The issue number at the end of a comment's `issue_url`.
fn issue_of(comment: &Value) -> Option<u64> {
    comment["issue_url"]
        .as_str()
        .and_then(|url| url.rsplit('/').next())
        .and_then(|n| n.parse().ok())
}

/// A comment body as it is compared: the queue's own `<!-- ghq:... -->` marker line and the
/// white space at the end are not part of it.
fn plain(body: &str) -> String {
    let text = body.trim_end();
    let text = match text.rfind("<!-- ghq:") {
        Some(at) if text.ends_with("-->") => text[..at].trim_end(),
        _ => text,
    };
    text.to_string()
}

/// One of the repo's issues as a restore reads it.
struct LiveIssue {
    number: u64,
    title: String,
    closed: bool,
    body: String,
}

/// The repo's issues now (PRs aside), lowest number first.
fn live_issues(api: &dyn GitHubTransport, repo: &str) -> Result<Vec<LiveIssue>, CoreError> {
    let mut issues: Vec<LiveIssue> = rest_all(api, &format!("/repos/{repo}/issues?state=all"))?
        .iter()
        .filter(|item| item.get("pull_request").is_none())
        .filter_map(|item| {
            Some(LiveIssue {
                number: item["number"].as_u64()?,
                title: item["title"].as_str()?.to_string(),
                closed: has_state(item, "closed"),
                body: item["body"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();
    issues.sort_by(|a, b| (a.number, &a.title).cmp(&(b.number, &b.title)));
    Ok(issues)
}

/// `(number, title)` of each live issue, as `match_issues` reads them.
fn titles_of(live: &[LiveIssue]) -> Vec<(u64, String)> {
    live.iter()
        .map(|issue| (issue.number, issue.title.clone()))
        .collect()
}

/// Which issue of the repo each backed-up issue is: the backup's number to the repo's number.
/// An issue is the one with its title; of several with one title, the one with its own number
/// first, then the lowest number not taken. Both lists are lowest number first.
fn match_issues(backup: &[(u64, String)], live: &[(u64, String)]) -> HashMap<u64, u64> {
    let mut matched = HashMap::new();
    let mut taken = HashSet::new();
    for (number, title) in backup {
        if live.contains(&(*number, title.clone())) {
            matched.insert(*number, *number);
            taken.insert(*number);
        }
    }
    for (number, title) in backup {
        if matched.contains_key(number) {
            continue;
        }
        let found = live
            .iter()
            .find(|(n, t)| t == title && !taken.contains(n))
            .map(|(n, _)| *n);
        if let Some(now) = found {
            matched.insert(*number, now);
            taken.insert(now);
        }
    }
    matched
}

/// Sends `creates` through the queue; a create the queue fails is an error. Answers what the
/// queue said of each create (`queue::send_keyed`).
fn send_checked(
    fleet: &FleetBridge,
    repo: &str,
    creates: &[Create],
) -> Result<Vec<Queued>, CoreError> {
    if creates.is_empty() {
        return Ok(Vec::new());
    }
    let before = queue_counts(fleet)?;
    let (sent, keyed) = queue::send_keyed(fleet, creates)?;
    if sent.failed > before.failed {
        return Err(CoreError::GitHub(format!(
            "{repo}: {} issue or comment creates failed in the queue",
            sent.failed - before.failed
        )));
    }
    Ok(keyed)
}

/// One create a restore over a live repo queued and the queue added as new: the queue's key
/// for it, and the number GitHub gave it, once the queue's own record says it made it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Created {
    pub key: String,
    pub number: Option<u64>,
}

/// What a restore over a live repo made (#365): the issues, by the backup's number, and the
/// milestones, by title in lower case, that it queued and the queue added as new. Only these
/// are closed or given new `#N`; everything else in the repo is left as it is. The confirm step
/// keeps the record beside the held Waiting item, so the second confirm changes the same ones.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Made {
    pub issues: BTreeMap<u64, Created>,
    pub milestones: BTreeMap<String, Created>,
}

/// Which part of a restore of issues runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pass {
    /// Everything: the issues the repo lacks are queued, then the comments, then the state.
    Whole,
    /// Only what an earlier pass held back until its issues were there: the comments, the
    /// closes and the renumbered bodies. No issue is queued. The record is what the whole pass
    /// made: only those issues and milestones are closed or renumbered.
    Held(Made),
}

/// The number after `part` in a created item's URL (`/issues/` or `/milestone/`).
fn number_in(url: &str, part: &str) -> Option<u64> {
    let at = url.find(part)? + part.len();
    let digits: String = url[at..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Gives each create in `made` that has no number yet the one the queue's own record says it
/// made. One still `pending` keeps none. One the queue found there already (`exists`), made
/// without saying which, failed, or no longer holds is not attributed to the restore: it leaves
/// the record, and is named among what the restore did not make.
fn resolve(fleet: &FleetBridge, made: &mut Made) -> Result<(), CoreError> {
    let keys: Vec<String> = made
        .issues
        .values()
        .chain(made.milestones.values())
        .filter(|c| c.number.is_none())
        .map(|c| c.key.clone())
        .collect();
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    let record = queue::records(fleet, &keys)?;
    let settle = |created: &mut Created, part: &str| -> bool {
        if created.number.is_some() {
            return true;
        }
        match record.get(&created.key) {
            Some(r) if r.status == "pending" => true,
            Some(r) if r.status == "done" => {
                created.number = r.result.as_deref().and_then(|url| number_in(url, part));
                created.number.is_some()
            }
            _ => false,
        }
    };
    made.issues.retain(|_, c| settle(c, "/issues/"));
    made.milestones.retain(|_, c| settle(c, "/milestone/"));
    Ok(())
}

/// PATCHes `path` with `body`. `Ok(true)` when GitHub answered 200; `Ok(false)` when it
/// answered 403 or 429 (a rate limit: the caller stops and the rest waits); any other status is
/// an error.
fn patched(api: &dyn GitHubTransport, path: &str, body: &Value) -> Result<bool, CoreError> {
    match api.rest_patch(path, body)?.status {
        200 => Ok(true),
        403 | 429 => Ok(false),
        other => Err(CoreError::GitHub(format!("PATCH {path}: status {other}"))),
    }
}

/// `n` items, then the names: `2 ...: #7, #9`.
fn named(what: &str, names: &[String]) -> Option<String> {
    (!names.is_empty()).then(|| format!("{} {what}: {}", names.len(), names.join(", ")))
}

/// BAK-3: puts the issues and comments of the backup in `backup_dir` on `repo` (`owner/name`),
/// with the state they had (#365). The counts must be the manifest's.
///
/// Reads the repo's issues first: an issue that is there already (the same title) is not created
/// again, and the rest are queued lowest number first. A new repo numbers its issues from 1, so
/// the repo is read again after the creates and each comment is queued on the number its issue
/// has now, never the backup's, with each plain `#N` in it rewritten to the number that issue
/// has now (`renumber`). A comment the repo already has on that issue is not created again; one
/// whose issue, or an issue it names, is not there yet is held back and counted
/// (`comments_waiting`), and a second call queues it. A rate-limit stop leaves creates queued;
/// only a failed create is an error.
///
/// Then the state, through the transport's PATCH (a close is not a create, GH-4): an issue that
/// was closed in the backup and is open in the repo is closed (over a live repo, only one the
/// restore made: `restore_over_live_from`); an issue body that still reads as
/// the backup's gets its `#N` rewritten; and, once every issue is there, a milestone that was
/// closed in the backup and is open in the repo is closed. What has to wait for an issue that is
/// not there yet, or for a rate limit, is counted in `state_waiting`, and a second call does it.
/// `left_out` says what is not put back: the comments on PRs.
pub fn restore_issues(
    fleet: &FleetBridge,
    backup_dir: &Path,
    repo: &str,
    remote: &Remote<'_>,
) -> Result<IssuesReport, CoreError> {
    restore_issues_made(fleet, backup_dir, repo, remote, true, None)
}

/// `restore_issues`. `whole` false is the held pass: it queues no issue, only what was held
/// back. With `made` (a restore over a live repo) only the issues and milestones in that record
/// are closed or renumbered: the issues this pass queues and the queue adds as new go into it,
/// each with the number the queue's own record says it made (`resolve`), never one found by
/// title. Without it (a new repo) everything in the repo is the restore's.
fn restore_issues_made(
    fleet: &FleetBridge,
    backup_dir: &Path,
    repo: &str,
    remote: &Remote<'_>,
    whole: bool,
    mut made: Option<&mut Made>,
) -> Result<IssuesReport, CoreError> {
    split_repo(repo)?;
    let manifest = manifest_in(backup_dir)?;
    let issues = items_in(backup_dir, "issues.json", manifest.issues)?;
    let comments = items_in(backup_dir, "comments.json", manifest.comments)?;
    let milestones = keyed_items(backup_dir, "milestones.json", "title", manifest.milestones)?;
    let text = |item: &Value, key: &str| item[key].as_str().unwrap_or("").to_string();
    let mut numbered: Vec<(u64, &Value)> = Vec::new();
    for issue in &issues {
        let number = issue["number"]
            .as_u64()
            .ok_or_else(|| CoreError::Parse(format!("{repo}: an issue with no number")))?;
        numbered.push((number, issue));
    }
    numbered.sort_by_key(|(number, _)| *number);
    let titles: Vec<(u64, String)> = numbered
        .iter()
        .map(|(number, issue)| (*number, text(issue, "title")))
        .collect();
    let mut old_numbers = Vec::new();
    for comment in &comments {
        old_numbers.push(
            issue_of(comment).ok_or_else(|| {
                CoreError::Parse(format!("{repo}: a comment with no issue number"))
            })?,
        );
    }

    let mut live = live_issues(remote.api, repo)?;
    let mut report = IssuesReport::default();
    if whole {
        let there = match_issues(&titles, &titles_of(&live));
        let to_queue: Vec<&(u64, &Value)> = numbered
            .iter()
            .filter(|(number, _)| !there.contains_key(number))
            .collect();
        let issue_creates: Vec<Create> = to_queue
            .iter()
            .map(|(_, issue)| Create::Issue {
                repo: repo.to_string(),
                title: text(issue, "title"),
                body: text(issue, "body"),
                labels: issue["labels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|label| label["name"].as_str().map(String::from))
                    .collect(),
                milestone: issue["milestone"]["title"].as_str().map(String::from),
            })
            .collect();
        report.issues = issue_creates.len() as u64;
        report.issues_there = (numbered.len() - issue_creates.len()) as u64;
        if !issue_creates.is_empty() {
            let keyed = send_checked(fleet, repo, &issue_creates)?;
            // Only a create this batch added is the restore's: one the queue held already
            // (another request, or a second issue with the same title) is not.
            if let Some(made) = made.as_deref_mut() {
                for ((old, _), queued) in to_queue.iter().zip(&keyed) {
                    if queued.added {
                        made.issues.insert(
                            *old,
                            Created {
                                key: queued.key.clone(),
                                number: None,
                            },
                        );
                    }
                }
            }
            live = live_issues(remote.api, repo)?;
        }
    }
    if let Some(made) = made.as_deref_mut() {
        resolve(fleet, made)?;
    }
    // Which issue of the repo each backed-up issue is now: a recorded one is the number the
    // queue made (none while it waits); any other is matched by title among the rest.
    let now: HashMap<u64, u64> = match made.as_deref() {
        None => match_issues(&titles, &titles_of(&live)),
        Some(made) => {
            let recorded: HashSet<u64> = made.issues.values().filter_map(|c| c.number).collect();
            let others: Vec<(u64, String)> = titles
                .iter()
                .filter(|(old, _)| !made.issues.contains_key(old))
                .cloned()
                .collect();
            let rest: Vec<(u64, String)> = titles_of(&live)
                .into_iter()
                .filter(|(number, _)| !recorded.contains(number))
                .collect();
            let mut now = match_issues(&others, &rest);
            for (old, created) in &made.issues {
                if let Some(number) = created
                    .number
                    .filter(|n| live.iter().any(|l| l.number == *n))
                {
                    now.insert(*old, number);
                }
            }
            now
        }
    };
    if !whole {
        report.issues_there = now.len() as u64;
    }

    let backed_up: HashSet<u64> = titles.iter().map(|(number, _)| *number).collect();
    // What a `#N` in the backup's text names now.
    let number_now = |old: u64| match now.get(&old) {
        Some(&new) => Now::Is(new),
        None if backed_up.contains(&old) => Now::NotYet,
        None => Now::Unknown,
    };

    // What is on the repo's issues already: (issue number, body) and how many times.
    let mut there: HashMap<(u64, String), u64> = HashMap::new();
    if !comments.is_empty() {
        for comment in rest_all(remote.api, &format!("/repos/{repo}/issues/comments?"))? {
            if let Some(number) = issue_of(&comment) {
                *there
                    .entry((number, plain(&text(&comment, "body"))))
                    .or_default() += 1;
            }
        }
    }
    let mut comment_creates = Vec::new();
    let mut on_prs = 0;
    for (comment, old) in comments.iter().zip(old_numbers) {
        if !backed_up.contains(&old) {
            on_prs += 1;
            continue;
        }
        let Some(&number) = now.get(&old) else {
            report.comments_waiting += 1;
            continue;
        };
        let body = renumber(&text(comment, "body"), &number_now);
        if body.waiting {
            report.comments_waiting += 1;
            continue;
        }
        match there.get_mut(&(number, plain(&body.text))) {
            Some(left) if *left > 0 => {
                *left -= 1;
                report.comments_there += 1;
            }
            _ => comment_creates.push(Create::Comment {
                repo: repo.to_string(),
                number,
                body: body.text,
                marker: None,
            }),
        }
    }
    report.comments = comment_creates.len() as u64;
    send_checked(fleet, repo, &comment_creates)?;

    // The state (#365). After a rate-limit answer nothing more is sent: the rest waits. Only
    // what this restore made is changed: any other issue or milestone is left, and named.
    let ours = |old: &u64| made.as_deref().map_or(true, |m| m.issues.contains_key(old));
    let mut left_open = Vec::new();
    let mut not_made = Vec::new();
    let mut kept_numbers = Vec::new();
    let mut limited = false;
    for (old, issue) in &numbered {
        let closed = has_state(issue, "closed");
        let body = text(issue, "body");
        let renumbered = renumber(&body, &number_now);
        let found = now
            .get(old)
            .and_then(|number| live.iter().find(|l| l.number == *number));
        if !ours(old) {
            match found {
                Some(found) => {
                    if closed && !found.closed {
                        left_open.push(format!("#{}", found.number));
                    }
                    if renumbered.text != body && plain(&found.body) == plain(&body) {
                        kept_numbers.push(format!("#{} (not made by this restore)", found.number));
                    }
                }
                None => not_made.push(format!("backup #{old}")),
            }
            continue;
        }
        let Some(found) = found else {
            if closed || renumbered.text != body || renumbered.waiting {
                report.state_waiting += 1;
            }
            continue;
        };
        let mut change = serde_json::Map::new();
        // Only a body that still reads as the backup's is rewritten; what the queue put after
        // it (its marker) stays.
        let as_backed_up = plain(&found.body) == plain(&body);
        if renumbered.waiting {
            report.state_waiting += 1;
        } else if renumbered.text != body && as_backed_up {
            let kept = &found.body[plain(&found.body).len()..];
            let rewritten = format!("{}{kept}", plain(&renumbered.text));
            change.insert("body".to_string(), Value::String(rewritten));
        } else if renumbered.text != body && plain(&found.body) != plain(&renumbered.text) {
            // Neither the backup's text nor the rewritten one: changed by someone since.
            kept_numbers.push(format!("#{} (changed in the repo since)", found.number));
        }
        if closed && !found.closed {
            change.insert("state".to_string(), Value::String("closed".to_string()));
            if let Some(reason @ ("completed" | "not_planned")) = issue["state_reason"].as_str() {
                change.insert(
                    "state_reason".to_string(),
                    Value::String(reason.to_string()),
                );
            }
        }
        if change.is_empty() {
            continue;
        }
        let closes = change.contains_key("state");
        let rewrites = change.contains_key("body");
        let path = format!("/repos/{repo}/issues/{}", found.number);
        if limited || !patched(remote.api, &path, &Value::Object(change))? {
            limited = true;
            report.state_waiting += 1;
            continue;
        }
        report.closed += u64::from(closes);
        report.renumbered += u64::from(rewrites);
    }

    // A milestone is closed last, once every issue that may name it is there: over a live repo,
    // every issue the restore made; over a new one, every issue of the backup.
    let closed_milestones: Vec<String> = milestones
        .iter()
        .filter(|m| has_state(m, "closed"))
        .map(|m| text(m, "title").to_lowercase())
        .collect();
    let mut milestones_left_open = Vec::new();
    if !closed_milestones.is_empty() {
        let all_there = match made.as_deref() {
            None => numbered.iter().all(|(old, _)| now.contains_key(old)),
            Some(made) => made.issues.keys().all(|old| now.contains_key(old)),
        };
        let live_milestones = rest_all(remote.api, &format!("/repos/{repo}/milestones?state=all"))?;
        for title in &closed_milestones {
            let found = match made.as_deref() {
                None => live_milestones
                    .iter()
                    .find(|m| text(m, "title").to_lowercase() == *title),
                Some(made) => match made.milestones.get(title) {
                    // The one the queue made, by its number.
                    Some(created) => created.number.and_then(|number| {
                        live_milestones
                            .iter()
                            .find(|m| m["number"].as_u64() == Some(number))
                    }),
                    None => {
                        let there = live_milestones
                            .iter()
                            .find(|m| text(m, "title").to_lowercase() == *title);
                        match there {
                            Some(m) if !has_state(m, "closed") => {
                                milestones_left_open.push(text(m, "title"));
                            }
                            Some(_) => {}
                            None => not_made.push(format!("milestone {title}")),
                        }
                        continue;
                    }
                },
            };
            let number = match found {
                Some(m) if has_state(m, "closed") => continue,
                Some(m) if all_there && !limited => m["number"].as_u64(),
                _ => None,
            };
            let done = match number {
                Some(number) => patched(
                    remote.api,
                    &format!("/repos/{repo}/milestones/{number}"),
                    &serde_json::json!({"state": "closed"}),
                )?,
                None => false,
            };
            if done {
                report.milestones_closed += 1;
            } else {
                limited = limited || number.is_some();
                report.state_waiting += 1;
            }
        }
    }

    report.left_out.extend(
        [
            named(
                "issues closed in the backup are open in the repo and were left open, as this \
                 restore did not make them",
                &left_open,
            ),
            named(
                "milestones closed in the backup are open in the repo and were left open, as \
                 this restore did not make them",
                &milestones_left_open,
            ),
            named(
                "issues or milestones of the backup are not in the repo and the create queue did \
                 not say this restore made them, so nothing was changed for them",
                &not_made,
            ),
            named(
                "issue bodies keep the backup's #N, not rewritten",
                &kept_numbers,
            ),
        ]
        .into_iter()
        .flatten(),
    );
    if limited {
        report.left_out.push(
            "GitHub answered a close or a body change with 403 or 429 (a rate limit, or the \
             token may not write): the rest waits"
                .to_string(),
        );
    }
    if on_prs > 0 {
        report.left_out.push(format!(
            "{on_prs} comments on PRs are not restored: PRs are not"
        ));
    }
    Ok(report)
}

/// BAK-3: restore the backup in `backup_dir` over the live repo `repo` (`owner/name`), as
/// `restore_over_live_from` does, against GitHub.
pub fn restore_over_live(
    caller: &Caller,
    fleet: &FleetBridge,
    backup_dir: &Path,
    repo: &str,
    confirmed: bool,
) -> Result<RestoreReport, CoreError> {
    with_github(|remote| restore_over_live_from(caller, fleet, backup_dir, repo, confirmed, remote))
}

/// `restore_over_live` against `remote`. Writes nothing unless `rules::guard` allows
/// `RestoreOverLive` for `caller` and `confirmed` is set. Then the backup is read and checked as
/// `restore_history_from` does, the repo must exist, its labels and milestones are read, the
/// bundle's refs are force-pushed to it, the labels and milestones it does not have yet are
/// queued, and `restore_issues` queues the issues and comments it does not have yet and puts
/// their state back. A second restore of the same backup therefore queues nothing. A label the
/// repo has with another colour is left as it is and named in `left_out`.
pub fn restore_over_live_from(
    caller: &Caller,
    fleet: &FleetBridge,
    backup_dir: &Path,
    repo: &str,
    confirmed: bool,
    remote: &Remote<'_>,
) -> Result<RestoreReport, CoreError> {
    let backup = live_backup(caller, backup_dir, repo, confirmed, remote)?;
    let (owner, name) = split_repo(repo)?;
    let work = work_dir(owner, name);
    let restored = clone_mirror(remote, &backup.bundle.to_string_lossy(), &work)
        .and_then(|()| check_refs(&work, &backup))
        .and_then(|()| {
            // What the repo has already is not created again.
            let live_labels: HashMap<String, Option<String>> =
                rest_all(remote.api, &format!("/repos/{repo}/labels?"))?
                    .iter()
                    .filter_map(|item| {
                        let name = item["name"].as_str()?.to_lowercase();
                        Some((name, filled(item, "color").map(|c| c.to_lowercase())))
                    })
                    .collect();
            let live_milestones: HashSet<String> =
                rest_all(remote.api, &format!("/repos/{repo}/milestones?state=all"))?
                    .iter()
                    .filter_map(|item| item["title"].as_str().map(str::to_lowercase))
                    .collect();
            let mut other_colour = Vec::new();
            let mut labels = Vec::new();
            for create in label_creates(&backup, repo) {
                let Create::Label { name, color, .. } = &create else {
                    continue;
                };
                match live_labels.get(&name.to_lowercase()) {
                    None => labels.push(create),
                    Some(live) => {
                        let backed_up = color.as_ref().map(|c| c.to_lowercase());
                        if backed_up.is_some() && live.is_some() && backed_up != *live {
                            other_colour.push(name.clone());
                        }
                    }
                }
            }
            let milestones: Vec<Create> = milestone_creates(&backup, repo)
                .into_iter()
                .filter(|c| !matches!(c, Create::Milestone { title, .. } if live_milestones.contains(&title.to_lowercase())))
                .collect();
            let refs = push_refs(remote, &work, repo).map_err(|e| about(repo, e))?;
            let before = queue_counts(fleet)?;
            let creates: Vec<Create> = labels.iter().chain(&milestones).cloned().collect();
            let keyed = queue_labels(fleet, repo, &creates, &before)?;
            // Only what this restore made is closed (#365): the milestones this batch added to
            // the queue, by the number the queue says it made, never what the repo had already.
            let mut made = Made::default();
            for (create, queued) in milestones.iter().zip(keyed.iter().skip(labels.len())) {
                if let (Create::Milestone { title, .. }, true) = (create, queued.added) {
                    made.milestones.insert(
                        title.to_lowercase(),
                        Created {
                            key: queued.key.clone(),
                            number: None,
                        },
                    );
                }
            }
            let issues =
                restore_issues_made(fleet, backup_dir, repo, remote, true, Some(&mut made))?;
            let mut left_out = left_out(&backup);
            if !other_colour.is_empty() {
                left_out.push(format!(
                    "{} labels are there with another colour and were left as they are: {}",
                    other_colour.len(),
                    other_colour.join(", ")
                ));
            }
            Ok(RestoreReport {
                refs,
                labels: labels.len() as u64,
                milestones: milestones.len() as u64,
                issues,
                left_out,
                made,
            })
        });
    let _ = std::fs::remove_dir_all(&work);
    restored
}

/// The checks before any write over the live repo `repo`: `rules::guard` allows
/// `RestoreOverLive` for `caller`, `confirmed` is set, the backup reads and matches its manifest
/// (`read_backup`), and the repo is there.
fn live_backup(
    caller: &Caller,
    backup_dir: &Path,
    repo: &str,
    confirmed: bool,
    remote: &Remote<'_>,
) -> Result<Backup, CoreError> {
    match rules::guard(
        caller,
        &GuardedAction::RestoreOverLive {
            repo: repo.to_string(),
        },
    ) {
        Verdict::Allow => {}
        Verdict::ToWaiting(why) | Verdict::Refuse(why) => return Err(CoreError::Refused(why)),
    }
    if !confirmed {
        return Err(CoreError::Refused(format!(
            "restore over {repo}: needs a confirm in the app"
        )));
    }
    split_repo(repo)?;
    let backup = read_backup(backup_dir)?;
    let path = format!("/repos/{repo}");
    match remote.api.rest_get(&path, None)?.status {
        200 => Ok(backup),
        404 => Err(CoreError::GitHub(format!(
            "{repo}: no such repo to restore over"
        ))),
        other => Err(CoreError::GitHub(format!("GET {path}: status {other}"))),
    }
}

/// BAK-3 (#347): what a restore over the live repo `repo` held back, as
/// `restore_held_over_live_from` does, against GitHub.
pub fn restore_held_over_live(
    caller: &Caller,
    fleet: &FleetBridge,
    backup_dir: &Path,
    repo: &str,
    confirmed: bool,
    made: &Made,
) -> Result<RestoreReport, CoreError> {
    with_github(|remote| {
        restore_held_over_live_from(caller, fleet, backup_dir, repo, confirmed, made, remote)
    })
}

/// The second confirm of a restore over the live repo `repo` (#347): after the same checks as
/// `restore_over_live_from`, only what the first restore held back is done
/// (`Pass::Held`): the comments whose issues are there now are queued, and the closes and body
/// changes that waited are sent, only to the issues and milestones in `made` (what the whole
/// pass made, with the numbers the create queue's own record gives them), so never to one the
/// repo had or someone else made. No ref is pushed and no label, milestone or issue is queued,
/// so nothing pushed to the repo since the first confirm is overwritten. The report's `refs`,
/// `labels` and `milestones` are 0, and its `made` is the record with any new numbers.
pub fn restore_held_over_live_from(
    caller: &Caller,
    fleet: &FleetBridge,
    backup_dir: &Path,
    repo: &str,
    confirmed: bool,
    made: &Made,
    remote: &Remote<'_>,
) -> Result<RestoreReport, CoreError> {
    live_backup(caller, backup_dir, repo, confirmed, remote)?;
    let mut made = made.clone();
    Ok(RestoreReport {
        issues: restore_issues_made(fleet, backup_dir, repo, remote, false, Some(&mut made))?,
        made,
        ..RestoreReport::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_formats_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_762_696_800), "2025-11-09T14:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn split_repo_keeps_the_folder_inside_github() {
        assert_eq!(split_repo("owner/name").unwrap(), ("owner", "name"));
        for bad in ["name", "a/b/c", "../x", "a/..", "/x", "a/", "a b/c"] {
            assert!(split_repo(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn two_work_dirs_in_a_row_differ() {
        let first = work_dir("owner", "demo");
        let second = work_dir("owner", "demo");
        assert_ne!(first, second);
    }
}
