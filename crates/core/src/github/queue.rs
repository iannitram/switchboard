//! GH-4: every create (issue, comment, label, milestone, repo) goes through the pipeline's one
//! throttled create queue, `fleet ghq`, shared with fleet jobs (the create limit is per
//! account). The core adds items and runs the queue; it never calls `gh` to create. A repo
//! item is created private when asked, answers `exists` when its name is taken, and the
//! new repo's URL comes back from the queue.

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
    /// `color` is six hex digits (the queue's own default when `None`); `description` one line.
    Label {
        repo: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// `due` is `YYYY-MM-DD`.
    Milestone {
        repo: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        due: Option<String>,
    },
    /// A new repo `repo` (owner/name), created private when `private` is set.
    Repo { repo: String, private: bool },
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
/// label, milestone or repo), then `fleet ghq run`. A run that stopped on a rate limit (exit 75) is
/// not an error: the items stay queued and the status says so.
pub fn send(fleet: &FleetBridge, creates: &[Create]) -> Result<QueueStatus, CoreError> {
    if creates.is_empty() {
        return Ok(QueueStatus::default());
    }
    // The counter keeps two sends in one process apart when the clock does not tick
    // between them (macOS reads it in whole microseconds).
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file = std::env::temp_dir().join(format!(
        "switchboard-ghq-{}-{nanos}-{seq}.json",
        std::process::id()
    ));
    let io = |e: String| CoreError::Io(format!("{}: {e}", file.display()));
    let body = serde_json::to_string(creates).map_err(|e| io(e.to_string()))?;
    std::fs::write(&file, body).map_err(|e| io(e.to_string()))?;
    let path = file.to_string_lossy().into_owned();
    let coordinator = creates.iter().any(|c| {
        matches!(
            c,
            Create::Label { .. } | Create::Milestone { .. } | Create::Repo { .. }
        )
    });
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
