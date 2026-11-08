//! GH-1, GH-2, GH-3: the 10-minute GraphQL poll, the ETag skip and refresh-on-action.

use super::snapshot::{RepoSnapshot, SnapshotStore};
use super::transport::GitHubTransport;
use crate::clock::{Clock, UnixSeconds};
use crate::CoreError;
use serde_json::Value;

/// GH-1: ten minutes between reads.
pub const POLL_SECONDS: i64 = 600;

/// GH-1: the GraphQL query for one repo: issues (labels, milestone, body, closing PR), labels,
/// milestones, PRs (head, labels, closing refs, review verdict comments, check state),
/// ROADMAP.md and docs/code-map.md.
pub const REPO_QUERY: &str = "query { __typename }"; // GH-1 fills this in

/// GH-1: turn the GraphQL `data` for one repo into a snapshot.
pub fn parse_repo(
    repo: &str,
    data: &Value,
    read_at: UnixSeconds,
) -> Result<RepoSnapshot, CoreError> {
    todo!("GH-1")
}

/// GH-2: true when the REST conditional request answered 304 (unchanged), so no GraphQL read.
pub fn unchanged(
    transport: &dyn GitHubTransport,
    repo: &str,
    etag: Option<&str>,
) -> Result<(bool, Option<String>), CoreError> {
    todo!("GH-2")
}

/// The poller: which repos are due, read them, store them.
pub struct Poller<'a> {
    pub transport: &'a dyn GitHubTransport,
    pub store: &'a SnapshotStore,
    pub clock: &'a dyn Clock,
    /// The constellations' repos, `owner/name`.
    pub repos: Vec<String>,
}

impl Poller<'_> {
    /// GH-1: read every repo whose snapshot is `POLL_SECONDS` old or more (or missing). Returns
    /// the repos read.
    pub fn tick(&self) -> Result<Vec<String>, CoreError> {
        todo!("GH-1")
    }

    /// The Refresh button: read every repo now, whatever its age.
    pub fn refresh_all(&self) -> Result<Vec<String>, CoreError> {
        todo!("GH-1")
    }

    /// GH-3: re-read one repo right after a write to it.
    pub fn after_write(&self, repo: &str) -> Result<(), CoreError> {
        todo!("GH-3")
    }
}
