//! `reports`: a fixed report for every finished job and every chat handoff, in BroCrew's
//! fixed-report shape plus the line our job cards lack ("what wasn't checked").

use crate::clock::UnixSeconds;
use crate::error::CoreError;
use crate::fleet::FleetBridge;
use crate::joblog::{ledger_files, read_all, read_ledger, JobRow, NOT_RECORDED};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub id: String,
    pub title: Option<String>,
    pub project: Option<String>,
    /// When the job ended, unix seconds UTC.
    pub at: Option<UnixSeconds>,
    /// REP-1 part 1: outcome first.
    pub outcome: String,
    pub failure_mode: Option<String>,
    /// REP-1 part 2: PR, files and lines changed.
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

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Changed {
    pub pr: Option<String>,
    pub files: Option<u64>,
    /// The ledger's `loc_changed`: a total, not split into added and removed.
    pub lines: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Cost {
    pub pool: Option<String>,
    pub tokens: Option<u64>,
    pub usd: Option<f64>,
    pub seconds: Option<f64>,
}

/// REP-2: one move of a chat or job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Move {
    pub from_service: String,
    pub from_model: String,
    pub to_service: String,
    pub to_model: String,
    pub why: String,
}

/// The line a report shows when a job ran no checks.
pub const NO_CHECKS: &str = "no checks were recorded";
/// The line a chat's report shows under "not checked".
pub const CHAT_NOT_CHECKED: &str = "a chat runs no build, test or lint";

/// A non-empty JSON string field, or `None` for a missing key, `null`, `""` or any other type.
fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// owner/repo from a GitHub pull request URL, or `None` for any other URL.
pub fn project_of(pr_url: &str) -> Option<String> {
    let rest = pr_url.strip_prefix("https://github.com/")?;
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() >= 3 && !parts[0].is_empty() && !parts[1].is_empty() && parts[2] == "pull" {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

/// Days from 1970-01-01 to a civil date (the days-from-civil formula, proleptic Gregorian).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12; // March = 0
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The card's ISO time as unix seconds UTC. Only `YYYY-MM-DDTHH:MM:SS`, an optional `.` with
/// digits, then `Z` or `+HH:MM` / `-HH:MM`. Anything else is `None`.
pub fn parse_time(t: &str) -> Option<UnixSeconds> {
    let b = t.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    // A fixed-width field of ASCII digits, as a number.
    let field = |range: std::ops::Range<usize>| -> Option<i64> {
        let digits = &b[range];
        if digits.iter().all(u8::is_ascii_digit) {
            Some(digits.iter().fold(0, |n, d| n * 10 + i64::from(d - b'0')))
        } else {
            None
        }
    };
    let year = field(0..4)?;
    let month = field(5..7)?;
    let day = field(8..10)?;
    let hour = field(11..13)?;
    let minute = field(14..16)?;
    let second = field(17..19)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    // Optional fraction of seconds: a `.` and one or more digits.
    let mut i = 19;
    if b[i] == b'.' {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
    }

    // The offset from UTC, in seconds (east positive).
    let offset = match &b[i..] {
        [b'Z'] => 0,
        [sign, h1, h2, b':', m1, m2]
            if matches!(sign, b'+' | b'-')
                && [h1, h2, m1, m2].iter().all(|c| c.is_ascii_digit()) =>
        {
            let secs = i64::from(h1 - b'0') * 36_000
                + i64::from(h2 - b'0') * 3_600
                + i64::from(m1 - b'0') * 600
                + i64::from(m2 - b'0') * 60;
            if *sign == b'-' {
                -secs
            } else {
                secs
            }
        }
        _ => return None,
    };

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second - offset)
}

/// The sum of the values, or `None` when there are none.
fn total<T: std::iter::Sum<T>>(values: Vec<T>) -> Option<T> {
    if values.is_empty() {
        None
    } else {
        Some(values.into_iter().sum())
    }
}

/// REP-1: a report from a fleet done card (JSON) and its job-log rows.
pub fn from_card(card: &Value, rows: &[JobRow]) -> Report {
    let id = text(card, "id");
    // The card's row: the last row with the card's id.
    let row = id.as_ref().and_then(|id| {
        rows.iter()
            .rev()
            .find(|r| r.id.as_deref() == Some(id.as_str()))
    });

    // The rows of this job only; a card without an id has none.
    let own: Vec<JobRow> = match &id {
        Some(id) => rows
            .iter()
            .filter(|r| r.id.as_deref() == Some(id.as_str()))
            .cloned()
            .collect(),
        None => Vec::new(),
    };

    let checks: Vec<&Value> = card
        .get("checks")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |a| a.iter().filter(|c| c.is_object()).collect());
    let mut checked = Vec::new();
    let mut not_checked = Vec::new();
    for c in &checks {
        let name = text(c, "name").unwrap_or_else(|| NOT_RECORDED.to_string());
        let status = text(c, "status").unwrap_or_else(|| NOT_RECORDED.to_string());
        let line = match text(c, "note") {
            Some(note) => format!("{name}: {status} ({note})"),
            None => format!("{name}: {status}"),
        };
        if matches!(status.as_str(), "passed" | "failed" | "warn") {
            checked.push(line);
        } else {
            not_checked.push(line);
        }
    }
    if checks.is_empty() {
        not_checked.push(NO_CHECKS.to_string());
    }

    let project =
        text(card, "repo").or_else(|| text(card, "pr_url").and_then(|url| project_of(&url)));

    Report {
        id: id.unwrap_or_else(|| NOT_RECORDED.to_string()),
        title: text(card, "title"),
        project,
        at: text(card, "at").and_then(|t| parse_time(&t)),
        outcome: text(card, "outcome").unwrap_or_else(|| NOT_RECORDED.to_string()),
        failure_mode: text(card, "failure_mode"),
        changed: Changed {
            pr: text(card, "pr_url"),
            files: row.and_then(|r| r.files_changed),
            lines: row.and_then(|r| r.loc_changed),
        },
        checked,
        not_checked,
        cost: Cost {
            pool: text(card, "lane").or_else(|| row.and_then(|r| r.lane.clone())),
            tokens: card
                .get("tokens")
                .and_then(Value::as_u64)
                .or_else(|| row.and_then(|r| r.tokens)),
            usd: card
                .get("cost_usd")
                .and_then(Value::as_f64)
                .or_else(|| row.and_then(|r| r.cost_usd)),
            seconds: card
                .get("seconds")
                .and_then(Value::as_f64)
                .or_else(|| row.and_then(|r| r.seconds)),
        },
        moves: moves(&own),
    }
}

/// REP-1: a report from a chat thread's rows (FD-13).
pub fn from_thread(thread_id: &str, rows: &[JobRow]) -> Report {
    let kept: Vec<&JobRow> = rows.iter().filter(|r| !r.implicit).collect();
    let last = kept.last().copied();

    Report {
        id: thread_id.to_string(),
        title: None,
        project: None,
        at: None,
        outcome: last
            .and_then(|r| r.outcome.clone())
            .unwrap_or_else(|| NOT_RECORDED.to_string()),
        failure_mode: last.and_then(|r| r.failure_mode.clone()),
        changed: Changed::default(),
        checked: Vec::new(),
        not_checked: vec![CHAT_NOT_CHECKED.to_string()],
        cost: Cost {
            pool: last.and_then(|r| r.lane.clone()),
            tokens: total(kept.iter().filter_map(|r| r.tokens).collect()),
            usd: total(kept.iter().filter_map(|r| r.cost_usd).collect()),
            seconds: total(kept.iter().filter_map(|r| r.seconds).collect()),
        },
        moves: moves(rows),
    }
}

/// REP-2: the moves of a thread, oldest first: one per change of service or model between
/// consecutive rows (implicit rows left out), with the earlier row's outcome as the `why`.
pub fn moves(rows: &[JobRow]) -> Vec<Move> {
    let kept: Vec<&JobRow> = rows.iter().filter(|r| !r.implicit).collect();
    let place = |r: &JobRow| (r.lane.clone(), r.model.clone());
    kept.windows(2)
        .filter(|w| place(w[0]) != place(w[1]))
        .map(|w| {
            let (from, to) = (w[0], w[1]);
            let or_not = |v: &Option<String>| v.clone().unwrap_or_else(|| NOT_RECORDED.to_string());
            let why = match (&from.outcome, &from.failure_mode) {
                (Some(o), Some(m)) => format!("{o} ({m})"),
                (Some(o), None) => o.clone(),
                (None, Some(m)) => m.clone(),
                (None, None) => NOT_RECORDED.to_string(),
            };
            Move {
                from_service: or_not(&from.lane),
                from_model: or_not(&from.model),
                to_service: or_not(&to.lane),
                to_model: or_not(&to.model),
                why,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    pub project: Option<String>,
    pub outcome: Option<String>,
    pub pool: Option<String>,
}

/// REP-3: the reports that match every set field of the filter, order kept.
pub fn filter<'a>(reports: &'a [Report], f: &Filter) -> Vec<&'a Report> {
    reports
        .iter()
        .filter(|r| {
            (f.project.is_none() || r.project == f.project)
                && (f.outcome.is_none() || f.outcome.as_ref() == Some(&r.outcome))
                && (f.pool.is_none() || r.cost.pool == f.pool)
        })
        .collect()
}

/// The reports with a time first, newest first; the reports without one after them, in the
/// order given. The input is not changed.
pub fn newest_first(reports: &[Report]) -> Vec<Report> {
    let mut sorted = reports.to_vec();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.at));
    sorted
}

/// The env var that names the fleet-state folder outright (the rule `fleet` uses too).
pub const FLEET_STATE_DIR_ENV: &str = "FLEET_STATE_DIR";

/// Where the fleet state is: `env` when it is set and not empty; else the sibling folder
/// `<root>-fleet-state` when that is a checkout or the root is not one; else the first checkout
/// in `worktrees` (the text of `git worktree list --porcelain`) on `fleet-state` that exists;
/// else the sibling folder.
pub fn state_dir_from(pipeline_root: &Path, env: Option<&OsStr>, worktrees: &str) -> PathBuf {
    if let Some(dir) = env.filter(|e| !e.is_empty()) {
        return PathBuf::from(dir);
    }
    let name = pipeline_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let default = pipeline_root.with_file_name(format!("{name}-fleet-state"));
    if default.join(".git").exists() || !pipeline_root.join(".git").exists() {
        return default;
    }
    let mut current: Option<&str> = None;
    for line in worktrees.lines() {
        let line = line.trim();
        if line.is_empty() {
            current = None;
        } else if let Some(path) = line.strip_prefix("worktree ") {
            current = Some(path.trim());
        } else if line == "branch refs/heads/fleet-state" {
            if let Some(path) = current.map(PathBuf::from).filter(|p| p.is_dir()) {
                return path;
            }
        }
    }
    default
}

/// The done cards in `<state_dir>/jobs/done/`: the `.json` files whose names do not end
/// `.job.json`, by path, each kept when it parses to a JSON object. A missing folder is no
/// cards; any other folder or file error is `CoreError::Io`.
pub fn read_done_cards(state_dir: &Path) -> Result<Vec<Value>, CoreError> {
    let done = state_dir.join("jobs").join("done");
    let entries = match std::fs::read_dir(&done) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(CoreError::Io(format!("{}: {e}", done.display()))),
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| CoreError::Io(format!("{}: {e}", done.display())))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_file() && name.ends_with(".json") && !name.ends_with(".job.json") {
            paths.push(path);
        }
    }
    paths.sort();

    let mut cards = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| CoreError::Io(format!("{}: {e}", path.display())))?;
        if let Some(card) = serde_json::from_str::<Value>(&text)
            .ok()
            .filter(Value::is_object)
        {
            cards.push(card);
        }
    }
    Ok(cards)
}

/// The reports for the done cards, then the threads, newest first (the ones without a time
/// after, in that order), narrowed by `f` when it is given. The inputs are not changed.
pub fn assemble(
    cards: &[Value],
    ledger_events: &[Value],
    threads: &[(String, Vec<JobRow>)],
    f: Option<&Filter>,
) -> Vec<Report> {
    let rows = read_all(ledger_events, &[]);
    let mut reports: Vec<Report> = cards.iter().map(|c| from_card(c, &rows)).collect();
    reports.extend(threads.iter().map(|(id, rows)| from_thread(id, rows)));
    let sorted = newest_first(&reports);
    match f {
        Some(f) => filter(&sorted, f).into_iter().cloned().collect(),
        None => sorted,
    }
}

/// The reports on disk: the done cards in the fleet-state folder and the ledger rows, with
/// the given threads, as `assemble` builds them. `git worktree list` failing counts as none.
pub fn load(
    fleet: &FleetBridge,
    env: Option<&OsStr>,
    threads: &[(String, Vec<JobRow>)],
    f: Option<&Filter>,
) -> Result<Vec<Report>, CoreError> {
    let root = fleet.root();
    let worktrees = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "list", "--porcelain"])
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let state = state_dir_from(root, env, &worktrees);
    let cards = read_done_cards(&state)?;
    let events = read_ledger(&ledger_files(root, &state))?;
    Ok(assemble(&cards, &events, threads, f))
}
