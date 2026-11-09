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
    if creates.is_empty() {
        return Ok(QueueStatus::default());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file = std::env::temp_dir().join(format!(
        "switchboard-ghq-{}-{nanos}.json",
        std::process::id()
    ));
    let io = |e: String| CoreError::Io(format!("{}: {e}", file.display()));
    let body = serde_json::to_string(creates).map_err(|e| io(e.to_string()))?;
    std::fs::write(&file, body).map_err(|e| io(e.to_string()))?;
    let path = file.to_string_lossy().into_owned();
    let coordinator = creates
        .iter()
        .any(|c| matches!(c, Create::Label { .. } | Create::Milestone { .. }));
    let mut add = vec!["ghq", "add-batch", path.as_str()];
    if coordinator {
        add.push("--coordinator");
    }
    let added = fleet.run_json(&add);
    let _ = std::fs::remove_file(&file);
    added.map_err(CoreError::Fleet)?;
    if let Err(e) = fleet.run_json(&["ghq", "run"]) {
        if !matches!(e.exit_code, Some(75 | 1)) {
            return Err(CoreError::Fleet(e));
        }
    }
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
