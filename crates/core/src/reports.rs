//! `reports`: a fixed report for every finished job and every chat handoff, in BroCrew's
//! fixed-report shape plus the line our job cards lack ("what wasn't checked").

use crate::joblog::JobRow;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub id: String,
    pub project: Option<String>,
    /// REP-1 part 1: outcome first.
    pub outcome: String,
    /// REP-1 part 2: PR, files, lines added and removed.
    pub changed: Changed,
    /// REP-1 part 3: the build, test and lint output R40 puts in the PR, one line per check.
    pub checked: Vec<String>,
    /// REP-1 part 4: what wasn't checked; empty shows as "nothing".
    pub not_checked: Vec<String>,
    /// REP-1 part 5: cost.
    pub cost: Cost,
    /// REP-2: the work tree, one move per line.
    pub moves: Vec<Move>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changed {
    pub pr: Option<String>,
    pub files: Option<u64>,
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cost {
    pub pool: Option<String>,
    pub tokens: Option<u64>,
    pub usd: Option<f64>,
    pub seconds: Option<f64>,
}

/// REP-2: one move of a chat or job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub from_service: String,
    pub from_model: String,
    pub to_service: String,
    pub to_model: String,
    pub why: String,
}

/// REP-1: a report from a fleet done card (JSON) and its job-log rows.
pub fn from_card(card: &Value, rows: &[JobRow]) -> Report {
    todo!("REP-1")
}

/// REP-1: a report from a chat thread's rows (FD-13).
pub fn from_thread(thread_id: &str, rows: &[JobRow]) -> Report {
    todo!("REP-1")
}

/// REP-2: the moves of a thread, oldest first: one per change of service or model between
/// consecutive rows, with the `why` the later row carries.
pub fn moves(rows: &[JobRow]) -> Vec<Move> {
    todo!("REP-2")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub project: Option<String>,
    pub outcome: Option<String>,
    pub pool: Option<String>,
}

/// REP-3: the reports that match every set field of the filter, order kept.
pub fn filter<'a>(reports: &'a [Report], f: &Filter) -> Vec<&'a Report> {
    todo!("REP-3")
}
