//! GH-4: every create (issue, comment, label, milestone) goes through the pipeline's one
//! throttled create queue, `fleet ghq`, shared with fleet jobs (the create limit is per
//! account). The core adds items and runs the queue; it never calls `gh` to create.

use crate::fleet::FleetBridge;
use crate::CoreError;
use serde::Serialize;

/// One create, in the keys `fleet ghq add-batch` reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Create {
    Issue {
        repo: String,
        title: String,
        body: String,
        labels: Vec<String>,
        milestone: Option<String>,
    },
    Comment {
        repo: String,
        number: u64,
        body: String,
        marker: Option<String>,
    },
    Label {
        repo: String,
        name: String,
    },
    Milestone {
        repo: String,
        title: String,
    },
}

/// What the queue said after a run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueStatus {
    pub pending: u64,
    pub done: u64,
    pub exists: u64,
    pub failed: u64,
}

/// Add the creates as one batch (`fleet ghq add-batch <file>`; `--coordinator` when any is a
/// label or milestone), then `fleet ghq run`. A run that stopped on a rate limit (exit 75) is
/// not an error: the items stay queued and the status says so.
pub fn send(fleet: &FleetBridge, creates: &[Create]) -> Result<QueueStatus, CoreError> {
    todo!("GH-4")
}
