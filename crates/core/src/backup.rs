//! `backup`: one latest backup of each repo in `Backups/github/<repo>/` in NextCloud (BAK-1),
//! guarded against a bad pass overwriting the last good copy (BAK-2), restorable to a new
//! private repo (BAK-3).

use crate::fleet::FleetBridge;
use crate::github::queue::{self, Create};
use crate::github::transport::{GitHubTransport, HttpTransport};
use crate::settings::Settings;
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
            let url = asset["browser_download_url"].as_str().ok_or_else(|| {
                CoreError::Parse(format!("release {tag}: asset {name} has no download URL"))
            })?;
            let bytes = files.fetch(url)?;
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

/// BAK-1 and BAK-2: what one backup did. `Refused` holds `refuse_reason`'s text for the Waiting
/// on you item; the last backup was kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupOutcome {
    Written(Manifest),
    Refused(String),
}

/// Where a backup reads a repo from: git clones `<git_base>/<repo>.git` and
/// `<git_base>/<repo>.wiki.git`, with each `git_config` entry passed as `-c`, and the REST API
/// through `api`. The app uses `https://github.com` and `HttpTransport`; tests use a folder of
/// bare repos and recorded answers.
pub struct Remote<'a> {
    pub git_base: String,
    pub git_config: Vec<String>,
    pub api: &'a dyn GitHubTransport,
}

/// BAK-1: back up `repo` (`owner/name`) from GitHub into
/// `<nextcloud_backups>/github/<owner>/<name>/`, replacing the last backup unless BAK-2 refuses.
/// Git reads use `gh` as the credential helper and the API uses `gh auth token`.
pub fn backup_repo(settings: &Settings, repo: &str) -> Result<BackupOutcome, CoreError> {
    backups_dir(settings)?;
    let api = HttpTransport::new(&crate::platform::GhAuthToken::default())?;
    backup_repo_from(settings, repo, &github_remote(&api), &now_rfc3339())
}

/// BAK-1: back up every repo in `repos`, in order, each as `backup_repo` does. Stops at the
/// first error, which names the repo; backups already written stay written.
pub fn backup_all(
    settings: &Settings,
    repos: &[String],
) -> Result<Vec<(String, BackupOutcome)>, CoreError> {
    backups_dir(settings)?;
    let api = HttpTransport::new(&crate::platform::GhAuthToken::default())?;
    backup_all_from(settings, repos, &github_remote(&api), &now_rfc3339())
}

fn github_remote(api: &dyn GitHubTransport) -> Remote<'_> {
    Remote {
        git_base: "https://github.com".to_string(),
        git_config: vec![
            "credential.helper=".to_string(),
            "credential.helper=!gh auth git-credential".to_string(),
        ],
        api,
    }
}

/// `backup_all` against `remote`, every manifest stamped `taken_at`.
pub fn backup_all_from(
    settings: &Settings,
    repos: &[String],
    remote: &Remote<'_>,
    taken_at: &str,
) -> Result<Vec<(String, BackupOutcome)>, CoreError> {
    backups_dir(settings)?;
    let mut outcomes = Vec::new();
    for repo in repos {
        let outcome =
            backup_repo_from(settings, repo, remote, taken_at).map_err(|e| about(repo, e))?;
        outcomes.push((repo.clone(), outcome));
    }
    Ok(outcomes)
}

/// `backup_repo` against `remote`, the manifest stamped `taken_at` (RFC 3339).
///
/// Mirrors the repo into a temp folder, then builds the new backup beside the old one in
/// `.<name>.new`: `bundle`, `write_json`, `copy_wiki`, and the manifest with the ref count and
/// the wiki. Asks `refuse_reason` against the old folder's manifest; a refusal removes the new
/// folder and leaves the old one untouched. Otherwise the old folder moves to `.<name>.old`, the
/// new one takes its place, and the old one is removed. Any error before the swap leaves the
/// old folder as it was.
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
    let staging = parent.join(format!(".{name}.new"));
    let work = work_dir(owner, name);
    remove_dir(&staging)?;
    let built = build(repo, remote, taken_at, &work, &staging);
    let _ = std::fs::remove_dir_all(&work);
    let manifest = match built {
        Ok(manifest) => manifest,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let old = match read_manifest(&folder) {
        Ok(old) => old,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    if let Some(reason) = old.and_then(|old| refuse_reason(&old, &manifest)) {
        remove_dir(&staging)?;
        return Ok(BackupOutcome::Refused(reason));
    }
    swap_in(&staging, &folder, &parent.join(format!(".{name}.old")))?;
    Ok(BackupOutcome::Written(manifest))
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

    // A repo with no wiki (or no wiki pages yet) has nothing to clone.
    let wiki = work.join("wiki.git");
    let wiki_url = format!("{}/{repo}.wiki.git", remote.git_base);
    let wiki_mirror = clone_mirror(remote, &wiki_url, &wiki).ok().map(|()| wiki);
    copy_wiki(wiki_mirror.as_deref(), staging, &mut manifest)?;

    let path = staging.join("manifest.json");
    std::fs::write(&path, pretty(&manifest)?).map_err(|e| io_error(&path, e))?;
    Ok(manifest)
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

/// The last backup's manifest, or `None` when there is no last backup.
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

/// Moves `staging` to `folder`, the old `folder` aside to `aside` first and removed after. A
/// failed move puts the old folder back.
fn swap_in(staging: &Path, folder: &Path, aside: &Path) -> Result<(), CoreError> {
    remove_dir(aside)?;
    let had_old = folder.exists();
    if had_old {
        std::fs::rename(folder, aside).map_err(|e| io_error(folder, e))?;
    }
    if let Err(e) = std::fs::rename(staging, folder) {
        if had_old {
            let _ = std::fs::rename(aside, folder);
        }
        let _ = std::fs::remove_dir_all(staging);
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

/// A fresh temp folder for this backup's mirrors. The counter keeps two
/// backups in one process apart when the clock does not tick between them
/// (macOS reads it in whole microseconds).
fn work_dir(owner: &str, name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!(
        "switchboard-backup-{owner}-{name}-{}-{nanos}-{seq}",
        std::process::id()
    ))
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

/// BAK-3: what `restore_history` put back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreReport {
    /// Refs pushed to the new repo (every ref in the bundle but GitHub's own `refs/pull/`).
    pub refs: u64,
    /// Labels sent through the create queue.
    pub labels: u64,
    /// Milestones sent through the create queue.
    pub milestones: u64,
}

/// BAK-3, first half: restore a backup (`repo.bundle`, `labels.json`, `milestones.json` and
/// `manifest.json` in `backup_dir`) to the new private repo `new_repo` (`owner/name`).
///
/// GitHub is reached only through the fleet bridge. `fleet repo view <new_repo>` answers
/// `{"exists": bool}`; a name that exists is refused (`CoreError::Refused`) before any write,
/// since restoring over a live repo needs `rules::guard` first (#101). `fleet repo create
/// <new_repo> --private --coordinator` answers `{"url": <push url>, "private": bool}`; an answer
/// that is not private stops the restore before the push. Every ref in the bundle is pushed to
/// that URL (but `refs/pull/`, which GitHub keeps for itself) and read back with `ls-remote`.
/// The labels and milestones go through the create queue (`github::queue::send`), never `gh`.
///
/// Everything is read and checked first: a missing file, a malformed name, or a label or
/// milestone count that differs from the manifest is an error with no write.
pub fn restore_history(
    fleet: &FleetBridge,
    backup_dir: &Path,
    new_repo: &str,
) -> Result<RestoreReport, CoreError> {
    if !is_owner_name(new_repo) {
        return Err(CoreError::Refused(format!(
            "the repo to restore to must be owner/name, got {new_repo:?}"
        )));
    }
    let backup_dir = &std::path::absolute(backup_dir).map_err(|e| io_error(backup_dir, e))?;
    let manifest: Manifest = read_json(&backup_dir.join("manifest.json"))?;
    let labels = names_in(&backup_dir.join("labels.json"), "name")?;
    let milestones = names_in(&backup_dir.join("milestones.json"), "title")?;
    check_count("labels", labels.len(), manifest.labels)?;
    check_count("milestones", milestones.len(), manifest.milestones)?;
    let bundle = backup_dir.join("repo.bundle");
    if !bundle.is_file() {
        return Err(CoreError::Io(format!("{}: no bundle", bundle.display())));
    }

    let seen = fleet
        .run_json(&["repo", "view", new_repo])
        .map_err(CoreError::Fleet)?;
    if seen["exists"].as_bool() != Some(false) {
        return Err(CoreError::Refused(format!(
            "{new_repo} already exists: a restore goes to a new private repo, and over a live \
             repo only after I confirm in the app"
        )));
    }

    let unpacked = scratch_dir()?;
    let refs = unpack_refs(&bundle, &unpacked);
    let refs = match refs {
        Ok(refs) => refs,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&unpacked);
            return Err(e);
        }
    };

    let made = fleet
        .run_json(&["repo", "create", new_repo, "--private", "--coordinator"])
        .map_err(CoreError::Fleet);
    let pushed = made.and_then(|made| push_to_new(&made, new_repo, &unpacked, &refs));
    let _ = std::fs::remove_dir_all(&unpacked);
    pushed?;

    let creates: Vec<Create> = labels
        .iter()
        .map(|name| Create::Label {
            repo: new_repo.to_string(),
            name: name.clone(),
        })
        .chain(milestones.iter().map(|title| Create::Milestone {
            repo: new_repo.to_string(),
            title: title.clone(),
        }))
        .collect();
    queue::send(fleet, &creates)?;

    Ok(RestoreReport {
        refs: refs.len() as u64,
        labels: labels.len() as u64,
        milestones: milestones.len() as u64,
    })
}

/// `owner/name`: two non-empty parts of letters, digits, `-`, `_` and `.`.
fn is_owner_name(repo: &str) -> bool {
    let ok = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    match repo.split_once('/') {
        Some((owner, name)) => ok(owner) && ok(name),
        None => false,
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, CoreError> {
    let text = std::fs::read_to_string(path).map_err(|e| io_error(path, e))?;
    serde_json::from_str(&text).map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))
}

/// The `key` string of every item in the JSON array at `path`; an item without one is an error.
fn names_in(path: &Path, key: &str) -> Result<Vec<String>, CoreError> {
    let items: Vec<Value> = read_json(path)?;
    items
        .iter()
        .map(|item| {
            item[key].as_str().map(str::to_string).ok_or_else(|| {
                CoreError::Parse(format!("{}: an item has no {key}", path.display()))
            })
        })
        .collect()
}

fn check_count(what: &str, found: usize, manifest: u64) -> Result<(), CoreError> {
    if found as u64 == manifest {
        return Ok(());
    }
    Err(CoreError::Refused(format!(
        "the backup holds {found} {what}, its manifest says {manifest}"
    )))
}

/// A new, empty temp folder for the unpacked bundle.
fn scratch_dir() -> Result<PathBuf, CoreError> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "switchboard-restore-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
    Ok(dir)
}

/// Clone the bundle bare into `into/repo.git`; the refs it holds, sorted, but `refs/pull/`.
fn unpack_refs(bundle: &Path, into: &Path) -> Result<Vec<String>, CoreError> {
    let bare = into.join("repo.git");
    git(
        into,
        &[
            "clone",
            "-q",
            "--mirror",
            &bundle.to_string_lossy(),
            &bare.to_string_lossy(),
        ],
    )?;
    let listed = git(&bare, &["for-each-ref", "--format=%(refname)"])?;
    let refs = pushable(listed.lines());
    if refs.is_empty() {
        return Err(CoreError::Io(format!(
            "{}: the bundle holds no refs to restore",
            bundle.display()
        )));
    }
    Ok(refs)
}

/// The ref names in `names`, sorted and deduplicated, without `HEAD` and GitHub's own
/// `refs/pull/` (which a push may not write).
fn pushable<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut refs: Vec<String> = names
        .map(str::trim)
        .filter(|name| name.starts_with("refs/") && !name.starts_with("refs/pull/"))
        .filter(|name| !name.ends_with("^{}"))
        .map(str::to_string)
        .collect();
    refs.sort();
    refs.dedup();
    refs
}

/// Check the create answer, push every ref of the unpacked bundle to it, and read the refs back.
fn push_to_new(
    made: &Value,
    new_repo: &str,
    unpacked: &Path,
    refs: &[String],
) -> Result<(), CoreError> {
    if made["private"].as_bool() != Some(true) {
        return Err(CoreError::GitHub(format!(
            "{new_repo} was not created private; nothing was pushed"
        )));
    }
    let url = match made["url"].as_str() {
        Some(url) if !url.is_empty() => url,
        _ => {
            return Err(CoreError::GitHub(format!(
                "the create answer for {new_repo} has no url"
            )))
        }
    };
    let bare = unpacked.join("repo.git");
    git(
        &bare,
        &[
            "push",
            "-q",
            "--force",
            url,
            "refs/*:refs/*",
            "^refs/pull/*",
        ],
    )?;
    let remote = git(&bare, &["ls-remote", url])?;
    let there = pushable(remote.lines().filter_map(|line| line.split('\t').nth(1)));
    let missing: Vec<&String> = refs.iter().filter(|r| !there.contains(r)).collect();
    if !missing.is_empty() {
        return Err(CoreError::GitHub(format!(
            "{new_repo} is missing {} of the bundle's refs after the push, first {}",
            missing.len(),
            missing[0]
        )));
    }
    Ok(())
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
