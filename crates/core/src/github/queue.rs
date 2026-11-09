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
    Label {
        repo: String,
        name: String,
    },
    Milestone {
        repo: String,
        title: String,
    },
    /// A new repo `repo` (owner/name), created private when `private` is set.
    Repo {
        repo: String,
        private: bool,
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

/// What the queue said about one create of a send: its `status` (`done`, `exists`, `failed`,
/// `pending`, ...) and the `detail` the run gave (for a created repo or issue, its URL).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub status: String,
    pub detail: Option<String>,
}

/// Add the creates as one batch (`fleet ghq add-batch <file>`; `--coordinator` when any is a
/// label, milestone or repo), then `fleet ghq run`. A run that stopped on a rate limit (exit 75) is
/// not an error: the items stay queued and the status says so.
pub fn send(fleet: &FleetBridge, creates: &[Create]) -> Result<QueueStatus, CoreError> {
    send_each(fleet, creates).map(|(status, _)| status)
}

/// `send`, plus what the queue said about each create, in the order given. A create's answer
/// is its item in the `ghq run` report, matched by the key `ghq add-batch` gave it; an item that
/// was already queued keeps the status `add-batch` gave. `None` when the queue said nothing
/// about it (a run that stopped on a rate limit or a failure gives no report).
pub fn send_each(
    fleet: &FleetBridge,
    creates: &[Create],
) -> Result<(QueueStatus, Vec<Option<Sent>>), CoreError> {
    if creates.is_empty() {
        return Ok((QueueStatus::default(), Vec::new()));
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
    let added = added.map_err(CoreError::Fleet)?;
    let report = match fleet.run_json(&["ghq", "run"]) {
        Ok(report) => report,
        Err(e) if matches!(e.exit_code, Some(75 | 1)) => serde_json::Value::Null,
        Err(e) => return Err(CoreError::Fleet(e)),
    };
    let answer = fleet
        .run_json(&["ghq", "status"])
        .map_err(CoreError::Fleet)?;
    let count = |name: &str| answer["counts"][name].as_u64().unwrap_or(0);
    let status = QueueStatus {
        pending: count("pending"),
        done: count("done"),
        exists: count("exists"),
        failed: count("failed"),
    };
    let each = (0..creates.len())
        .map(|i| sent_for(&added[i], &report))
        .collect();
    Ok((status, each))
}

/// The answer for one `add-batch` row: its item in the run report, else the status it was
/// queued with when it was already there.
fn sent_for(row: &serde_json::Value, report: &serde_json::Value) -> Option<Sent> {
    let text = |v: &serde_json::Value| v.as_str().filter(|t| !t.is_empty()).map(str::to_string);
    let key = text(&row["key"])?;
    let ran = report["items"].as_array().and_then(|items| {
        items
            .iter()
            .find(|item| text(&item["key"]) == Some(key.clone()))
    });
    match ran {
        Some(item) => Some(Sent {
            status: text(&item["status"])?,
            detail: text(&item["detail"]),
        }),
        None if row["added"].as_bool() == Some(false) => Some(Sent {
            status: text(&row["status"])?,
            detail: None,
        }),
        None => None,
    }
}
