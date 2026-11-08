//! `budget`: each pool against its pace line (BUD-1), edited in place (BUD-2), read on a timer
//! (BUD-3), and Claude's pool from its newest recorded reading plus a marked estimate (BUD-4).
//! All numbers come from `fleet budget --json`; the core never works pace out itself.

use crate::fleet::FleetBridge;
use crate::CoreError;
use serde_json::Value;

/// BUD-1: one row of the panel, from one pool's entry in `fleet budget --json`.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolRow {
    pub pool: String,
    /// Used, percent of the window (`pct`).
    pub pct: Option<f64>,
    /// The pace line, percent (`expected`).
    pub pace: Option<f64>,
    /// Points over (+) or under (-) pace (`delta`).
    pub delta: Option<f64>,
    /// `verdict`: e.g. `on-pace`, `push`, `brake`.
    pub verdict: String,
    /// Reading age in minutes (`reading_age_min`), and whether fleet calls it stale.
    pub reading_age_min: Option<i64>,
    pub stale: bool,
    /// `why`, shown when the row is expanded.
    pub why: String,
}

/// BUD-1: parse `fleet budget --json` (an object keyed by pool) into rows sorted by pool name.
pub fn rows_from_json(value: &Value) -> Result<Vec<PoolRow>, CoreError> {
    todo!("BUD-1")
}

/// BUD-2: the fields I can edit in place.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoolEdit {
    pub pool: String,
    pub plan: Option<String>,
    pub cap: Option<f64>,
    /// e.g. `Sun 6:00am`.
    pub reset: Option<String>,
    pub cost_usd: Option<f64>,
}

/// BUD-2: save the edit to the pipeline clone's `tools/fleet/config.json` and write a ledger
/// row, then re-read `budget`. U5: the edit stays local; a Waiting on you item asks for it to be
/// landed through review.
pub fn save_edit(fleet: &FleetBridge, edit: &PoolEdit) -> Result<Vec<PoolRow>, CoreError> {
    todo!("BUD-2")
}

/// BUD-3: refresh every pool that has a reader (`fleet budget --refresh`).
pub fn refresh(fleet: &FleetBridge) -> Result<Vec<PoolRow>, CoreError> {
    todo!("BUD-3")
}

/// BUD-4: Claude's pool: the newest recorded `get_usage` reading with its age, plus an estimate
/// from my sessions' local usage logs since that reading, kept apart.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudePool {
    pub reading_pct: Option<f64>,
    pub reading_age_min: Option<i64>,
    /// Tokens in local session logs since the reading; an estimate, labelled as one.
    pub estimate_tokens_since: u64,
}

pub fn claude_pool(
    reading: &PoolRow,
    session_logs: &[std::path::PathBuf],
    since: i64,
) -> Result<ClaudePool, CoreError> {
    todo!("BUD-4")
}
