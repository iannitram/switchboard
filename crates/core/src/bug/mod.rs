//! The bug-loop's core: the bug row and the owner table (`Bug`, `OwnerRow`), and the intake
//! steps that turn a dropped report into one tracked bug (`intake`).

pub mod intake;

use crate::CoreError;
use serde::{Deserialize, Serialize};

/// One row of `bugs.jsonl` in fleet-state (specs/bug-loop.json, record `bug`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bug {
    pub id: String,
    pub repo: String,
    pub issue: u64,
    /// The sha ids of every report filed under this bug; a duplicate adds its id here.
    pub reports: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_signature: Option<String>,
    pub severity: String,
    pub status: String,
    pub attempts: u64,
}

impl Bug {
    /// A bug still being worked: a repeat of it is a duplicate, not a new bug.
    pub fn is_open(&self) -> bool {
        matches!(self.status.as_str(), "open" | "reproduced")
    }
}

/// One row of the owner table (`config/bug-owners.json`): the app and the repo that owns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerRow {
    pub app: String,
    pub repo: String,
    #[serde(default)]
    pub test_cmd: String,
    #[serde(default)]
    pub test_dir: String,
    #[serde(default)]
    pub lint_cmd: Option<String>,
    #[serde(default)]
    pub release_style: String,
    #[serde(default)]
    pub siblings: Vec<String>,
}

/// Read the owner table: a JSON array of rows, or an object holding it under `owners`.
pub fn parse_owners(text: &str) -> Result<Vec<OwnerRow>, CoreError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| CoreError::Parse(format!("owner table: {e}")))?;
    let rows = match value {
        serde_json::Value::Object(mut o) => o.remove("owners").unwrap_or_default(),
        other => other,
    };
    serde_json::from_value(rows).map_err(|e| CoreError::Parse(format!("owner table: {e}")))
}
