//! CORE-4: the job log. One row per attempt, from the fleet ledger and from the local chat
//! rows (FD-13), in one shape. A field a row lacks is `None` and shows as "not recorded",
//! never a guess.

use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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

/// `value` with every object's keys in sorted order, so two events that differ only in key
/// order compare the same as text (serde_json keeps insertion order with `preserve_order`).
fn key_sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k.clone(), key_sorted(v)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(key_sorted).collect()),
        other => other.clone(),
    }
}

/// The text a view shows for a field a row lacks.
pub const NOT_RECORDED: &str = "not recorded";

/// Turn one ledger event (a JSON object) into a row. Unknown fields are ignored; missing ones
/// stay `None`.
pub fn row_from_ledger(event: &Value) -> JobRow {
    let Some(obj) = event.as_object() else {
        return JobRow::default();
    };
    // A string field: a JSON string, or a JSON number as its text. Anything else is unset.
    let text = |key: &str| -> Option<String> {
        match obj.get(key)? {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    };
    // A whole, non-negative number: an integer, or a float with no fraction.
    let whole = |key: &str| -> Option<u64> {
        let v = obj.get(key)?;
        v.as_u64().or_else(|| {
            v.as_f64()
                .filter(|f| *f >= 0.0 && f.fract() == 0.0 && *f < 18_446_744_073_709_551_616.0)
                .map(|f| f as u64)
        })
    };
    let small = |key: &str| -> Option<u32> { whole(key).and_then(|n| u32::try_from(n).ok()) };
    let real = |key: &str| -> Option<f64> { obj.get(key)?.as_f64() };
    // Any JSON value except null, copied as it is.
    let any = |key: &str| -> Option<Value> { obj.get(key).filter(|v| !v.is_null()).cloned() };

    JobRow {
        id: text("id"),
        kind: text("kind"),
        size: text("size"),
        lane: text("lane"),
        model: text("model"),
        effort: text("effort"),
        tokens: whole("tokens"),
        tokens_in: whole("tokens_in"),
        tokens_out: whole("tokens_out"),
        cost_usd: real("cost_usd"),
        seconds: real("seconds"),
        tools: any("tools"),
        loc_changed: whole("loc_changed"),
        files_changed: whole("files_changed"),
        outcome: text("outcome"),
        failure_mode: text("failure_mode"),
        failure_class: text("failure_class"),
        attempt: small("attempt"),
        rounds: small("rounds"),
        rating: text("rating"),
        handoff_id: text("handoff_id"),
        checks: any("checks"),
        escalation_step: text("escalation_step"),
        review_fix_loc: whole("review_fix_loc"),
        source: RowSource::Ledger,
        implicit: false,
    }
}

/// The ledger event's `event` name, if it has one.
fn event_name(event: &Value) -> Option<&str> {
    event.get("event")?.as_str()
}

/// Read every attempt row: the ledger's `end` events, then the chat rows, oldest first.
/// Each `rating` event sets the rating of the last `end` row with its id (and its attempt,
/// when it names one), in slice order. Neither input is changed.
pub fn read_all(ledger_events: &[Value], chat_rows: &[JobRow]) -> Vec<JobRow> {
    let mut rows: Vec<JobRow> = ledger_events
        .iter()
        .filter(|e| event_name(e) == Some("end"))
        .map(row_from_ledger)
        .collect();

    for event in ledger_events
        .iter()
        .filter(|e| event_name(e) == Some("rating"))
    {
        let parsed = row_from_ledger(event);
        let Some(rating) = parsed.rating else {
            continue;
        };
        let names_attempt = event.get("attempt").is_some_and(|v| !v.is_null());
        let target = rows.iter_mut().rev().find(|row| {
            parsed.id.is_some()
                && row.id == parsed.id
                && (!names_attempt || (parsed.attempt.is_some() && row.attempt == parsed.attempt))
        });
        if let Some(row) = target {
            row.rating = Some(rating);
        }
    }

    rows.extend(chat_rows.iter().map(|row| JobRow {
        source: RowSource::Chat,
        ..row.clone()
    }));
    rows
}

/// Every JSON object line of a JSONL text, in order. Blank lines, lines that do not parse and
/// a trailing `\r` are skipped or trimmed.
pub fn parse_jsonl(text: &str) -> Vec<Value> {
    text.lines()
        .filter_map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            serde_json::from_str::<Value>(line).ok()
        })
        .collect()
}

/// The ledger files to read: the legacy `pipeline_root/logs/jobs.jsonl` first (if it is a file),
/// then each `*.jsonl` file in `state_dir/ledger/`, sorted by path. A missing folder gives none.
pub fn ledger_files(pipeline_root: &Path, state_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let legacy = pipeline_root.join("logs").join("jobs.jsonl");
    if legacy.is_file() {
        files.push(legacy);
    }
    let mut machines: Vec<PathBuf> = std::fs::read_dir(state_dir.join("ledger"))
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".jsonl"))
        })
        .collect();
    machines.sort();
    files.extend(machines);
    files
}

/// Read and parse the ledger files in order. A file that does not exist is skipped; any other
/// read error is `CoreError::Io`. An event equal to one already kept is dropped.
pub fn read_ledger(files: &[PathBuf]) -> Result<Vec<Value>, CoreError> {
    let mut kept: Vec<Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for path in files {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
        };
        for event in parse_jsonl(&text) {
            if seen.insert(key_sorted(&event).to_string()) {
                kept.push(event);
            }
        }
    }
    Ok(kept)
}

/// The text a view shows for a field: the value, or `NOT_RECORDED` when the row lacks it.
pub fn shown<T: std::fmt::Display>(field: &Option<T>) -> String {
    match field {
        Some(value) => value.to_string(),
        None => NOT_RECORDED.to_string(),
    }
}
