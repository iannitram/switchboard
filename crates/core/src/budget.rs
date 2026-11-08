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
    /// Assistant messages those tokens came from.
    pub estimate_messages: u64,
}

impl ClaudePool {
    /// The recorded reading with its age, or "no reading". Never filled in from the estimate.
    pub fn reading_label(&self) -> String {
        match (self.reading_pct, self.reading_age_min) {
            (None, _) => "no reading".to_string(),
            (Some(p), Some(a)) => format!("{p:.1}% (reading, {a} min old)"),
            (Some(p), None) => format!("{p:.1}% (reading, age not recorded)"),
        }
    }

    /// The estimate from local session logs, labelled as one.
    pub fn estimate_label(&self) -> String {
        format!(
            "estimate: {} tokens in {} messages since the reading",
            self.estimate_tokens_since, self.estimate_messages
        )
    }
}

/// Days from 1970-01-01 to the given civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The number in `bytes[range]`, if every byte there is an ASCII digit.
fn digits(bytes: &[u8], range: std::ops::Range<usize>) -> Option<i64> {
    let field = bytes.get(range)?;
    if field.is_empty() || !field.iter().all(u8::is_ascii_digit) {
        return None;
    }
    field.iter().try_fold(0i64, |n, b| {
        n.checked_mul(10)?.checked_add(i64::from(b - b'0'))
    })
}

/// `YYYY-MM-DDTHH:MM:SS[.fraction]Z` to Unix seconds; anything else is None.
pub fn parse_utc(ts: &str) -> Option<i64> {
    let b = ts.as_bytes();
    if b.len() < 20 || b[b.len() - 1] != b'Z' {
        return None;
    }
    if b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    match b.len() {
        20 => {}
        n if b[19] == b'.' && n > 21 => {
            digits(b, 20..n - 1)?;
        }
        _ => return None,
    }
    let y = digits(b, 0..4)?;
    let m = digits(b, 5..7)?;
    let d = digits(b, 8..10)?;
    let h = digits(b, 11..13)?;
    let mi = digits(b, 14..16)?;
    let s = digits(b, 17..19)?;
    if !(1..=12).contains(&m)
        || !(1..=31).contains(&d)
        || !(0..=23).contains(&h)
        || !(0..=59).contains(&mi)
        || !(0..=59).contains(&s)
    {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86400 + h * 3600 + mi * 60 + s)
}

/// The usage of one log line, if it counts toward the estimate: it parses as JSON, has a
/// `timestamp` `parse_utc` accepts at or after `since`, and `message.usage` is an object.
fn counted_usage(line: &str, since: i64) -> Option<Value> {
    let v: Value = serde_json::from_str(line).ok()?;
    let at = parse_utc(v.get("timestamp")?.as_str()?)?;
    if at < since {
        return None;
    }
    v.get("message")?.get("usage")?.as_object()?;
    Some(v)
}

pub fn claude_pool(
    reading: &PoolRow,
    session_logs: &[std::path::PathBuf],
    since: i64,
) -> Result<ClaudePool, CoreError> {
    if reading.pool != "claude" {
        return Err(CoreError::Parse(format!(
            "claude_pool needs the claude row, got {}",
            reading.pool
        )));
    }
    let mut seen = std::collections::HashSet::new();
    let mut tokens: u64 = 0;
    let mut messages: u64 = 0;
    for path in session_logs {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(CoreError::Io(format!("{}: {}", path.display(), e))),
        };
        for line in text.lines() {
            let Some(v) = counted_usage(line, since) else {
                continue;
            };
            let message = &v["message"];
            if let (Some(id), Some(req)) = (message["id"].as_str(), v["requestId"].as_str()) {
                if !seen.insert((id.to_string(), req.to_string())) {
                    continue;
                }
            }
            let usage = &message["usage"];
            let field = |name: &str| usage[name].as_u64().unwrap_or(0);
            tokens += field("input_tokens")
                + field("output_tokens")
                + field("cache_creation_input_tokens");
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

/// Every `*.jsonl` under `projects_dir`, at any depth, sorted.
pub fn session_log_paths(
    projects_dir: &std::path::Path,
) -> Result<Vec<std::path::PathBuf>, CoreError> {
    fn walk(
        dir: &std::path::Path,
        out: &mut Vec<std::path::PathBuf>,
        top: bool,
    ) -> Result<(), CoreError> {
        let io = |e: std::io::Error| CoreError::Io(format!("{}: {}", dir.display(), e));
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if top && e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io(e)),
        };
        for entry in entries {
            let entry = entry.map_err(io)?;
            let path = entry.path();
            if entry.file_type().map_err(io)?.is_dir() {
                walk(&path, out, false)?;
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".jsonl"))
            {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(projects_dir, &mut out, true)?;
    out.sort();
    Ok(out)
}
