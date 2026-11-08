//! `switchboard mcp`: JSON-RPC over stdio.
//! MCP-1: without a role flag, six read tools only: `sky`, `constellation`, `star`, `waiting`,
//! `reports`, `budget`, their schemas under about 2,000 tokens together.
//! MCP-2: with a role flag, that role's write tools, each through CORE-3.
//! MCP-3: inside a job worktree, no write tool whatever the flag.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

/// The core behind the tools. `serve` passes a read tool's name and arguments here and returns
/// the answer as text. The real core is a thin struct in main.rs.
pub trait ReadCore {
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String>;
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

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "switchboard";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Read one JSON-RPC message per line from `reader` and write each reply as one line to
/// `writer`. Notifications (no `id`) get no reply. A malformed line gets -32700 and the loop
/// carries on. Returns when the reader ends or fails.
pub fn serve<R: BufRead, W: Write>(
    reader: R,
    mut writer: W,
    core: &dyn ReadCore,
) -> std::io::Result<()> {
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle(&line, core) {
            writeln!(writer, "{reply}")?;
            writer.flush()?;
        }
    }
    Ok(())
}

/// The reply to one line, or `None` for a notification.
fn handle(line: &str, core: &dyn ReadCore) -> Option<Value> {
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
        "tools/list" => ok(&id, json!({ "tools": tools() })),
        "tools/call" => call_tool(&id, &params, core),
        _ => error(
            &id,
            METHOD_NOT_FOUND,
            &format!("method not found: {method}"),
        ),
    })
}

/// `tools/call`: the core's answer as one JSON text content block; a core error as text with
/// `isError: true`.
fn call_tool(id: &Value, params: &Value, core: &dyn ReadCore) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !READ_TOOLS.contains(&name) {
        return error(id, INVALID_PARAMS, &format!("unknown tool: {name}"));
    }
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match core.call(name, &args) {
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

/// MCP-1: the six read tools with one-sentence descriptions and small input schemas.
fn tools() -> Value {
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
