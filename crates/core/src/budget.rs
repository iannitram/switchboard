//! `budget`: each pool against its pace line (BUD-1), edited in place (BUD-2), read on a timer
//! (BUD-3), and Claude's pool from its newest recorded reading plus a marked estimate (BUD-4).
//! All numbers come from `fleet budget --json`; the core never works pace out itself.

use crate::clock::{Clock, UnixSeconds};
use crate::fleet::FleetBridge;
use crate::joblog::NOT_RECORDED;
use crate::waiting::{Item, Source};
use crate::CoreError;
use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

/// BUD-1: one row of the panel, from one pool's entry in `fleet budget --json`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
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
    let pools = value
        .as_object()
        .ok_or_else(|| CoreError::Parse("budget: expected an object keyed by pool".into()))?;
    let mut rows = Vec::with_capacity(pools.len());
    for (pool, entry) in pools {
        let entry = entry
            .as_object()
            .ok_or_else(|| CoreError::Parse(format!("budget: pool {pool} is not an object")))?;
        rows.push(PoolRow {
            pool: pool.clone(),
            pct: entry.get("pct").and_then(|v| v.as_f64()),
            pace: entry.get("expected").and_then(|v| v.as_f64()),
            delta: entry.get("delta").and_then(|v| v.as_f64()),
            verdict: entry
                .get("verdict")
                .and_then(|v| v.as_str())
                .unwrap_or(NOT_RECORDED)
                .to_string(),
            reading_age_min: entry
                .get("reading_age_min")
                .and_then(|v| v.as_f64())
                .map(|a| a.floor() as i64),
            stale: entry
                .get("stale")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            why: entry
                .get("why")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        });
    }
    rows.sort_by(|a, b| a.pool.cmp(&b.pool));
    Ok(rows)
}

/// BUD-1: run `fleet budget --json` and parse it.
pub fn read(fleet: &FleetBridge) -> Result<Vec<PoolRow>, CoreError> {
    let value = fleet.run_json(&["budget"]).map_err(CoreError::Fleet)?;
    rows_from_json(&value)
}

/// BUD-2: the fields I can edit in place.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub struct PoolEdit {
    pub pool: String,
    pub plan: Option<String>,
    pub cap: Option<f64>,
    /// e.g. `Sun 6:00am`.
    pub reset: Option<String>,
    pub cost_usd: Option<f64>,
}

/// BUD-2: what a save gives back.
#[derive(Debug, Clone, PartialEq)]
pub struct EditSaved {
    pub rows: Vec<PoolRow>,
    pub ledger_row: Value,
    pub waiting: Item,
}

/// BUD-2: `<root>/tools/fleet/config.json` (next to `fleet.script()`).
pub fn config_path(fleet: &FleetBridge) -> PathBuf {
    fleet.root().join("tools").join("fleet").join("config.json")
}

/// BUD-2: `Sun 6:00am` to (`sun`, 6, 0).
pub fn parse_reset(s: &str) -> Result<(String, u32, u32), CoreError> {
    let refuse = || CoreError::Parse(format!("reset must look like Sun 6:00am, got {s}"));
    let parts: Vec<&str> = s.split_whitespace().collect();
    let (day, time) = match parts.as_slice() {
        [day, time] => (day.to_lowercase(), time.to_lowercase()),
        _ => return Err(refuse()),
    };
    if !["mon", "tue", "wed", "thu", "fri", "sat", "sun"].contains(&day.as_str()) {
        return Err(refuse());
    }
    let (clock, pm) = match (time.strip_suffix("am"), time.strip_suffix("pm")) {
        (Some(clock), _) => (clock, false),
        (None, Some(clock)) => (clock, true),
        (None, None) => return Err(refuse()),
    };
    let (hour, minute) = match clock.split_once(':') {
        Some((hour, minute)) => (hour, Some(minute)),
        None => (clock, None),
    };
    let two_digits =
        |f: &str| !f.is_empty() && f.len() <= 2 && f.bytes().all(|b| b.is_ascii_digit());
    if !two_digits(hour) {
        return Err(refuse());
    }
    let hour: u32 = hour.parse().map_err(|_| refuse())?;
    if !(1..=12).contains(&hour) {
        return Err(refuse());
    }
    let minute: u32 = match minute {
        None => 0,
        Some(m) if m.len() == 2 && m.bytes().all(|b| b.is_ascii_digit()) => {
            let m: u32 = m.parse().map_err(|_| refuse())?;
            if m > 59 {
                return Err(refuse());
            }
            m
        }
        Some(_) => return Err(refuse()),
    };
    let hour = match (hour, pm) {
        (12, false) => 0,
        (12, true) => 12,
        (h, false) => h,
        (h, true) => h + 12,
    };
    Ok((day, hour, minute))
}

/// BUD-2: append one JSON line to the ledger, creating its folders.
fn append_ledger(path: &Path, row: &Value) -> Result<(), CoreError> {
    let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(io)?;
    writeln!(file, "{row}").map_err(io)
}

/// BUD-2: save the edit to the pipeline clone's `tools/fleet/config.json` and append a ledger
/// row to `edits_log`, then re-read `budget`. A refused edit writes nothing. U5: the edit stays
/// local; the Waiting on you item asks for it to be landed through review.
pub fn save_edit(
    fleet: &FleetBridge,
    edit: &PoolEdit,
    edits_log: &Path,
    clock: &dyn Clock,
) -> Result<EditSaved, CoreError> {
    let parse = |why: &str| CoreError::Parse(why.to_string());
    if edit.plan.is_none() && edit.cap.is_none() && edit.reset.is_none() && edit.cost_usd.is_none()
    {
        return Err(parse(&format!(
            "pool edit for {} changes nothing",
            edit.pool
        )));
    }
    if edit.cap.is_some_and(|c| !(c.is_finite() && c >= 0.0)) {
        return Err(parse("cap must be a number 0 or more"));
    }
    if edit.cost_usd.is_some_and(|c| !(c.is_finite() && c >= 0.0)) {
        return Err(parse("cost_usd must be a number 0 or more"));
    }
    let reset = match &edit.reset {
        Some(reset) => Some(parse_reset(reset)?),
        None => None,
    };

    let path = config_path(fleet);
    let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", path.display()));
    let text = std::fs::read_to_string(&path).map_err(io)?;
    let mut config: Value =
        serde_json::from_str(&text).map_err(|e| parse(&format!("{}: {e}", path.display())))?;
    let pool = config
        .get_mut("pools")
        .and_then(Value::as_object_mut)
        .and_then(|pools| pools.get_mut(&edit.pool))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| parse(&format!("no pool {} in config.json", edit.pool)))?;

    let mut set = Map::new();
    let mut before = Map::new();
    if let Some(plan) = &edit.plan {
        let old = pool.insert("plan".into(), json!(plan));
        before.insert("plan".into(), old.unwrap_or(Value::Null));
        set.insert("plan".into(), json!(plan));
    }
    if let Some(cap) = edit.cap {
        let old = pool.insert("cap_usd".into(), json!(cap));
        before.insert("cap_usd".into(), old.unwrap_or(Value::Null));
        set.insert("cap_usd".into(), json!(cap));
    }
    if let Some(cost) = edit.cost_usd {
        let old = pool.insert("cost_usd".into(), json!(cost));
        before.insert("cost_usd".into(), old.unwrap_or(Value::Null));
        set.insert("cost_usd".into(), json!(cost));
    }
    if let Some((day, hour, minute)) = reset {
        let pace = pool
            .entry("pace")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| parse(&format!("pace of {} is not an object", edit.pool)))?;
        for (name, new) in [
            ("reset_weekday", json!(day)),
            ("reset_hour", json!(hour)),
            ("reset_minute", json!(minute)),
        ] {
            let old = pace.insert(name.into(), new.clone());
            before.insert(format!("pace.{name}"), old.unwrap_or(Value::Null));
            set.insert(format!("pace.{name}"), new);
        }
    }

    let now = clock.now();
    let mut out = serde_json::to_string_pretty(&config)
        .map_err(|e| parse(&format!("{}: {e}", path.display())))?;
    out.push('\n');
    std::fs::write(&path, out).map_err(io)?;

    let ledger_row = json!({
        "at": now,
        "pool": edit.pool,
        "set": Value::Object(set),
        "before": Value::Object(before),
    });
    append_ledger(edits_log, &ledger_row)?;

    let rows = read(fleet)?;
    let waiting = Item {
        what: format!("Land the {} pool edit", edit.pool),
        why: "A pool edit is saved in the local pipeline clone only; it lands through review (U5)."
            .to_string(),
        source: Source::PoolEdit {
            pool: edit.pool.clone(),
        },
        link: path.display().to_string(),
        action: "Open config.json".to_string(),
        since: now,
    };
    Ok(EditSaved {
        rows,
        ledger_row,
        waiting,
    })
}

/// BUD-3: refresh every pool that has a reader (`fleet budget --refresh --json`).
pub fn refresh(fleet: &FleetBridge) -> Result<Vec<PoolRow>, CoreError> {
    let value = fleet
        .run_json(&["budget", "--refresh"])
        .map_err(CoreError::Fleet)?;
    rows_from_json(&value)
}

/// BUD-3: fleet's gate_fresh_minutes (30) in seconds.
pub const REFRESH_EVERY_SECS: i64 = 30 * 60;

/// BUD-3: when the budget is next refreshed, on the injected clock. The first tick is always
/// due; after that a refresh waits `every` seconds from the last attempt, failed or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshTimer {
    every: i64,
    last: Option<UnixSeconds>,
}

impl RefreshTimer {
    pub fn new(every: i64) -> Self {
        Self { every, last: None }
    }

    /// Due at once if it never ran; otherwise `every` seconds after the last attempt. A clock
    /// behind the last attempt is not due.
    pub fn due(&self, now: UnixSeconds) -> bool {
        match self.last {
            None => true,
            Some(last) => now.saturating_sub(last) >= self.every,
        }
    }

    /// Refresh if due at the clock's time, recording that attempt first so a failed refresh
    /// still waits the full interval. `None` when not due (fleet is not run).
    pub fn tick(
        &mut self,
        fleet: &FleetBridge,
        clock: &dyn Clock,
    ) -> Option<Result<Vec<PoolRow>, CoreError>> {
        let now = clock.now();
        if !self.due(now) {
            return None;
        }
        self.last = Some(now);
        Some(refresh(fleet))
    }
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
