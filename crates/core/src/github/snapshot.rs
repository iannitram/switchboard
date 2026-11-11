//! GH-1: the snapshot, the cached GitHub read. One JSON file per repo in the app's data
//! folder. Every view after `github` reads only the snapshot.

use crate::clock::UnixSeconds;
use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepoSnapshot {
    /// `owner/name`.
    pub repo: String,
    /// When this read finished.
    pub read_at: UnixSeconds,
    /// The REST ETag GH-2 sends next time.
    pub etag: Option<String>,
    pub archived: bool,
    pub issues: Vec<Issue>,
    pub pulls: Vec<PullRequest>,
    pub milestones: Vec<Milestone>,
    pub labels: Vec<String>,
    /// ROADMAP.md on the default branch, if any.
    pub roadmap_md: Option<String>,
    /// docs/code-map.md on the default branch, if any.
    pub code_map_md: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    /// `open` or `closed`.
    pub state: String,
    pub labels: Vec<String>,
    pub milestone: Option<String>,
    pub body: String,
    /// Closed by a merged PR (BAK-2 tells these from closes no PR made).
    pub closed_by_pr: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    /// `open`, `closed` or `merged`.
    pub state: String,
    pub head_sha: String,
    pub head_ref: String,
    pub labels: Vec<String>,
    /// Issues it closes (`Closes #N`).
    pub closes: Vec<u64>,
    /// Every R25 verdict comment, oldest first.
    pub verdicts: Vec<ReviewVerdict>,
    /// The combined check state on the head: `success`, `failure`, `pending` or `none`.
    pub ci_on_head: String,
    /// A draft PR (#432: the merge pass skips it). Snapshots written before have none: false.
    #[serde(default)]
    pub draft: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewVerdict {
    /// `PASS` or `FAIL`.
    pub verdict: String,
    /// The head sha the verdict names.
    pub sha: String,
    /// True when the FAIL carried no fix (a design question or a decision of mine).
    pub needs_me: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Milestone {
    pub title: String,
    pub open_issues: u64,
    pub closed_issues: u64,
}

/// The snapshot store: `<data>/snapshot/<owner>__<name>.json`.
pub struct SnapshotStore {
    dir: PathBuf,
}

impl SnapshotStore {
    pub fn new(data_dir: &Path) -> Self {
        SnapshotStore {
            dir: data_dir.join("snapshot"),
        }
    }

    /// The file for `owner/name`: `<dir>/<owner>__<name>.json`.
    pub fn path_for(&self, repo: &str) -> PathBuf {
        self.dir.join(format!("{}.json", repo.replace('/', "__")))
    }

    /// Write one repo's snapshot, replacing the old one (write to a temp file, then rename).
    pub fn save(&self, snap: &RepoSnapshot) -> Result<(), CoreError> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| CoreError::Io(format!("{}: {e}", self.dir.display())))?;
        let target = self.path_for(&snap.repo);
        let text =
            serde_json::to_string_pretty(snap).map_err(|e| CoreError::Parse(e.to_string()))?;
        let mut tmp = target.clone().into_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, text).map_err(|e| CoreError::Io(format!("{}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &target)
            .map_err(|e| CoreError::Io(format!("{}: {e}", tmp.display())))?;
        Ok(())
    }

    /// Read one repo's snapshot; `Ok(None)` when there is none yet.
    pub fn load(&self, repo: &str) -> Result<Option<RepoSnapshot>, CoreError> {
        read_one(&self.path_for(repo))
    }

    /// Every stored snapshot that reads, sorted by repo name. A file that does not read is
    /// skipped (`load_each` names it), so one bad file hides no other repo (#350).
    pub fn load_all(&self) -> Result<Vec<RepoSnapshot>, CoreError> {
        self.load_each().map(|(snaps, _)| snaps)
    }

    /// Every stored snapshot that reads, sorted by repo name, and every file that does not,
    /// sorted by path. Only a snapshot folder that cannot be listed is an error; a missing one
    /// is no snapshots.
    pub fn load_each(&self) -> Result<(Vec<RepoSnapshot>, Vec<Unreadable>), CoreError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Vec::new(), Vec::new()))
            }
            Err(e) => return Err(CoreError::Io(format!("{}: {e}", self.dir.display()))),
        };
        let mut snaps = Vec::new();
        let mut unreadable = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| CoreError::Io(format!("{}: {e}", self.dir.display())))?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            match read_one(&path) {
                Ok(Some(snap)) => snaps.push(snap),
                Ok(None) => {}
                Err(e) => unreadable.push(Unreadable {
                    repo: repo_of(&path),
                    error: e.to_string(),
                    path,
                }),
            }
        }
        snaps.sort_by(|a, b| a.repo.cmp(&b.repo));
        unreadable.sort_by(|a, b| a.path.cmp(&b.path));
        Ok((snaps, unreadable))
    }
}

/// A snapshot file that does not read (#350): the repo its name gives, the file, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// `owner/name`, from the file name `<owner>__<name>.json`.
    pub repo: String,
    pub path: PathBuf,
    /// The read or parse error, naming the file.
    pub error: String,
}

/// `owner/name` from `<dir>/<owner>__<name>.json` (an owner has no `_`, so the first `__` is
/// the `/`).
fn repo_of(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    stem.replacen("__", "/", 1)
}

fn read_one(path: &Path) -> Result<Option<RepoSnapshot>, CoreError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))
}

/// The snapshot's age in whole minutes at `now` (each view shows it).
pub fn age_minutes(snap: &RepoSnapshot, now: UnixSeconds) -> i64 {
    (now - snap.read_at).max(0) / 60
}
