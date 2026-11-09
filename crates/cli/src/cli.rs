//! CLI-1: `switchboard sky | waiting | reports | budget | loadout | star <repo>#<n>`, as text or
//! `--json`, printing what the GUI shows.
//! CLI-2: `switchboard refresh | backup <repo> | backup --all`, the GUI buttons.
//! CLI-3: a repo delete or a restore over a live repo is refused and becomes a Waiting on you
//! item; `--role <role> merge <repo>#<pr>` goes through CORE-3; inside a job worktree no role
//! gives write actions.
//!
//! The raw `fleet` CLI stays the documented escape hatch: Switchboard keeps no state that only
//! it can read.

use std::path::Path;
use std::process::ExitCode;

use switchboard_core::budget::{self, PoolRow};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::joblog::NOT_RECORDED;

/// The environment variable that names the pipeline checkout. There is no default.
pub const PIPELINE_ENV: &str = "SWITCHBOARD_PIPELINE";

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
        Some(command @ ("sky" | "waiting" | "reports" | "star")) => {
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
