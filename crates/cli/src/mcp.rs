//! `switchboard mcp`: JSON-RPC over stdio.
//! MCP-1: without a role flag, six read tools only: `sky`, `constellation`, `star`, `waiting`,
//! `reports`, `budget`, their schemas under about 2,000 tokens together.
//! MCP-2: with a role flag, that role's write tools, each through CORE-3.
//! MCP-3: inside a job worktree, no write tool whatever the flag.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::{json, Value};
use switchboard_core::budget::{self, PoolEdit};
use switchboard_core::clock::Clock;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::rules::{self, Caller, Face, GuardedAction, Role, Verdict};
use switchboard_core::waiting::{Item, LocalItems, Source};

/// The core behind the tools. `serve` passes a read tool's name and arguments here and returns
/// the answer as text. The real core is a thin struct in main.rs.
pub trait ReadCore {
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String>;
}

/// The core behind the write tools (MCP-2). The real one is a thin struct in main.rs.
pub trait WriteCore {
    /// GH-5's eligibility (PASS on head, CI green) and the merge itself.
    fn merge(&self, repo: &str, pr: u64) -> Result<(), String>;
    fn set_pool_cap(&self, pool: &str, cap: f64) -> Result<(), String>;
    /// A guarded request the CLI or MCP may not run becomes a Waiting on you item.
    fn waiting(&self, text: &str) -> Result<(), String>;
}

/// MCP-2: the real write core. `merge` is the CLI's merge (CLI-3, #242); `set_pool_cap` is
/// BUD-2's `save_edit`; `waiting` adds a local Waiting on you item.
pub struct CoreWrites<'a> {
    pub merge: &'a dyn Fn(&str, u64) -> Result<(), String>,
    /// The pipeline checkout (`SWITCHBOARD_PIPELINE`); `None` when unset.
    pub fleet: Option<FleetBridge>,
    /// The app data folder (`SWITCHBOARD_DATA`, else the desktop app's); `None` when unknown.
    pub data: Option<PathBuf>,
    pub clock: &'a dyn Clock,
}

impl WriteCore for CoreWrites<'_> {
    fn merge(&self, repo: &str, pr: u64) -> Result<(), String> {
        (self.merge)(repo, pr)
    }

    fn set_pool_cap(&self, pool: &str, cap: f64) -> Result<(), String> {
        let fleet = self.fleet.as_ref().ok_or_else(|| {
            "set_pool_cap: set SWITCHBOARD_PIPELINE to your pipeline checkout".to_string()
        })?;
        let data = self.data.as_ref().ok_or_else(|| {
            "set_pool_cap: set SWITCHBOARD_DATA to the app data folder".to_string()
        })?;
        let edit = PoolEdit {
            pool: pool.to_string(),
            cap: Some(cap),
            ..PoolEdit::default()
        };
        let saved = budget::save_edit(fleet, &edit, &data.join("pool-edits.jsonl"), self.clock)
            .map_err(|e| format!("set_pool_cap {pool}: {e}"))?;
        LocalItems::new(data)
            .add(saved.waiting)
            .map_err(|e| format!("set_pool_cap {pool}: {e}"))
    }

    fn waiting(&self, text: &str) -> Result<(), String> {
        let data = self
            .data
            .as_ref()
            .ok_or_else(|| "waiting: set SWITCHBOARD_DATA to the app data folder".to_string())?;
        let item = Item {
            what: text.to_string(),
            why: "Asked for from the CLI or MCP; only the app can do it, after I confirm (CORE-3)."
                .to_string(),
            source: Source::GuardedAction {
                text: text.to_string(),
            },
            link: String::new(),
            action: "Confirm in the app".to_string(),
            since: self.clock.now(),
        };
        LocalItems::new(data)
            .add(item)
            .map_err(|e| format!("waiting: {e}"))
    }
}

/// MCP-1: the read tools, in this order.
pub const READ_TOOLS: [&str; 6] = [
    "sky",
    "constellation",
    "star",
    "waiting",
    "reports",
    "budget",
];

/// MCP-2: the write tools a role holds, in this order. No role, or a role with none, holds none.
pub fn write_tools(role: &Role) -> Vec<&'static str> {
    match role {
        Role::ProjectManager => vec!["merge"],
        Role::BudgetManager => vec!["set_pool_cap"],
        Role::Satellite => vec![],
    }
}

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "switchboard";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Read one JSON-RPC message per line from `reader` and write each reply as one line to
/// `writer`. Notifications (no `id`) get no reply. A malformed line gets -32700 and the loop
/// carries on. Returns when the reader ends or fails.
///
/// MCP-3: the job-worktree check runs once here, on the current directory. Inside one the role
/// is dropped, so the list and the calls hold the read tools only.
pub fn serve<R: BufRead, W: Write>(
    reader: R,
    mut writer: W,
    core: &dyn ReadCore,
    role: Option<Role>,
    write: &dyn WriteCore,
) -> std::io::Result<()> {
    let in_job_worktree = match std::env::current_dir() {
        Ok(dir) => rules::is_job_worktree(&dir),
        // Cannot tell where we are: fail closed, no write tools.
        Err(_) => true,
    };
    let role = if in_job_worktree {
        if role.is_some() {
            eprintln!("switchboard mcp: inside a job worktree; no write tools");
        }
        None
    } else {
        role
    };
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle(&line, core, role.as_ref(), write, in_job_worktree) {
            writeln!(writer, "{reply}")?;
            writer.flush()?;
        }
    }
    Ok(())
}

/// The reply to one line, or `None` for a notification.
fn handle(
    line: &str,
    core: &dyn ReadCore,
    role: Option<&Role>,
    write: &dyn WriteCore,
    in_job_worktree: bool,
) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(msg) => msg,
        Err(e) => {
            return Some(error(
                &Value::Null,
                PARSE_ERROR,
                &format!("parse error: {e}"),
            ))
        }
    };
    let id = msg.get("id").cloned();
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        return id.map(|id| error(&id, INVALID_REQUEST, "invalid request: no method"));
    };
    // A message without an id is a notification, e.g. `notifications/initialized`: no reply.
    let id = id?;
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => ok(
            &id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
            }),
        ),
        "ping" => ok(&id, json!({})),
        "tools/list" => ok(&id, json!({ "tools": tools(role) })),
        "tools/call" => call_tool(&id, &params, core, role, write, in_job_worktree),
        _ => error(
            &id,
            METHOD_NOT_FOUND,
            &format!("method not found: {method}"),
        ),
    })
}

/// `tools/call`: a read tool goes to the read core; a write tool the role holds goes through
/// CORE-3 (MCP-2). Anything else is unknown.
fn call_tool(
    id: &Value,
    params: &Value,
    core: &dyn ReadCore,
    role: Option<&Role>,
    write: &dyn WriteCore,
    in_job_worktree: bool,
) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if READ_TOOLS.contains(&name) {
        return read_call(id, name, &args, core);
    }
    match role {
        Some(role) if write_tools(role).contains(&name) => {
            write_call(id, name, &args, role, write, in_job_worktree)
        }
        _ => error(id, INVALID_PARAMS, &format!("unknown tool: {name}")),
    }
}

/// A read tool: the core's answer as one JSON text content block; a core error as text with
/// `isError: true`.
fn read_call(id: &Value, name: &str, args: &Value, core: &dyn ReadCore) -> Value {
    match core.call(name, args) {
        Ok(answer) => ok(
            id,
            json!({ "content": [{ "type": "text", "text": answer.to_string() }], "isError": false }),
        ),
        Err(message) => ok(
            id,
            json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
        ),
    }
}

/// MCP-2: a write tool. `merge` is guarded by CORE-3 here; the rule has no action for a pool
/// cap, so `set_pool_cap` is gated by its role alone (see `write_tools`).
fn write_call(
    id: &Value,
    name: &str,
    args: &Value,
    role: &Role,
    write: &dyn WriteCore,
    in_job_worktree: bool,
) -> Value {
    let outcome = match name {
        "merge" => match (args["repo"].as_str(), args["pr"].as_u64()) {
            (Some(repo), Some(pr)) => guarded_merge(role, repo, pr, write, in_job_worktree),
            _ => {
                return error(
                    id,
                    INVALID_PARAMS,
                    "merge needs repo (string) and pr (integer)",
                )
            }
        },
        _ => match (args["pool"].as_str(), args["cap"].as_f64()) {
            (Some(pool), Some(cap)) => write.set_pool_cap(pool, cap),
            _ => {
                return error(
                    id,
                    INVALID_PARAMS,
                    "set_pool_cap needs pool (string) and cap (number)",
                )
            }
        },
    };
    match outcome {
        Ok(()) => ok(
            id,
            json!({ "content": [{ "type": "text", "text": format!("{name}: done") }], "isError": false }),
        ),
        Err(message) => ok(
            id,
            json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
        ),
    }
}

/// CORE-3 first: `Allow` hands the merge to `WriteCore`, which applies GH-5's rule. A refusal
/// is the guard's text, the same one the CLI gives; `ToWaiting` also adds a Waiting item.
fn guarded_merge(
    role: &Role,
    repo: &str,
    pr: u64,
    write: &dyn WriteCore,
    in_job_worktree: bool,
) -> Result<(), String> {
    let caller = Caller {
        face: Face::Mcp,
        role: Some(role.clone()),
        in_job_worktree,
        confirmed_in_app: false,
    };
    let action = GuardedAction::Merge {
        repo: repo.to_string(),
        pr,
    };
    let verdict = rules::guard(&caller, &action);
    if let Verdict::ToWaiting(text) = &verdict {
        write.waiting(text)?;
    }
    match verdict.into_result() {
        Ok(()) => write.merge(repo, pr),
        Err(refusal) => Err(refusal.to_string()),
    }
}

/// MCP-1 and MCP-2: the six read tools, then the role's write tools.
fn tools(role: Option<&Role>) -> Value {
    let mut list = read_tools().as_array().cloned().unwrap_or_default();
    if let Some(role) = role {
        let held = write_tools(role);
        let all: [(&str, Value); 2] = [
            (
                "merge",
                json!({
                    "name": "merge",
                    "description": "Merge a PR once its latest PASS is on its head and CI is green on that head.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "repo": { "type": "string", "description": "The repo's name, e.g. switchboard" },
                            "pr": { "type": "integer", "minimum": 1 }
                        },
                        "required": ["repo", "pr"]
                    }
                }),
            ),
            (
                "set_pool_cap",
                json!({
                    "name": "set_pool_cap",
                    "description": "Set one budget pool's cap.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "pool": { "type": "string", "description": "The pool's name" },
                            "cap": { "type": "number", "minimum": 0 }
                        },
                        "required": ["pool", "cap"]
                    }
                }),
            ),
        ];
        list.extend(
            all.into_iter()
                .filter(|(name, _)| held.iter().any(|h| h == name))
                .map(|(_, schema)| schema),
        );
    }
    Value::Array(list)
}

/// MCP-1: the six read tools with one-sentence descriptions and small input schemas.
fn read_tools() -> Value {
    let limit = json!({ "type": "integer", "minimum": 1 });
    let repo = json!({ "type": "string", "description": "The repo's name, e.g. switchboard" });
    json!([
        {
            "name": "sky",
            "description": "The sky: one constellation per repo, as the GUI shows it.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "constellation",
            "description": "One repo's constellation: its stars and where they sit.",
            "inputSchema": {
                "type": "object",
                "properties": { "repo": repo.clone() },
                "required": ["repo"]
            }
        },
        {
            "name": "star",
            "description": "One star: an issue or PR with its labels, size, PR and review.",
            "inputSchema": {
                "type": "object",
                "properties": { "repo": repo, "issue": { "type": "integer", "minimum": 1 } },
                "required": ["repo", "issue"]
            }
        },
        {
            "name": "waiting",
            "description": "The Waiting on you list: items that need a person, one merged list.",
            "inputSchema": { "type": "object", "properties": { "limit": limit.clone() } }
        },
        {
            "name": "reports",
            "description": "Reports built from job cards and handoff threads.",
            "inputSchema": { "type": "object", "properties": { "limit": limit } }
        },
        {
            "name": "budget",
            "description": "Fleet budget: the pool rows and what is left in each.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

fn ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
