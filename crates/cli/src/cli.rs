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
use switchboard_core::fleet::FleetBridge;
use switchboard_core::joblog::NOT_RECORDED;

const USAGE: &str =
    "usage: switchboard [--json] [--role <role>] sky | waiting | reports | budget | loadout | star <repo>#<n> | mcp";

/// The pipeline repo: `SWITCHBOARD_PIPELINE`, else `~/dev/potn-dev`.
fn pipeline_root() -> PathBuf {
    if let Some(root) = std::env::var_os("SWITCHBOARD_PIPELINE").filter(|v| !v.is_empty()) {
        return PathBuf::from(root);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .unwrap_or_default();
    PathBuf::from(home).join("dev").join("potn-dev")
}

fn not_built(command: &str) -> ExitCode {
    eprintln!("switchboard: {command} is not built yet");
    ExitCode::from(2)
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

pub fn run(argv: &[String]) -> ExitCode {
    let args = match crate::parse_args(argv) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("switchboard: {e}");
            return usage();
        }
    };
    match args.command.first().map(String::as_str) {
        Some("budget") => budget(args.json),
        Some("loadout") => {
            eprintln!("switchboard: loadout is not built yet (IT-1: loadout probes on Linux)");
            ExitCode::from(2)
        }
        Some(command @ ("sky" | "waiting" | "reports" | "star")) => not_built(command),
        _ => usage(),
    }
}

/// CLI-1: what the Budget panel shows, one line per pool, or the rows as a JSON array.
fn budget(json: bool) -> ExitCode {
    let fleet = FleetBridge::new(&pipeline_root());
    let rows = match budget::read(&fleet) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("switchboard budget: {e}");
            return ExitCode::from(1);
        }
    };
    if json {
        match serde_json::to_string(&rows) {
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

/// `<pool>: <pct>% vs pace <pace>%, <verdict>`, one line per row, `not recorded` for a missing
/// number.
fn budget_text(rows: &[PoolRow]) -> String {
    let number = |n: Option<f64>| match n {
        Some(n) => format!("{n}%"),
        None => NOT_RECORDED.to_string(),
    };
    rows.iter()
        .map(|row| {
            format!(
                "{}: {} vs pace {}, {}\n",
                row.pool,
                number(row.pct),
                number(row.pace),
                row.verdict
            )
        })
        .collect()
}
