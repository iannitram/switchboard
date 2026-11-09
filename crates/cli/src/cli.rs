//! CLI-1: `switchboard sky | waiting | reports | budget | loadout | star <repo>#<n>`, as text or
//! `--json`, printing what the GUI shows.
//! CLI-2: `switchboard refresh | backup <repo> | backup --all`, the GUI buttons.
//! CLI-3: a repo delete or a restore over a live repo is refused and becomes a Waiting on you
//! item; `--role <role> merge <repo>#<pr>` goes through CORE-3; inside a job worktree no role
//! gives write actions.
//!
//! The raw `fleet` CLI stays the documented escape hatch: Switchboard keeps no state that only
//! it can read.

use std::path::PathBuf;
use std::process::ExitCode;

use switchboard_core::budget::{self, PoolRow};
use switchboard_core::error::CoreError;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::joblog::{self, NOT_RECORDED};
use switchboard_core::platform::{current_os, Os};
use switchboard_core::reports::{self, Filter, Report};
use switchboard_core::waiting;

/// The environment variable that names the pipeline checkout. There is no default.
pub const PIPELINE_ENV: &str = "SWITCHBOARD_PIPELINE";
/// The environment variable that names the app data folder (the snapshot is in `snapshot/`).
/// Unset, the folder is the desktop app's own (see `data_dir`).
pub const DATA_ENV: &str = "SWITCHBOARD_DATA";
/// The environment variable that names the fleet state folder (its `ledger/` holds the job log).
/// Unset, the app data folder is used.
pub const STATE_ENV: &str = "SWITCHBOARD_STATE";

/// The app data folder's name: the desktop app's identifier.
const APP_DIR: &str = "app.switchboard.desktop";
/// The line `waiting` and `reports` print when nothing is waiting.
const NOTHING_WAITING: &str = "nothing waiting on you";
/// The line `reports` prints when there are no reports.
const NO_REPORTS: &str = "no reports";
/// The message when no snapshot has been written yet.
const NO_SNAPSHOT: &str = "switchboard: no snapshot yet; run switchboard refresh";

const USAGE: &str = "usage: switchboard [--json] [--role <role>] \
                     [--project <repo>] [--outcome <o>] [--pool <p>] \
                     <sky | waiting | reports | budget | loadout | star <repo>#<n> | mcp>";

pub fn run(argv: &[String]) -> ExitCode {
    let (filter, argv) = match take_filter(argv) {
        Ok(taken) => taken,
        Err(e) => {
            eprintln!("switchboard: {e}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let args = match super::parse_args(&argv) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("switchboard: {e}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let command = args.command.first().map(String::as_str);
    if command != Some("reports") && filter != Filter::default() {
        eprintln!("switchboard: --project, --outcome and --pool are for reports");
        return ExitCode::from(2);
    }
    match command {
        Some("budget") => budget(args.json),
        Some("waiting") => waiting(args.json),
        Some("reports") => reports(args.json, &filter),
        Some(command @ ("sky" | "star")) => {
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

/// The REP-3 filter flags (`--project <repo>`, `--outcome <o>`, `--pool <p>`) taken out of
/// `argv`, with the other words kept in order. `parse_args` knows only `--json` and `--role`.
fn take_filter(argv: &[String]) -> Result<(Filter, Vec<String>), String> {
    let mut filter = Filter::default();
    let mut rest = Vec::new();
    let mut words = argv.iter();
    while let Some(word) = words.next() {
        let slot = match word.as_str() {
            "--project" => &mut filter.project,
            "--outcome" => &mut filter.outcome,
            "--pool" => &mut filter.pool,
            _ => {
                rest.push(word.clone());
                continue;
            }
        };
        match words.next() {
            Some(value) if !value.starts_with("--") => *slot = Some(value.clone()),
            _ => return Err(format!("{word} needs a value")),
        }
    }
    Ok((filter, rest))
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

/// The pipeline checkout, or the exit code after saying it is not set.
fn pipeline_root() -> Result<PathBuf, ExitCode> {
    env_path(PIPELINE_ENV).ok_or_else(|| {
        eprintln!("switchboard: set {PIPELINE_ENV} to your pipeline checkout");
        ExitCode::from(2)
    })
}

/// `switchboard budget`: one line per pool (text) or the rows as a JSON array (`--json`).
fn budget(json: bool) -> ExitCode {
    let root = match pipeline_root() {
        Ok(root) => root,
        Err(code) => return code,
    };
    let rows = match budget::read(&FleetBridge::new(&root)) {
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

/// `switchboard waiting`: the Waiting on you items from the stored snapshots, newest first:
/// one line each (text) or the items as a JSON array (`--json`). CalDAV is not read yet (U7).
fn waiting(json: bool) -> ExitCode {
    let Some(data) = data_dir() else {
        eprintln!("switchboard: set {DATA_ENV} to the app data folder");
        return ExitCode::from(2);
    };
    let snaps = match SnapshotStore::new(&data).load_all() {
        Ok(snaps) => snaps,
        Err(e) => {
            eprintln!("switchboard waiting: {e}");
            return ExitCode::from(1);
        }
    };
    if snaps.is_empty() {
        eprintln!("{NO_SNAPSHOT}");
        return ExitCode::from(1);
    }
    let items = waiting::merge(vec![waiting::from_snapshots(&snaps)]);
    if json {
        return print_json("waiting", serde_json::to_string_pretty(&items));
    }
    print!("{}", waiting_text(&items));
    ExitCode::SUCCESS
}

/// One line per item, in list order: `<what> | <why> | <action> | <link>`; no items gives the
/// nothing-waiting line.
fn waiting_text(items: &[waiting::Item]) -> String {
    if items.is_empty() {
        return format!("{NOTHING_WAITING}\n");
    }
    items
        .iter()
        .map(|item| {
            format!(
                "{} | {} | {} | {}\n",
                item.what, item.why, item.action, item.link
            )
        })
        .collect()
}

/// `switchboard reports`: the reports from the fleet's done cards and the job log, newest first,
/// narrowed by the filter flags: one line each (text) or the reports as a JSON array (`--json`).
fn reports(json: bool, filter: &Filter) -> ExitCode {
    let root = match pipeline_root() {
        Ok(root) => root,
        Err(code) => return code,
    };
    let fleet = FleetBridge::new(&root);
    let cards = match fleet.run_json(&["cards"]).map_err(CoreError::Fleet) {
        Ok(cards) => cards,
        Err(e) => {
            eprintln!("switchboard reports: {e}");
            return ExitCode::from(1);
        }
    };
    let Some(cards) = cards.as_array() else {
        eprintln!("switchboard reports: fleet cards is not a JSON array");
        return ExitCode::from(1);
    };
    let state = env_path(STATE_ENV)
        .or_else(data_dir)
        .unwrap_or_else(|| root.clone());
    let events = match joblog::read_ledger(&joblog::ledger_files(&root, &state)) {
        Ok(events) => events,
        Err(e) => {
            eprintln!("switchboard reports: {e}");
            return ExitCode::from(1);
        }
    };
    let rows = joblog::read_all(&events, &[]);
    let all: Vec<Report> = cards
        .iter()
        .map(|card| reports::from_card(card, &rows))
        .collect();
    let sorted = reports::newest_first(&all);
    let shown = reports::filter(&sorted, filter);
    if json {
        return print_json("reports", serde_json::to_string_pretty(&shown));
    }
    print!("{}", reports_text(&shown));
    ExitCode::SUCCESS
}

/// One line per report, in list order: `<title> | <outcome> | <project>`; no reports gives the
/// no-reports line.
fn reports_text(reports: &[&reports::Report]) -> String {
    if reports.is_empty() {
        return format!("{NO_REPORTS}\n");
    }
    reports
        .iter()
        .map(|report| {
            format!(
                "{} | {} | {}\n",
                joblog::shown(&report.title),
                report.outcome,
                joblog::shown(&report.project)
            )
        })
        .collect()
}

/// Print the JSON on stdout; a failure is named by `command` on stderr, exit 1.
fn print_json(command: &str, json: serde_json::Result<String>) -> ExitCode {
    match json {
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
