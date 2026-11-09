//! CLI-1: `switchboard sky | waiting | reports | budget | loadout | star <repo>#<n>`, as text or
//! `--json`, printing what the GUI shows.
//! CLI-2: `switchboard refresh | backup <repo> | backup --all`, the GUI buttons.
//! CLI-3: a repo delete or a restore over a live repo is refused and becomes a Waiting on you
//! item; `--role <role> merge <repo>#<pr>` goes through CORE-3; inside a job worktree no role
//! gives write actions.
//!
//! The raw `fleet` CLI stays the documented escape hatch: Switchboard keeps no state that only
//! it can read.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use switchboard_core::budget::{self, PoolRow};
use switchboard_core::clock::{Clock, SystemClock, UnixSeconds};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::joblog::NOT_RECORDED;
use switchboard_core::rules::{self, Caller, Face, GuardedAction, Role, Verdict};
use switchboard_core::waiting::{guarded_item, Item};

/// The environment variable that names the pipeline checkout. There is no default.
pub const PIPELINE_ENV: &str = "SWITCHBOARD_PIPELINE";

const USAGE: &str = "usage: switchboard [--json] [--role <role>] \
                     <sky | waiting | reports | budget | loadout | star <repo>#<n> | \
                     delete <repo> | restore <repo> | merge <repo>#<pr> | mcp>";

/// The writes a CLI command can reach (CLI-3). The real ones are `CliWrites`; the tests pass
/// fakes.
pub trait Writes {
    /// GH-5: merge only with a PASS on the head and CI green, through `merge_pr.py`. An error
    /// is the refusal text.
    fn merge(&self, repo: &str, pr: u64) -> Result<(), String>;
    /// WAIT-1: add one Waiting on you item.
    fn waiting(&self, item: &Item) -> Result<(), String>;
}

/// The real writes. Neither is wired to its core call yet, so each answers with the issue that
/// builds it, as `switchboard mcp` does.
struct CliWrites;

impl Writes for CliWrites {
    fn merge(&self, repo: &str, pr: u64) -> Result<(), String> {
        Err(format!(
            "merge {repo}#{pr}: not built yet (GH-5: the CLI has no PR snapshot)"
        ))
    }

    fn waiting(&self, item: &Item) -> Result<(), String> {
        Err(format!(
            "waiting: not built yet (WAIT-1, #50): {}",
            item.what
        ))
    }
}

pub fn run(argv: &[String]) -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        // Cannot tell whether this is a job worktree: fail closed, no writes.
        Err(e) => {
            eprintln!("switchboard: {e}");
            return ExitCode::from(1);
        }
    };
    run_with(
        argv,
        &cwd,
        SystemClock.now(),
        &CliWrites,
        &mut std::io::stderr(),
    )
}

/// `run` with its seams passed in: the working directory, the time, the writes and stderr.
pub fn run_with(
    argv: &[String],
    cwd: &Path,
    now: UnixSeconds,
    writes: &dyn Writes,
    err: &mut dyn Write,
) -> ExitCode {
    let args = match super::parse_args(argv) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("switchboard: {e}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if let Some(role) = &args.role {
        if Role::parse(role).is_none() {
            let _ = writeln!(err, "switchboard: unknown role: {role}");
            return ExitCode::from(2);
        }
    }
    match args.command.first().map(String::as_str) {
        Some("delete" | "restore" | "merge") => write_command(&args, cwd, now, writes, err),
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

/// CLI-3: `delete <repo>`, `restore <repo>` and `merge <repo>#<pr>` through `rules::guard`. A
/// refusal prints the guard's text and exits 1; `ToWaiting` also adds its Waiting on you item.
/// A merge the guard allows goes to `writes`, which applies GH-5's rule.
fn write_command(
    args: &super::Args,
    cwd: &Path,
    now: UnixSeconds,
    writes: &dyn Writes,
    err: &mut dyn Write,
) -> ExitCode {
    let command = args.command.first().map(String::as_str).unwrap_or_default();
    let action = match (command, args.command.get(1)) {
        ("merge", Some(target)) => match parse_target(target) {
            Some((repo, pr)) => GuardedAction::Merge { repo, pr },
            None => {
                let _ = writeln!(err, "switchboard: merge needs <repo>#<pr>");
                return ExitCode::from(2);
            }
        },
        ("delete", Some(repo)) => GuardedAction::DeleteRepo { repo: repo.clone() },
        ("restore", Some(repo)) => GuardedAction::RestoreOverLive { repo: repo.clone() },
        _ => {
            let _ = writeln!(err, "switchboard: {command} needs a repo");
            return ExitCode::from(2);
        }
    };
    match rules::guard(&guard_for(args, cwd), &action) {
        Verdict::Allow => match &action {
            GuardedAction::Merge { repo, pr } => match writes.merge(repo, *pr) {
                Ok(()) => {
                    println!("merge {repo}#{pr}: done");
                    ExitCode::SUCCESS
                }
                Err(text) => refuse(err, &text),
            },
            // The guard allows only a merge from the CLI (CORE-3).
            _ => refuse(err, &request_text(&action)),
        },
        Verdict::ToWaiting(text) => {
            let asked = format!("I was asked from the CLI to {}", request_text(&action));
            if let Err(e) = writes.waiting(&guarded_item(&action, &asked, now)) {
                let _ = writeln!(err, "{e}");
            }
            refuse(err, &text)
        }
        Verdict::Refuse(text) => refuse(err, &text),
    }
}

/// CLI-3: who is asking. The face is the CLI; the role is the `--role` flag, already checked;
/// a job worktree is found from `cwd`.
fn guard_for(args: &super::Args, cwd: &Path) -> Caller {
    Caller {
        face: Face::Cli,
        role: args.role.as_deref().and_then(Role::parse),
        in_job_worktree: rules::is_job_worktree(cwd),
        confirmed_in_app: false,
    }
}

/// The action as the guard words it, e.g. `delete switchboard`.
fn request_text(action: &GuardedAction) -> String {
    match action {
        GuardedAction::Merge { repo, pr } => format!("merge {repo}#{pr}"),
        GuardedAction::DeleteRepo { repo } => format!("delete {repo}"),
        GuardedAction::RestoreOverLive { repo } => format!("restore over {repo}"),
    }
}

/// A refusal: its text on stderr, exit 1.
fn refuse(err: &mut dyn Write, text: &str) -> ExitCode {
    let _ = writeln!(err, "{text}");
    ExitCode::from(1)
}

/// `<repo>#<pr>`: a non-empty repo and a PR number from 1.
fn parse_target(word: &str) -> Option<(String, u64)> {
    let (repo, pr) = word.split_once('#')?;
    let pr: u64 = pr.parse().ok().filter(|pr| *pr >= 1)?;
    (!repo.is_empty()).then(|| (repo.to_string(), pr))
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
