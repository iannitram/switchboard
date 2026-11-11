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
    send_keyed(fleet, creates).map(|(status, _)| status)
}

/// What `fleet ghq add-batch` said of one create: the queue's key for it, and whether this batch
/// added it (`false`: the queue held an item with that key already, from an earlier batch or
/// from this one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    pub key: String,
    pub added: bool,
}

/// `send`, with what the queue said of each create, in the order of `creates`. When the queue's
/// answer to `add-batch` is not one entry per create, the list is empty: no create is known.
pub fn send_keyed(
    fleet: &FleetBridge,
    creates: &[Create],
) -> Result<(QueueStatus, Vec<Queued>), CoreError> {
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
    let keyed: Vec<Queued> = added
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(Queued {
                        key: item["key"].as_str()?.to_string(),
                        added: item["added"].as_bool()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let keyed = if keyed.len() == creates.len() {
        keyed
    } else {
        Vec::new()
    };
    if let Err(e) = fleet.run_json(&["ghq", "run"]) {
        if !matches!(e.exit_code, Some(75 | 1)) {
            return Err(CoreError::Fleet(e));
        }
    }
    let answer = fleet
        .run_json(&["ghq", "status"])
        .map_err(CoreError::Fleet)?;
    let count = |name: &str| answer["counts"][name].as_u64().unwrap_or(0);
    Ok((
        QueueStatus {
            pending: count("pending"),
            done: count("done"),
            exists: count("exists"),
            failed: count("failed"),
        },
        keyed,
    ))
}

/// One item as the queue's own record (`queue.json` in the folder `fleet ghq status` names)
/// holds it: its status (`pending`, `done`, `exists`, `failed`) and, once done or found, its
/// result (the created item's URL, or what was found).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub status: String,
    pub result: Option<String>,
}

/// The queue's own record of the items with these keys. A key the record does not hold is not
/// in the map; no record file yet is an empty map.
pub fn records(
    fleet: &FleetBridge,
    keys: &[&str],
) -> Result<std::collections::HashMap<String, Record>, CoreError> {
    let mut found = std::collections::HashMap::new();
    if keys.is_empty() {
        return Ok(found);
    }
    let answer = fleet
        .run_json(&["ghq", "status"])
        .map_err(CoreError::Fleet)?;
    let Some(dir) = answer["dir"].as_str() else {
        return Ok(found);
    };
    let dir = std::path::Path::new(dir);
    let file = if dir.is_absolute() {
        dir.join("queue.json")
    } else {
        fleet.root().join(dir).join("queue.json")
    };
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(e) => return Err(CoreError::Io(format!("{}: {e}", file.display()))),
    };
    let queue: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| CoreError::Parse(format!("{}: {e}", file.display())))?;
    for item in queue["items"].as_array().into_iter().flatten() {
        let Some(key) = item["key"].as_str().filter(|k| keys.contains(k)) else {
            continue;
        };
        found.insert(
            key.to_string(),
            Record {
                status: item["status"].as_str().unwrap_or_default().to_string(),
                result: item["result"].as_str().map(String::from),
            },
        );
    }
    Ok(found)
}
