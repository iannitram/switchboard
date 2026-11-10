//! `budget`: each pool against its pace line (BUD-1), edited in place (BUD-2), read on a timer
//! (BUD-3), and Claude's pool from its newest recorded reading plus a marked estimate (BUD-4).
//! All numbers come from `fleet budget --json`; the core never works pace out itself.

use crate::clock::{Clock, UnixSeconds};
use crate::fleet::FleetBridge;
use crate::joblog::NOT_RECORDED;
use crate::CoreError;
use serde_json::Value;

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
    /// `fleet budget --json` read again after the save.
    pub rows: Vec<PoolRow>,
    /// The line appended to the pool-edit ledger.
    pub ledger_row: Value,
    /// The Waiting on you item asking for the edit to be landed through review (U5).
    pub waiting: crate::waiting::Item,
}

/// BUD-2: `<root>/tools/fleet/config.json` (next to `fleet.script()`).
pub fn config_path(fleet: &FleetBridge) -> std::path::PathBuf {
    fleet.script().with_file_name("config.json")
}

const WEEKDAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// `1` to `12` from one or two ASCII digits.
fn clock_hour(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let h: u32 = s.parse().ok()?;
    (1..=12).contains(&h).then_some(h)
}

/// `00` to `59`, exactly two ASCII digits.
fn clock_minute(s: &str) -> Option<u32> {
    if s.len() != 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let m: u32 = s.parse().ok()?;
    (m <= 59).then_some(m)
}

fn reset_parts(s: &str) -> Option<(String, u32, u32)> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let [day, time] = parts.as_slice() else {
        return None;
    };
    let day = day.to_lowercase();
    if !WEEKDAYS.contains(&day.as_str()) {
        return None;
    }
    let time = time.to_lowercase();
    let (clock, pm) = if let Some(clock) = time.strip_suffix("am") {
        (clock, false)
    } else {
        (time.strip_suffix("pm")?, true)
    };
    let (hour, minute) = match clock.split_once(':') {
        Some((h, m)) => (clock_hour(h)?, clock_minute(m)?),
        None => (clock_hour(clock)?, 0),
    };
    let hour = match (hour, pm) {
        (12, false) => 0,
        (12, true) => 12,
        (h, true) => h + 12,
        (h, false) => h,
    };
    Some((day, hour, minute))
}

/// BUD-2: `Sun 6:00am` to (`sun`, 6, 0).
pub fn parse_reset(s: &str) -> Result<(String, u32, u32), CoreError> {
    reset_parts(s)
        .ok_or_else(|| CoreError::Parse(format!("reset must look like Sun 6:00am, got {s}")))
}

/// A number 0 or more and finite, as config.json holds it.
fn non_negative(value: Option<f64>, name: &str) -> Result<Option<Value>, CoreError> {
    match value {
        None => Ok(None),
        Some(v) if v.is_finite() && v >= 0.0 => Ok(Some(Value::from(v))),
        Some(_) => Err(CoreError::Parse(format!(
            "{name} must be a number 0 or more"
        ))),
    }
}

/// BUD-2: the largest dollar cap an edit may set (a typo guard, not a policy).
pub const MAX_CAP_USD: f64 = 100_000.0;

/// A cap: finite, more than 0 and at most `MAX_CAP_USD`. A negative number keeps the same
/// refusal text as any other number field.
fn cap_value(value: Option<f64>) -> Result<Option<Value>, CoreError> {
    match value {
        Some(v) if v.is_finite() && v > 0.0 && v <= MAX_CAP_USD => Ok(Some(Value::from(v))),
        Some(v) if !v.is_finite() || v < 0.0 => non_negative(Some(v), "cap"),
        Some(_) => Err(CoreError::Parse(format!(
            "cap must be more than 0 and at most {MAX_CAP_USD}"
        ))),
        None => Ok(None),
    }
}

/// BUD-2: save the edit to the pipeline clone's `tools/fleet/config.json` and append a row to
/// `edits_log`, then re-read `budget`. U5: the edit stays local; the Waiting on you item it
/// gives back asks for it to be landed through review. An `Err` means nothing changed: the
/// ledger row goes first, then `config.json` (replaced whole), then the re-read; a failure
/// after a write puts the file and the ledger back as they were. A cap is refused on a pool
/// with no dollar cap (no `cap_usd` and not `fixed-period`) and outside 0 to `MAX_CAP_USD`.
pub fn save_edit(
    fleet: &FleetBridge,
    edit: &PoolEdit,
    edits_log: &std::path::Path,
    clock: &dyn Clock,
) -> Result<EditSaved, CoreError> {
    let pool = edit.pool.as_str();
    if edit.plan.is_none() && edit.cap.is_none() && edit.reset.is_none() && edit.cost_usd.is_none()
    {
        return Err(CoreError::Parse(format!(
            "pool edit for {pool} changes nothing"
        )));
    }
    let cap = cap_value(edit.cap)?;
    let cost = non_negative(edit.cost_usd, "cost_usd")?;
    let reset = edit.reset.as_deref().map(parse_reset).transpose()?;

    let path = config_path(fleet);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| CoreError::Io(format!("{}: {e}", path.display())))?;
    let mut config: Value = serde_json::from_str(&text)
        .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))?;
    let entry = config
        .get_mut("pools")
        .and_then(|pools| pools.get_mut(pool))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| CoreError::Parse(format!("no pool {pool} in config.json")))?;

    if cap.is_some()
        && entry.get("cap_usd").is_none()
        && entry.get("kind").and_then(Value::as_str) != Some("fixed-period")
    {
        return Err(CoreError::Parse(format!(
            "pool {pool} has no dollar cap to set"
        )));
    }

    let mut set = serde_json::Map::new();
    let mut before = serde_json::Map::new();
    let mut put =
        |entry: &mut serde_json::Map<String, Value>, key: &str, ledger_key: &str, v: Value| {
            let old = entry.insert(key.to_string(), Value::clone(&v));
            before.insert(ledger_key.to_string(), old.unwrap_or(Value::Null));
            set.insert(ledger_key.to_string(), v);
        };
    if let Some(plan) = &edit.plan {
        put(entry, "plan", "plan", Value::from(plan.as_str()));
    }
    if let Some(cap) = cap {
        put(entry, "cap_usd", "cap_usd", cap);
    }
    if let Some(cost) = cost {
        put(entry, "cost_usd", "cost_usd", cost);
    }
    if let Some((day, hour, minute)) = reset {
        let pace = entry
            .entry("pace")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !pace.is_object() {
            *pace = Value::Object(serde_json::Map::new());
        }
        let pace = pace.as_object_mut().expect("pace is an object");
        put(
            pace,
            "reset_weekday",
            "pace.reset_weekday",
            Value::from(day),
        );
        put(pace, "reset_hour", "pace.reset_hour", Value::from(hour));
        put(
            pace,
            "reset_minute",
            "pace.reset_minute",
            Value::from(minute),
        );
    }

    let mut out = serde_json::to_string_pretty(&config)
        .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))?;
    out.push('\n');

    let now = clock.now();
    let ledger_row = serde_json::json!({
        "at": now,
        "pool": pool,
        "set": Value::Object(set),
        "before": Value::Object(before),
    });
    let ledger_undo = append_line(edits_log, &ledger_row)?;
    if let Err(err) = replace_file(&path, &out) {
        let stuck: Vec<String> = ledger_undo.undo().err().into_iter().collect();
        return Err(with_undo_failures(err, stuck));
    }
    let rows = match read(fleet) {
        Ok(rows) => rows,
        Err(err) => {
            let mut stuck: Vec<String> = ledger_undo.undo().err().into_iter().collect();
            stuck.extend(replace_file(&path, &text).err().map(|e| e.to_string()));
            return Err(with_undo_failures(err, stuck));
        }
    };
    let waiting = crate::waiting::Item {
        what: format!("Land the {pool} pool edit"),
        why: "A pool edit is saved in the local pipeline clone only; it lands through review (U5)."
            .to_string(),
        source: crate::waiting::Source::PoolEdit {
            pool: pool.to_string(),
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

/// How to take back one `append_line`.
struct LedgerUndo<'a> {
    log: &'a std::path::Path,
    /// The length before the append; `None`: the file did not exist.
    len: Option<u64>,
}

impl LedgerUndo<'_> {
    /// Take the line back off; the error text says what could not be put back.
    fn undo(self) -> Result<(), String> {
        let io = |e: std::io::Error| format!("{}: {e}", self.log.display());
        match self.len {
            None => match std::fs::remove_file(self.log) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
                _ => Ok(()),
            },
            Some(len) => std::fs::OpenOptions::new()
                .write(true)
                .open(self.log)
                .and_then(|f| f.set_len(len))
                .map_err(io),
        }
    }
}

/// `err`, and when taking the edit back failed too, say so: an `Err` from `save_edit` must not
/// hide that part of the edit is still in place. A fleet error stays a fleet error (the app
/// logs those), with the note on it.
fn with_undo_failures(err: CoreError, stuck: Vec<String>) -> CoreError {
    if stuck.is_empty() {
        return err;
    }
    let note = stuck.join("; ");
    match err {
        CoreError::Fleet(mut e) => {
            e.undo_note = Some(note);
            CoreError::Fleet(e)
        }
        other => CoreError::Io(format!(
            "{other}; the edit could not be fully taken back, check: {note}"
        )),
    }
}

/// One JSON line appended to `log`, its parent folders created first. Gives back the way to
/// take the line off again.
fn append_line<'a>(log: &'a std::path::Path, row: &Value) -> Result<LedgerUndo<'a>, CoreError> {
    use std::io::Write;
    let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", log.display()));
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let len = match std::fs::metadata(log) {
        Ok(meta) if meta.is_file() => Some(meta.len()),
        Ok(_) => None,
        Err(_) => None,
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(io)?;
    let line = format!("{row}\n");
    if let Err(e) = file.write_all(line.as_bytes()) {
        let stuck: Vec<String> = LedgerUndo { log, len }.undo().err().into_iter().collect();
        return Err(with_undo_failures(io(e), stuck));
    }
    Ok(LedgerUndo { log, len })
}

/// Write `text` to `path` whole: a sibling temp file, then a rename, so a failed write never
/// leaves a half-written `config.json`.
fn replace_file(path: &std::path::Path, text: &str) -> Result<(), CoreError> {
    let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", path.display()));
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".switchboard-tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, text).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
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
