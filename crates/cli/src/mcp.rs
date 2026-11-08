//! `switchboard mcp`: JSON-RPC over stdio.
//! MCP-1: without a role flag, six read tools only: `sky`, `constellation`, `star`, `waiting`,
//! `reports`, `budget`, their schemas under about 2,000 tokens together.
//! MCP-2: with a role flag, that role's write tools, each through CORE-3.
//! MCP-3: inside a job worktree, no write tool whatever the flag.

use std::process::ExitCode;

/// MCP-1: the read tools, in this order.
pub const READ_TOOLS: [&str; 6] = [
    "sky",
    "constellation",
    "star",
    "waiting",
    "reports",
    "budget",
];

pub fn serve(argv: &[String]) -> ExitCode {
    eprintln!("switchboard mcp: not built yet (MCP-1)");
    ExitCode::from(2)
}
