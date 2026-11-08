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

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match argv.first().map(String::as_str) {
        Some("mcp") => mcp::serve(&argv[1..]),
        _ => cli::run(&argv),
    }
}
