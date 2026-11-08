//! CORE-4: the job log. One row per attempt, from the fleet ledger and from the local chat
//! rows (FD-13), in one shape. A field a row lacks is `None` and shows as "not recorded",
//! never a guess.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JobRow {
    pub id: Option<String>,
    pub kind: Option<String>,
    pub size: Option<String>,
    /// The service (the ledger's `lane`).
    pub lane: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub tokens: Option<u64>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub cost_usd: Option<f64>,
    pub seconds: Option<f64>,
    pub tools: Option<Value>,
    pub loc_changed: Option<u64>,
    pub files_changed: Option<u64>,
    pub outcome: Option<String>,
    pub failure_mode: Option<String>,
    pub failure_class: Option<String>,
    pub attempt: Option<u32>,
    pub rounds: Option<u32>,
    /// Ruled pipeline batch fields.
    pub rating: Option<String>,
    pub handoff_id: Option<String>,
    /// Asked of the pipeline batch (U9); "not recorded" until it lands.
    pub checks: Option<Value>,
    pub escalation_step: Option<String>,
    pub review_fix_loc: Option<u64>,
    /// Where the row came from.
    pub source: RowSource,
    /// FD-9: an implicit signal row, which FD-4 does not count in 1.0.
    pub implicit: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowSource {
    #[default]
    Ledger,
    Chat,
}

/// The text a view shows for a field a row lacks.
pub const NOT_RECORDED: &str = "not recorded";

/// Turn one ledger event (a JSON object) into a row. Unknown fields are ignored; missing ones
/// stay `None`.
pub fn row_from_ledger(event: &Value) -> JobRow {
    todo!("CORE-4")
}

/// Read every attempt row: the ledger's `end` events, then the chat rows, oldest first.
pub fn read_all(ledger_events: &[Value], chat_rows: &[JobRow]) -> Vec<JobRow> {
    todo!("CORE-4")
}
