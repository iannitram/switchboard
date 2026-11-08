//! `switchboard`: the CLI face (CLI-1 to CLI-3) and `switchboard mcp` (MCP-1 to MCP-3). Both
//! are thin calls into the core; neither holds a rule of its own.

// Skeleton only: stubs take arguments they don't read yet. Remove once CLI-1 and MCP-1 land.
#![allow(unused_variables, dead_code)]

mod cli;
mod mcp;

use std::process::ExitCode;

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
    todo!("CLI-1")
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

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match argv.first().map(String::as_str) {
        Some("mcp") => {
            let stdin = std::io::stdin();
            match mcp::serve(stdin.lock(), std::io::stdout().lock(), &Core) {
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
