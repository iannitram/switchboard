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
        todo!("GH-1: snapshot store")
    }

    /// Write one repo's snapshot, replacing the old one (write to a temp file, then rename).
    pub fn save(&self, snap: &RepoSnapshot) -> Result<(), CoreError> {
        todo!("GH-1: snapshot store")
    }

    /// Read one repo's snapshot; `Ok(None)` when there is none yet.
    pub fn load(&self, repo: &str) -> Result<Option<RepoSnapshot>, CoreError> {
        todo!("GH-1: snapshot store")
    }

    /// Every stored snapshot, sorted by repo name.
    pub fn load_all(&self) -> Result<Vec<RepoSnapshot>, CoreError> {
        todo!("GH-1: snapshot store")
    }
}

/// The snapshot's age in whole minutes at `now` (each view shows it).
pub fn age_minutes(snap: &RepoSnapshot, now: UnixSeconds) -> i64 {
    todo!("GH-1: snapshot store")
}
