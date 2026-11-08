//! `budget`: each pool against its pace line (BUD-1), edited in place (BUD-2), read on a timer
//! (BUD-3), and Claude's pool from its newest recorded reading plus a marked estimate (BUD-4).
//! All numbers come from `fleet budget --json`; the core never works pace out itself.

use crate::fleet::FleetBridge;
use crate::CoreError;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    /// Assistant messages those tokens came from.
    pub estimate_messages: u64,
}

impl ClaudePool {
    /// The reading, labelled as a reading. A missing reading is never filled in.
    pub fn reading_label(&self) -> String {
        match (self.reading_pct, self.reading_age_min) {
            (None, _) => "no reading".to_string(),
            (Some(p), Some(a)) => format!("{p:.1}% (reading, {a} min old)"),
            (Some(p), None) => format!("{p:.1}% (reading, age not recorded)"),
        }
    }

    /// The estimate, labelled as an estimate.
    pub fn estimate_label(&self) -> String {
        format!(
            "estimate: {} tokens in {} messages since the reading",
            self.estimate_tokens_since, self.estimate_messages
        )
    }
}

/// BUD-4: `YYYY-MM-DDTHH:MM:SS[.fraction]Z` to Unix seconds (the fraction is dropped);
/// anything else is None.
pub fn parse_utc(ts: &str) -> Option<i64> {
    let b = ts.as_bytes();
    if b.len() < 20 || b.last() != Some(&b'Z') {
        return None;
    }
    if b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    if b.len() > 20 {
        // A fraction: '.' then one or more digits, then the Z.
        if b[19] != b'.' || b.len() < 22 || !b[20..b.len() - 1].iter().all(u8::is_ascii_digit) {
            return None;
        }
    }
    let year = digits(&b[0..4])?;
    let month = digits(&b[5..7])?;
    let day = digits(&b[8..10])?;
    let hour = digits(&b[11..13])?;
    let minute = digits(&b[14..16])?;
    let second = digits(&b[17..19])?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// All ASCII digits in `b`, as a number; anything else is None.
fn digits(b: &[u8]) -> Option<i64> {
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(b.iter().fold(0i64, |acc, d| acc * 10 + i64::from(d - b'0')))
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's civil-days algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// BUD-4: Claude's pool. The reading is copied from the claude row as it is; the estimate counts
/// the assistant messages in `session_logs` at or after `since` (Unix seconds), each message once
/// (deduped by `message.id` and `requestId`). A log that does not exist is skipped.
pub fn claude_pool(
    reading: &PoolRow,
    session_logs: &[PathBuf],
    since: i64,
) -> Result<ClaudePool, CoreError> {
    if reading.pool != "claude" {
        return Err(CoreError::Parse(format!(
            "claude_pool needs the claude row, got {}",
            reading.pool
        )));
    }
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut tokens = 0u64;
    let mut messages = 0u64;
    for path in session_logs {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(CoreError::Io(format!("{}: {}", path.display(), e))),
        };
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Some(at) = v.get("timestamp").and_then(Value::as_str).and_then(parse_utc) else {
                continue;
            };
            if at < since {
                continue;
            }
            let Some(usage) = v.pointer("/message/usage").filter(|u| u.is_object()) else {
                continue;
            };
            let message_id = v.pointer("/message/id").and_then(Value::as_str);
            let request_id = v.get("requestId").and_then(Value::as_str);
            if let (Some(mid), Some(rid)) = (message_id, request_id) {
                if !seen.insert((mid.to_string(), rid.to_string())) {
                    continue;
                }
            }
            let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
            tokens += count("input_tokens")
                + count("output_tokens")
                + count("cache_creation_input_tokens");
            messages += 1;
        }
    }
    Ok(ClaudePool {
        reading_pct: reading.pct,
        reading_age_min: reading.reading_age_min,
        estimate_tokens_since: tokens,
        estimate_messages: messages,
    })
}

/// BUD-4: every `*.jsonl` under `projects_dir`, at any depth, sorted. A missing folder is empty.
pub fn session_log_paths(projects_dir: &Path) -> Result<Vec<PathBuf>, CoreError> {
    let mut found = Vec::new();
    let mut folders = vec![projects_dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(CoreError::Io(format!("{}: {}", folder.display(), e))),
        };
        for entry in entries {
            let entry = entry.map_err(|e| CoreError::Io(format!("{}: {}", folder.display(), e)))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|e| CoreError::Io(format!("{}: {}", path.display(), e)))?;
            if file_type.is_dir() {
                folders.push(path);
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".jsonl"))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}
