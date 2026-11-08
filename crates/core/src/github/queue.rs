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
    std::fs::write(
        &file,
        serde_json::to_string(creates).map_err(|e| CoreError::Parse(e.to_string()))?,
    )
    .map_err(|e| CoreError::Io(format!("{}: {e}", file.display())))?;

    let path = file.to_string_lossy();
    let mut args: Vec<&str> = vec!["ghq", "add-batch", &path];
    if creates
        .iter()
        .any(|c| matches!(c, Create::Label { .. } | Create::Milestone { .. }))
    {
        args.push("--coordinator");
    }
    let added = fleet.run_json(&args);
    let _ = std::fs::remove_file(&file);
    added.map_err(CoreError::Fleet)?;

    match fleet.run_json(&["ghq", "run"]) {
        Ok(_) => {}
        Err(e) => match e.exit_code {
            Some(1) | Some(75) => {}
            _ => return Err(CoreError::Fleet(e)),
        },
    }

    let answer = fleet
        .run_json(&["ghq", "status"])
        .map_err(CoreError::Fleet)?;
    let counts = answer.get("counts");
    let count = |name: &str| {
        counts
            .and_then(|c| c.get(name))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    };
    Ok(QueueStatus {
        pending: count("pending"),
        done: count("done"),
        exists: count("exists"),
        failed: count("failed"),
    })
}
