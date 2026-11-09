//! `switchboard`: the CLI face (CLI-1 to CLI-3) and `switchboard mcp` (MCP-1 to MCP-3). Both
//! are thin calls into the core; neither holds a rule of its own.

// Skeleton only: stubs take arguments they don't read yet. Remove once CLI-1 and MCP-1 land.
#![allow(unused_variables, dead_code)]

mod cli;
mod mcp;

use std::process::ExitCode;

use switchboard_core::clock::SystemClock;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::rules;

/// The parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// `--role <role>`: unlocks that role's write actions (CLI-3, MCP-2).
    pub role: Option<String>,
    /// `--json`: print JSON instead of text (CLI-1).
    pub json: bool,
    /// The command and its words, e.g. `["star", "switchboard#12"]`.
    pub command: Vec<String>,
}

/// Parse `std::env::args` (without the program name). Flags may come anywhere.
pub fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        role: None,
        json: false,
        command: Vec::new(),
    };
    let mut words = argv.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--json" => args.json = true,
            "--role" => match words.next() {
                Some(value) if !value.starts_with("--") => args.role = Some(value.clone()),
                _ => return Err("--role needs a value".to_string()),
            },
            flag if flag.starts_with("--") => return Err(format!("unknown flag: {flag}")),
            _ => args.command.push(word.clone()),
        }
    }
    Ok(args)
}

/// The real core behind `switchboard mcp`: each read tool calls the same core call the CLI uses
/// for that answer. None of those calls is built yet, so each tool answers with the issue that
/// builds it.
struct Core;

impl mcp::ReadCore for Core {
    fn call(&self, tool: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let blocker = match tool {
            "sky" | "constellation" => "SKY-1, #30",
            "star" => "SKY-8, #43",
            "waiting" => "WAIT-1, #50",
            "reports" => "REP-1, #44",
            "budget" => "BUD-1, #23",
            other => return Err(format!("unknown tool: {other}")),
        };
        Err(format!("{tool}: not built yet ({blocker})"))
    }
}

/// MCP-2: the `--role <role>` flag of `switchboard mcp`; `None` without one. Any other value
/// is an error, so `serve` never starts.
fn mcp_role(words: &[String]) -> Result<Option<rules::Role>, String> {
    let mut words = words.iter();
    let mut role = None;
    while let Some(word) = words.next() {
        let value = match word.as_str() {
            "--role" => words
                .next()
                .map(String::as_str)
                .ok_or_else(|| "--role needs a value".to_string())?,
            other => match other.strip_prefix("--role=") {
                Some(value) => value,
                None => continue,
            },
        };
        role = Some(rules::Role::parse(value).ok_or_else(|| format!("unknown role: {value}"))?);
    }
    Ok(role)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match argv.first().map(String::as_str) {
        Some("mcp") => {
            let role = match mcp_role(&argv[1..]) {
                Ok(role) => role,
                Err(e) => {
                    eprintln!("switchboard mcp: {e}");
                    return ExitCode::from(2);
                }
            };
            // MCP-2: the real write core: the CLI's own merge, the pipeline checkout and the
            // app data folder from the environment, the real clock.
            let merge = cli::real_merge;
            let writes = mcp::CoreWrites {
                merge: &merge,
                fleet: cli::pipeline_dir().map(|root| FleetBridge::new(&root)),
                data: cli::data_dir(),
                clock: &SystemClock,
            };
            let stdin = std::io::stdin();
            match mcp::serve(stdin.lock(), std::io::stdout().lock(), &Core, role, &writes) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("switchboard mcp: {e}");
                    ExitCode::from(1)
                }
            }
        }
        _ => cli::run(&argv),
    }
}
