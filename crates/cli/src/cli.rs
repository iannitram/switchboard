//! CLI-1: `switchboard sky | waiting | reports | budget | loadout | star <repo>#<n>`, as text or
//! `--json`, printing what the GUI shows.
//! CLI-2: `switchboard refresh | backup <repo> | backup --all`, the GUI buttons.
//! CLI-3: a repo delete or a restore over a live repo is refused and becomes a Waiting on you
//! item; `--role <role> merge <repo>#<pr>` goes through CORE-3; inside a job worktree no role
//! gives write actions.
//!
//! The raw `fleet` CLI stays the documented escape hatch: Switchboard keeps no state that only
//! it can read.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{json, Value};
use switchboard_core::budget::{self, PoolRow};
use switchboard_core::clock::{Clock, SystemClock};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::RepoSnapshot;
use switchboard_core::joblog::{self, NOT_RECORDED};
use switchboard_core::platform::{current_os, Os};
use switchboard_core::sky;

/// The environment variable that names the pipeline checkout. There is no default.
pub const PIPELINE_ENV: &str = "SWITCHBOARD_PIPELINE";
/// The environment variable that names the app data folder (the snapshot is in `snapshot/`).
/// Unset, the folder is the desktop app's own (see `data_dir`).
pub const DATA_ENV: &str = "SWITCHBOARD_DATA";
/// The environment variable that names the fleet state folder (its `ledger/` holds the job log).
/// Unset, `FLEET_STATE_DIR`, else `<pipeline>-fleet-state` beside the pipeline checkout.
pub const STATE_ENV: &str = "SWITCHBOARD_STATE";

/// The app data folder's name: the desktop app's identifier.
const APP_DIR: &str = "app.switchboard.desktop";
/// The message when no snapshot has been written yet.
const NO_SNAPSHOT: &str = "switchboard: no snapshot yet; run switchboard refresh";

const USAGE: &str = "usage: switchboard [--json] [--role <role>] \
                     <sky | waiting | reports | budget | loadout | star <repo>#<n> | mcp>";

pub fn run(argv: &[String]) -> ExitCode {
    let args = match super::parse_args(argv) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("switchboard: {e}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match args.command.first().map(String::as_str) {
        Some("budget") => budget(args.json),
        Some("sky") => sky_command(args.json),
        Some("star") => match args.command.get(1..) {
            Some([target]) => star_command(target, args.json),
            _ => {
                eprintln!("switchboard: star needs one <repo>#<n>");
                eprintln!("{USAGE}");
                ExitCode::from(2)
            }
        },
        Some(command @ ("waiting" | "reports")) => {
            eprintln!("switchboard: {command} is not built yet");
            ExitCode::from(2)
        }
        Some("loadout") => {
            eprintln!("switchboard: loadout is not built yet (IT-1: loadout probes on Linux)");
            ExitCode::from(2)
        }
        Some(other) => {
            eprintln!("switchboard: unknown command: {other}");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        None => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// `switchboard budget`: one line per pool (text) or the rows as a JSON array (`--json`).
fn budget(json: bool) -> ExitCode {
    let root = match std::env::var_os(PIPELINE_ENV) {
        Some(root) if !root.is_empty() => root,
        _ => {
            eprintln!("switchboard: set {PIPELINE_ENV} to your pipeline checkout");
            return ExitCode::from(2);
        }
    };
    let rows = match budget::read(&FleetBridge::new(Path::new(&root))) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("switchboard budget: {e}");
            return ExitCode::from(1);
        }
    };
    if json {
        match serde_json::to_string_pretty(&rows) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("switchboard budget: {e}");
                return ExitCode::from(1);
            }
        }
    } else {
        print!("{}", budget_text(&rows));
    }
    ExitCode::SUCCESS
}

/// One line per pool, in row order: `<pool>: <pct>% vs pace <pace>%, <verdict>`.
fn budget_text(rows: &[PoolRow]) -> String {
    let percent = |value: Option<f64>| match value {
        Some(v) => format!("{v}%"),
        None => NOT_RECORDED.to_string(),
    };
    rows.iter()
        .map(|row| {
            format!(
                "{}: {} vs pace {}, {}\n",
                row.pool,
                percent(row.pct),
                percent(row.pace),
                row.verdict
            )
        })
        .collect()
}

/// An environment variable as a path, or `None` when unset or empty.
fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The app data folder: `SWITCHBOARD_DATA`, else the desktop app's folder for this OS.
fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = env_path(DATA_ENV) {
        return Some(dir);
    }
    let home = env_path("HOME");
    match current_os() {
        Os::Windows => env_path("APPDATA").map(|d| d.join(APP_DIR)),
        Os::Mac => home.map(|h| h.join("Library").join("Application Support").join(APP_DIR)),
        Os::Linux => env_path("XDG_DATA_HOME")
            .or_else(|| home.map(|h| h.join(".local").join("share")))
            .map(|d| d.join(APP_DIR)),
    }
}

/// Every stored snapshot, or the exit code after saying why there is none.
fn snapshots(command: &str) -> Result<(PathBuf, Vec<RepoSnapshot>), ExitCode> {
    let Some(data) = data_dir() else {
        eprintln!("switchboard: set {DATA_ENV} to the app data folder");
        return Err(ExitCode::from(2));
    };
    match SnapshotStore::new(&data).load_all() {
        Ok(snaps) if snaps.is_empty() => {
            eprintln!("{NO_SNAPSHOT}");
            Err(ExitCode::from(1))
        }
        Ok(snaps) => Ok((data, snaps)),
        Err(e) => {
            eprintln!("switchboard {command}: {e}");
            Err(ExitCode::from(1))
        }
    }
}

/// Print `value` as pretty JSON.
fn print_json(command: &str, value: &Value) -> ExitCode {
    match serde_json::to_string_pretty(value) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("switchboard {command}: {e}");
            ExitCode::from(1)
        }
    }
}

/// One constellation as `sky` prints it.
struct SkyRow {
    repo: String,
    brightness: f64,
    open: usize,
    drifted: usize,
}

/// `switchboard sky`: one row per repo in the Sky view's placement order, from the same
/// `sky::model` the window paints.
fn sky_command(json: bool) -> ExitCode {
    let (data, snaps) = match snapshots("sky") {
        Ok(found) => found,
        Err(code) => return code,
    };
    let model = match sky::model::load(&data, SystemClock.now()) {
        Ok(model) => model,
        Err(e) => {
            eprintln!("switchboard sky: {e}");
            return ExitCode::from(1);
        }
    };
    let rows: Vec<SkyRow> = model
        .constellations
        .iter()
        .map(|c| SkyRow {
            repo: c.repo.clone(),
            brightness: c.brightness,
            open: snaps
                .iter()
                .find(|s| s.repo == c.repo)
                .map_or(0, |s| s.issues.iter().filter(|i| i.state == "open").count()),
            drifted: c.drift.len(),
        })
        .collect();
    if json {
        let rows: Vec<Value> = rows
            .iter()
            .map(|row| {
                json!({
                    "repo": row.repo,
                    "brightness": row.brightness,
                    "open": row.open,
                    "drifted": row.drifted,
                })
            })
            .collect();
        print_json("sky", &Value::Array(rows))
    } else {
        print!("{}", sky_text(&rows));
        ExitCode::SUCCESS
    }
}

/// One line per repo: `<repo>: brightness <b>, <n> open, <d> drifted`.
fn sky_text(rows: &[SkyRow]) -> String {
    rows.iter()
        .map(|row| {
            format!(
                "{}: brightness {:.2}, {} open, {} drifted\n",
                row.repo, row.brightness, row.open, row.drifted
            )
        })
        .collect()
}

/// The PR that closes a star, as `star` prints it.
struct StarPr {
    number: u64,
    state: String,
    /// The last R25 verdict, `PASS` or `FAIL`.
    review: Option<String>,
    /// The combined check state on the head.
    ci: String,
}

/// One issue as `star` prints it.
struct Star {
    repo: String,
    number: u64,
    title: String,
    state: String,
    labels: Vec<String>,
    size: Option<String>,
    pr: Option<StarPr>,
    /// The fleet job running on it; `None` inside `Some` when the job log holds none, and the
    /// outer `None` when the job log was not read (no pipeline checkout set).
    job: Option<Option<String>>,
}

/// `switchboard star <repo>#<n>`: the facts the Sky view's star card shows. `<repo>` is
/// `owner/name` or the bare name.
fn star_command(target: &str, json: bool) -> ExitCode {
    let Some((repo, number)) = target
        .rsplit_once('#')
        .and_then(|(repo, n)| Some((repo, n.parse::<u64>().ok()?)))
        .filter(|(repo, _)| !repo.is_empty())
    else {
        eprintln!("switchboard: star needs <repo>#<n>, got {target}");
        return ExitCode::from(2);
    };
    let (_, snaps) = match snapshots("star") {
        Ok(found) => found,
        Err(code) => return code,
    };
    let suffix = format!("/{repo}");
    let snap = snaps
        .iter()
        .find(|s| s.repo == repo)
        .or_else(|| snaps.iter().find(|s| s.repo.ends_with(suffix.as_str())));
    let Some((snap, issue)) =
        snap.and_then(|s| Some((s, s.issues.iter().find(|i| i.number == number)?)))
    else {
        eprintln!("switchboard: no such star: {target}");
        return ExitCode::from(1);
    };
    let job = match running_job(&snap.repo, number) {
        Ok(job) => job,
        Err(e) => {
            eprintln!("switchboard star: {e}");
            return ExitCode::from(1);
        }
    };
    let star = Star {
        repo: snap.repo.clone(),
        number,
        title: issue.title.clone(),
        state: issue.state.clone(),
        labels: issue.labels.clone(),
        size: issue
            .labels
            .iter()
            .find_map(|label| label.strip_prefix("size:"))
            .map(str::to_string),
        pr: snap
            .pulls
            .iter()
            .find(|pr| pr.closes.contains(&number))
            .map(|pr| StarPr {
                number: pr.number,
                state: pr.state.clone(),
                review: pr
                    .verdicts
                    .last()
                    .filter(|v| matches!(v.verdict.as_str(), "PASS" | "FAIL"))
                    .map(|v| v.verdict.clone()),
                ci: pr.ci_on_head.clone(),
            }),
        job,
    };
    if json {
        let job = match &star.job {
            Some(job) => json!(job),
            None => json!(NOT_RECORDED),
        };
        print_json(
            "star",
            &json!({
                "repo": star.repo,
                "number": star.number,
                "title": star.title,
                "state": star.state,
                "labels": star.labels,
                "size": star.size,
                "pr": star.pr.as_ref().map(|pr| json!({
                    "number": pr.number,
                    "state": pr.state,
                    "review": pr.review,
                    "ci": pr.ci,
                })),
                "job": job,
            }),
        )
    } else {
        print!("{}", star_text(&star));
        ExitCode::SUCCESS
    }
}

/// One fact per line, the issue first.
fn star_text(star: &Star) -> String {
    let or_none = |value: Option<&str>| value.unwrap_or("none").to_string();
    let labels = if star.labels.is_empty() {
        "none".to_string()
    } else {
        star.labels.join(", ")
    };
    let (pr, review, ci) = match &star.pr {
        Some(pr) => (
            format!("#{} {}", pr.number, pr.state),
            or_none(pr.review.as_deref()),
            pr.ci.clone(),
        ),
        None => ("none".to_string(), "none".to_string(), "none".to_string()),
    };
    let job = match &star.job {
        Some(job) => or_none(job.as_deref()),
        None => NOT_RECORDED.to_string(),
    };
    format!(
        "{}#{}: {}\nstate: {}\nlabels: {labels}\nsize: {}\npr: {pr}\nreview: {review}\nci: {ci}\njob: {job}\n",
        star.repo,
        star.number,
        star.title,
        star.state,
        or_none(star.size.as_deref()),
    )
}

/// The fleet job running on `repo#number`: the last ledger `start` event for it with no `end`
/// after it, as `<lane> <model> on <machine>, attempt <n>`. `Ok(None)` when the pipeline
/// checkout is not set, so the job log cannot be read.
fn running_job(repo: &str, number: u64) -> Result<Option<Option<String>>, String> {
    let Some(pipeline) = env_path(PIPELINE_ENV) else {
        return Ok(None);
    };
    let state = env_path(STATE_ENV)
        .or_else(|| env_path("FLEET_STATE_DIR"))
        .unwrap_or_else(|| {
            let mut name = pipeline.file_name().unwrap_or_default().to_os_string();
            name.push("-fleet-state");
            pipeline.with_file_name(name)
        });
    let events =
        joblog::read_ledger(&joblog::ledger_files(&pipeline, &state)).map_err(|e| e.to_string())?;
    let short = repo.rsplit('/').next().unwrap_or(repo);
    let ids = [format!("{repo}#{number}"), format!("{short}#{number}")];
    let field = |event: &Value, key: &str| -> Option<String> {
        match event.get(key)? {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    };
    let mut running: Option<&Value> = None;
    for event in &events {
        if !field(event, "id").is_some_and(|id| ids.contains(&id)) {
            continue;
        }
        match event.get("event").and_then(Value::as_str) {
            Some("start") => running = Some(event),
            Some("end") => {
                let same_machine = running.is_some_and(|start| {
                    field(start, "machine").is_none()
                        || field(event, "machine").is_none()
                        || field(start, "machine") == field(event, "machine")
                });
                if same_machine {
                    running = None;
                }
            }
            _ => {}
        }
    }
    Ok(Some(running.map(|start| {
        let mut text = [field(start, "lane"), field(start, "model")]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            text = "a job".to_string();
        }
        if let Some(machine) = field(start, "machine") {
            text.push_str(&format!(" on {machine}"));
        }
        if let Some(attempt) = field(start, "attempt") {
            text.push_str(&format!(", attempt {attempt}"));
        }
        text
    })))
}
