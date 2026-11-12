//! `switchboard mcp`: JSON-RPC over stdio.
//! MCP-1: without a role flag, six read tools only: `sky`, `constellation`, `star`, `waiting`,
//! `reports`, `budget`, their schemas under about 2,000 tokens together.
//! MCP-2: with a role flag, that role's write tools, each through CORE-3: project-manager
//! `merge`, `request_archive`, `request_restore`, `drop_request` (#347: these file or turn down
//! a Waiting on you request and can never confirm one); budget-manager `set_pool_cap`, `pool_edit` (plan, cap, reset, cost, #374) and
//! `budget_refresh` (#374), the pool edits through `rules::guard_pool_edit` and
//! `budget::save_pool_edit`, as the CLI's commands of the same names.
//! `reports` takes the CLI's `project`, `outcome` and `pool` filters (#358).
//! #384: project-manager also holds the front door, `ask`, `ask_as_job` and `rate`
//! (`mcp_frontdoor.rs`), through `rules::guard_front_door`.
//! MCP-3: inside a job worktree, no write tool whatever the flag.
//! #375: the satellite role also reads `errors`, the fleet errors the app recorded and the open
//! IT tickets (IT-3: triage runs on another machine). MCP-1's six stay the list with no role.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::{json, Value};
use switchboard_core::budget::{self, PoolEdit};
use switchboard_core::clock::Clock;
use switchboard_core::confirm;
use switchboard_core::error::CoreError;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::rules::{self, Caller, Face, GuardedAction, Role, Verdict};
use switchboard_core::waiting::{guarded_item, Item, LocalItems, Source};

#[path = "mcp_frontdoor.rs"]
pub mod frontdoor;

/// The core behind the tools. `serve` passes a read tool's name and arguments here and returns
/// the answer as text. The real core is a thin struct in main.rs.
pub trait ReadCore {
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String>;
}

/// The core behind the write tools (MCP-2). The real one is a thin struct in main.rs.
pub trait WriteCore {
    /// GH-5's eligibility (PASS on head, CI green) and the merge itself.
    fn merge(&self, repo: &str, pr: u64) -> Result<(), String>;
    /// BUD-2: save `edit` (file, ledger row and Waiting item, all or nothing, #340). `tool`
    /// names the tool in the text; `Ok` is the reply stating what was set (#362), `Err` the
    /// reason nothing changed.
    fn pool_edit(&self, tool: &str, edit: &PoolEdit) -> Result<String, String>;
    /// BUD-2: `pool_edit` with the cap alone.
    fn set_pool_cap(&self, pool: &str, cap: f64) -> Result<String, String> {
        let edit = PoolEdit {
            pool: pool.to_string(),
            cap: Some(cap),
            ..PoolEdit::default()
        };
        self.pool_edit("set_pool_cap", &edit)
    }
    /// BUD-3: re-read every pool that has a reader now (`fleet budget --refresh`); the rows.
    fn budget_refresh(&self) -> Result<Value, String>;
    /// A guarded request the CLI or MCP may not run becomes a Waiting on you item.
    fn waiting(&self, text: &str) -> Result<(), String>;
    /// #347: the `owner/name` that `repo` names among the repos Switchboard knows; `Ok(None)`
    /// when it knows none, an error when it cannot tell. The default knows no repo list.
    fn full_repo(&self, repo: &str) -> Result<Option<String>, String> {
        let _ = repo;
        Err("no repo list here".to_string())
    }
    /// #347: file the request for `action` (an archive or a restore over a live repo) as a
    /// Waiting on you item whose text is `text`. It files only: nothing here confirms.
    fn request(&self, action: &GuardedAction, text: &str) -> Result<(), String> {
        let _ = action;
        self.waiting(text)
    }
    /// #347: turn down every waiting request for `action` (`confirm::drop_requests_for`); one
    /// line for each request dropped, or the refusal text.
    fn drop_requests(
        &self,
        action: &GuardedAction,
        caller: &Caller,
    ) -> Result<Vec<String>, String> {
        let _ = (action, caller);
        Err("drop_request: no Waiting on you list here".to_string())
    }
    /// #384: one front door call (`frontdoor::run` in the real core); the answer as JSON.
    fn front_door(&self, call: &frontdoor::Call) -> Result<Value, String> {
        Err(format!("{}: this core has no front door", call.tool()))
    }
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

    fn pool_edit(&self, tool: &str, edit: &PoolEdit) -> Result<String, String> {
        let fleet = self
            .fleet
            .as_ref()
            .ok_or_else(|| format!("{tool}: set SWITCHBOARD_PIPELINE to your pipeline checkout"))?;
        let data = self
            .data
            .as_ref()
            .ok_or_else(|| format!("{tool}: set SWITCHBOARD_DATA to the app data folder"))?;
        let pool = &edit.pool;
        // An empty pool name leaves no gap before the colon (#416).
        let who = if pool.is_empty() {
            tool.to_string()
        } else {
            format!("{tool} {pool}")
        };
        let saved = budget::save_pool_edit(fleet, edit, data, self.clock)
            .map_err(|e| format!("{who}: {e}"))?;
        Ok(format!(
            "{tool} {pool}: {}",
            budget::edit_summary(&saved.ledger_row)
        ))
    }

    fn budget_refresh(&self) -> Result<Value, String> {
        let fleet = self.fleet.as_ref().ok_or_else(|| {
            "budget_refresh: set SWITCHBOARD_PIPELINE to your pipeline checkout".to_string()
        })?;
        let rows = budget::refresh(fleet).map_err(|e| format!("budget_refresh: {e}"))?;
        serde_json::to_value(rows).map_err(|e| format!("budget_refresh: {e}"))
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

    /// The stored snapshots' repos. A snapshot file that does not read stops the answer: the
    /// request is never filed from a view that may be missing the repo it names.
    fn full_repo(&self, repo: &str) -> Result<Option<String>, String> {
        let data = self.data_dir()?;
        let (snaps, bad) = SnapshotStore::new(data)
            .load_each()
            .map_err(|e| e.to_string())?;
        if let Some(file) = bad.first() {
            return Err(format!("{}; run switchboard refresh", file.error));
        }
        if snaps.iter().any(|snap| snap.repo == repo) {
            return Ok(Some(repo.to_string()));
        }
        if repo.contains('/') {
            return Ok(None);
        }
        let named: Vec<&str> = snaps
            .iter()
            .map(|snap| snap.repo.as_str())
            .filter(|full| full.rsplit_once('/').is_some_and(|(_, name)| name == repo))
            .collect();
        match named.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(one.to_string())),
            many => Err(format!(
                "{repo} matches {}; name it as <owner>/{repo}",
                many.join(" and ")
            )),
        }
    }

    fn request(&self, action: &GuardedAction, text: &str) -> Result<(), String> {
        LocalItems::new(self.data_dir()?)
            .add_new(guarded_item(action, text, self.clock.now()))
            .map(|_| ())
            .map_err(|e| format!("waiting: {e}"))
    }

    fn drop_requests(
        &self,
        action: &GuardedAction,
        caller: &Caller,
    ) -> Result<Vec<String>, String> {
        confirm::drop_requests_for(self.data_dir()?, action, caller)
            .map(|dropped| dropped.lines)
            .map_err(|e| match e {
                CoreError::Refused(why) => why,
                other => format!("drop_request: {other}"),
            })
    }

    fn front_door(&self, call: &frontdoor::Call) -> Result<Value, String> {
        frontdoor::run(call, self.fleet.as_ref(), self.data.as_deref())
    }
}

impl CoreWrites<'_> {
    fn data_dir(&self) -> Result<&std::path::Path, String> {
        self.data
            .as_deref()
            .ok_or_else(|| "set SWITCHBOARD_DATA to the app data folder".to_string())
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
/// #385: the autopilot's pass tools come last: `autopilot_pass` for the Project Manager,
/// `satellite_tick` for the satellite.
pub fn write_tools(role: &Role) -> Vec<&'static str> {
    let mut tools = role_write_tools(role);
    match role {
        Role::ProjectManager => tools.push("autopilot_pass"),
        Role::Satellite => tools.push("satellite_tick"),
        Role::BudgetManager => {}
    }
    tools
}

fn role_write_tools(role: &Role) -> Vec<&'static str> {
    match role {
        Role::ProjectManager => {
            let mut held = vec!["merge"];
            held.extend(frontdoor::TOOLS);
            // #347: the tools that file or turn down a guarded request; none confirms.
            held.extend(["request_archive", "request_restore", "drop_request"]);
            held
        }
        Role::BudgetManager => vec!["set_pool_cap", "pool_edit", "budget_refresh"],
        Role::Satellite => vec![],
    }
}

/// #375: the read tools a role adds to MCP-1's six, in this order. The satellite is the machine
/// that triages IT tickets (IT-3), so it reads the fleet errors and the tickets.
pub fn role_read_tools(role: &Role) -> Vec<&'static str> {
    let mut tools = match role {
        Role::Satellite => vec!["errors"],
        Role::ProjectManager | Role::BudgetManager => vec![],
    };
    // #385: the two roles that run autopilot passes read its state.
    if matches!(role, Role::ProjectManager | Role::Satellite) {
        tools.push("autopilot_status");
    }
    tools
}

/// #385: the role an autopilot tool is guarded with (`rules::guard_autopilot`), or `None` for
/// any other tool. The status read is asked with the caller's own role, so only the job
/// worktree check can refuse it.
fn autopilot_needs(tool: &str, role: Option<&Role>) -> Option<Role> {
    match tool {
        "autopilot_pass" => Some(Role::ProjectManager),
        "satellite_tick" => Some(Role::Satellite),
        "autopilot_status" => role.cloned(),
        _ => None,
    }
}

/// #385: an autopilot tool. The guard is the CLI's; the answer is the read core's (the status,
/// or the pass's report as JSON). A report with errors is `isError: true`, its text still the
/// whole report.
fn autopilot_call(
    id: &Value,
    name: &str,
    args: &Value,
    core: &dyn ReadCore,
    role: Option<&Role>,
    needs: &Role,
    in_job_worktree: bool,
) -> Value {
    let caller = Caller {
        face: Face::Mcp,
        role: role.cloned(),
        in_job_worktree,
        confirmed_in_app: false,
    };
    let said = |text: String, failed: bool| {
        ok(
            id,
            json!({ "content": [{ "type": "text", "text": text }], "isError": failed }),
        )
    };
    if let Verdict::Refuse(text) | Verdict::ToWaiting(text) =
        rules::guard_autopilot(&caller, name, needs)
    {
        return said(text, true);
    }
    match core.call(name, args) {
        Ok(answer) => {
            let failed = answer["errors"].as_array().is_some_and(|e| !e.is_empty());
            said(answer.to_string(), failed)
        }
        Err(message) => said(message, true),
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
    writer: W,
    core: &dyn ReadCore,
    role: Option<Role>,
    write: &dyn WriteCore,
) -> std::io::Result<()> {
    let in_job_worktree = match std::env::current_dir() {
        Ok(dir) => rules::is_job_worktree(&dir),
        // Cannot tell where we are: fail closed, no write tools.
        Err(_) => true,
    };
    serve_with(reader, writer, core, role, write, in_job_worktree)
}

/// `serve` told whether it runs inside a job worktree instead of looking at the current
/// directory. Tests use it so a checkout whose folder name looks like a job worktree (it
/// contains `-fleet-`) cannot change their result (#433).
pub fn serve_with<R: BufRead, W: Write>(
    reader: R,
    mut writer: W,
    core: &dyn ReadCore,
    role: Option<Role>,
    write: &dyn WriteCore,
    in_job_worktree: bool,
) -> std::io::Result<()> {
    let role = if in_job_worktree {
        if role.is_some() {
            eprintln!("switchboard mcp: inside a job worktree; no write tools");
        }
        None
    } else {
        role
    };
    // Split on bytes, not `lines()`: a line that is not UTF-8 gets a parse error and the loop
    // carries on (#336).
    for raw in reader.split(b'\n') {
        let mut raw = raw?;
        if raw.last() == Some(&b'\r') {
            raw.pop();
        }
        if raw.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let reply = match String::from_utf8(raw) {
            Ok(line) => handle(&line, core, role.as_ref(), write, in_job_worktree),
            Err(_) => Some(error(
                &Value::Null,
                PARSE_ERROR,
                "parse error: line is not valid UTF-8",
            )),
        };
        if let Some(reply) = reply {
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
    // JSON-RPC 2.0 sec. 6 batches are not supported: MCP's stdio transport (2024-11-05, the
    // version this server speaks) sends one message per line, and later revisions drop batching.
    // A batch, like any other non-object, gets one Invalid Request with a null id (#353).
    let Some(obj) = msg.as_object() else {
        let what = if msg.is_array() {
            "invalid request: batch requests are not supported, send one request per line"
        } else {
            "invalid request: a request is a JSON object"
        };
        return Some(error(&Value::Null, INVALID_REQUEST, what));
    };
    // An id is a string or a number; anything else cannot be echoed back, so it is null.
    let id = match obj.get("id") {
        Some(id) if id.is_string() || id.is_number() => Some(id.clone()),
        Some(Value::Null) | None => None,
        Some(_) => {
            return Some(error(
                &Value::Null,
                INVALID_REQUEST,
                "invalid request: id must be a string or a number",
            ))
        }
    };
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // A response to a server request (none are sent) is not a request: no reply.
        if id.is_some() && (obj.contains_key("result") || obj.contains_key("error")) {
            return None;
        }
        return Some(error(
            &id.unwrap_or(Value::Null),
            INVALID_REQUEST,
            "invalid request: method must be a string",
        ));
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
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return error(id, INVALID_PARAMS, "tools/call needs name (string)");
    };
    let raw_args = params.get("arguments").unwrap_or(&Value::Null);
    let held = role.is_some_and(|r| write_tools(r).contains(&name));
    let read =
        READ_TOOLS.contains(&name) || role.is_some_and(|r| role_read_tools(r).contains(&name));
    if !read && !held {
        return error(id, INVALID_PARAMS, &format!("unknown tool: {name}"));
    }
    let args = match tool_schema(name).map(|schema| check_args(name, &schema, raw_args)) {
        Some(Ok(args)) => args,
        Some(Err(message)) => return error(id, INVALID_PARAMS, &message),
        None => raw_args.clone(),
    };
    if let Some(needs) = autopilot_needs(name, role) {
        return autopilot_call(id, name, &args, core, role, &needs, in_job_worktree);
    }
    if read {
        return read_call(id, name, &args, core);
    }
    match role {
        Some(role) if held => write_call(id, name, &args, role, write, in_job_worktree),
        _ => error(id, INVALID_PARAMS, &format!("unknown tool: {name}")),
    }
}

/// A tool's `inputSchema`, from the same lists `tools/list` serves.
fn tool_schema(name: &str) -> Option<Value> {
    let all = read_tools().as_array().cloned().unwrap_or_default();
    all.into_iter()
        .chain(role_read_schemas().into_iter().map(|(_, schema)| schema))
        .chain(write_schemas().into_iter().map(|(_, schema)| schema))
        .chain(frontdoor::schemas().into_iter().map(|(_, schema)| schema))
        .find(|tool| tool["name"] == name)
        .map(|tool| tool["inputSchema"].clone())
}

/// Check `args` against a tool's `inputSchema` (#360): an object; every `required` property
/// present; each given property the right `type` (`string`, `integer`, `number`) and at least its
/// `minimum` (integers only). The error names the tool and the argument. An integer written `2.0` is one and
/// comes back as `2`. Properties the schema does not list are left alone.
fn check_args(tool: &str, schema: &Value, args: &Value) -> Result<Value, String> {
    let mut args = match args {
        Value::Null => serde_json::Map::new(),
        Value::Object(map) => map.clone(),
        _ => return Err(format!("{tool}: arguments must be an object")),
    };
    let required = schema["required"].as_array().cloned().unwrap_or_default();
    for name in required.iter().filter_map(Value::as_str) {
        if !args.contains_key(name) {
            return Err(format!("{tool}: {name} is required"));
        }
    }
    let props = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (name, prop) in props {
        let Some(value) = args.get(&name).cloned() else {
            continue;
        };
        let kind = prop["type"].as_str().unwrap_or_default();
        let bad = |want: &str| format!("{tool}: {name} must be {want}, got {value}");
        let min = prop["minimum"].as_f64();
        let low = |n: f64| min.is_some_and(|m| n < m);
        let want_min = |want: &str| match min {
            Some(m) => format!("{want} of at least {m}"),
            None => want.to_string(),
        };
        match kind {
            "string" if !value.is_string() => return Err(bad("a string")),
            "integer" => {
                let n = value
                    .as_i64()
                    .map(Value::from)
                    .or_else(|| value.as_u64().map(Value::from))
                    .or_else(|| {
                        let f = value.as_f64()?;
                        (f.fract() == 0.0 && f.abs() < 9.0e15).then(|| Value::from(f as i64))
                    })
                    .ok_or_else(|| bad("an integer"))?;
                if low(n.as_f64().unwrap_or(0.0)) {
                    return Err(bad(&want_min("an integer")));
                }
                args.insert(name, n);
            }
            // A number's `minimum` (a pool cap's 0) is left to the core, whose refusal says why.
            "number" if value.as_f64().is_none() => return Err(bad("a number")),
            "boolean" if !value.is_boolean() => return Err(bad("true or false")),
            _ => {}
        }
    }
    Ok(Value::Object(args))
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

/// MCP-2: a write tool. `merge` is guarded by CORE-3 here; the pool edits by
/// `rules::guard_pool_edit`, the same check the CLI makes (the role already holds them, see
/// `write_tools`). The reply is the core's text: `merge: done`, the pool edit's new values
/// (#362), or the re-read rows as JSON.
fn write_call(
    id: &Value,
    name: &str,
    args: &Value,
    role: &Role,
    write: &dyn WriteCore,
    in_job_worktree: bool,
) -> Value {
    let caller = Caller {
        face: Face::Mcp,
        role: Some(role.clone()),
        in_job_worktree,
        confirmed_in_app: false,
    };
    let outcome = match name {
        "merge" => match (args["repo"].as_str(), args["pr"].as_u64()) {
            (Some(repo), Some(pr)) => guarded_merge(role, repo, pr, write, in_job_worktree)
                .map(|()| format!("{name}: done")),
            _ => {
                return error(
                    id,
                    INVALID_PARAMS,
                    "merge needs repo (string) and pr (integer)",
                )
            }
        },
        "set_pool_cap" => match (args["pool"].as_str(), args["cap"].as_f64()) {
            (Some(pool), Some(cap)) => {
                guard_pool(&caller, pool).and_then(|()| write.set_pool_cap(pool, cap))
            }
            _ => {
                return error(
                    id,
                    INVALID_PARAMS,
                    "set_pool_cap needs pool (string) and cap (number)",
                )
            }
        },
        "pool_edit" => {
            let edit = PoolEdit {
                pool: args["pool"].as_str().unwrap_or_default().to_string(),
                plan: args["plan"].as_str().map(str::to_string),
                cap: args["cap"].as_f64(),
                reset: args["reset"].as_str().map(str::to_string),
                cost_usd: args["cost_usd"].as_f64(),
            };
            guard_pool(&caller, &edit.pool).and_then(|()| write.pool_edit(name, &edit))
        }
        "request_archive" | "request_restore" | "drop_request" => {
            let kind = match name {
                "request_archive" => Some("archive"),
                "request_restore" => Some("restore"),
                _ => args["kind"].as_str(),
            };
            match (kind, args["repo"].as_str()) {
                (Some(kind @ ("archive" | "restore")), Some(repo)) => {
                    guarded_request(name, kind == "archive", repo, &caller, write)
                }
                _ => {
                    return error(
                        id,
                        INVALID_PARAMS,
                        "drop_request needs kind (archive or restore) and repo (string)",
                    )
                }
            }
        }
        // #384: the front door, the same check the CLI makes.
        tool if frontdoor::TOOLS.contains(&tool) => {
            let call = match frontdoor::call_from(tool, args) {
                Ok(call) => call,
                Err(message) => return error(id, INVALID_PARAMS, &message),
            };
            match rules::guard_front_door(&caller, tool) {
                Verdict::Allow => write.front_door(&call).map(|answer| answer.to_string()),
                Verdict::Refuse(text) | Verdict::ToWaiting(text) => Err(text),
            }
        }
        // `budget_refresh`: a re-read, held by the role alone (`write_tools`).
        _ => write.budget_refresh().map(|rows| rows.to_string()),
    };
    match outcome {
        Ok(text) => ok(
            id,
            json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
        ),
        Err(message) => ok(
            id,
            json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
        ),
    }
}

/// `rules::guard_pool_edit` as a `Result`: the refusal is its text, as the CLI prints it.
fn guard_pool(caller: &Caller, pool: &str) -> Result<(), String> {
    match rules::guard_pool_edit(caller, pool) {
        Verdict::Allow => Ok(()),
        Verdict::Refuse(text) | Verdict::ToWaiting(text) => Err(text),
    }
}

/// #347: `request_archive` and `request_restore` file the request in Waiting on you and
/// `drop_request` turns it down. None of them confirms: the guard never allows an archive or a
/// restore from MCP (CORE-3), and the typed confirm stays with me in the app. A repo
/// Switchboard does not know files nothing; `drop_request` takes an `owner/name` it no longer
/// knows as written, so its request can still be dropped.
fn guarded_request(
    tool: &str,
    archive: bool,
    repo: &str,
    caller: &Caller,
    write: &dyn WriteCore,
) -> Result<String, String> {
    let dropping = tool == "drop_request";
    let repo = match write.full_repo(repo).map_err(|e| format!("{tool}: {e}"))? {
        Some(full) => full,
        None if dropping && repo.contains('/') => repo.to_string(),
        None => return Err(format!("{tool}: no such repo: {repo}")),
    };
    let (action, what) = if archive {
        let what = format!("archive {repo}");
        (GuardedAction::DeleteRepo { repo }, what)
    } else {
        let what = format!("restore over {repo}");
        (GuardedAction::RestoreOverLive { repo }, what)
    };
    if dropping {
        return write
            .drop_requests(&action, caller)
            .map(|lines| lines.join("\n"));
    }
    match rules::guard(caller, &action) {
        Verdict::ToWaiting(text) => {
            write.request(&action, &format!("I was asked from MCP to {what}"))?;
            Ok(format!("{text}; the request is in Waiting on you"))
        }
        Verdict::Refuse(text) => Err(text),
        // The guard allows neither from MCP; were that to change, this still runs nothing.
        Verdict::Allow => Err(format!("{what}: needs a confirm in the app")),
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

/// MCP-1 and MCP-2: the six read tools, then the role's read tools (#375), then its write tools.
fn tools(role: Option<&Role>) -> Value {
    let mut list = read_tools().as_array().cloned().unwrap_or_default();
    if let Some(role) = role {
        let reads = role_read_tools(role);
        list.extend(
            role_read_schemas()
                .into_iter()
                .filter(|(name, _)| reads.contains(name))
                .map(|(_, schema)| schema),
        );
        let held = write_tools(role);
        // In `write_tools`' order, so the front door's tools (#384) stay beside `merge` and
        // the autopilot's pass tool (#385) comes last.
        let all: Vec<(&str, Value)> = write_schemas()
            .into_iter()
            .chain(frontdoor::schemas())
            .collect();
        list.extend(held.iter().filter_map(|h| {
            all.iter()
                .find(|(name, _)| name == h)
                .map(|(_, schema)| schema.clone())
        }));
    }
    Value::Array(list)
}

/// #375: every role read tool's `(name, schema)`; `tools` lists the ones a role holds.
fn role_read_schemas() -> [(&'static str, Value); 2] {
    [
        (
            "errors",
            json!({
                "name": "errors",
                "description": "The fleet errors the app recorded, newest first, and the open IT tickets.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
        ),
        (
            "autopilot_status",
            json!({
                "name": "autopilot_status",
                "description": "The autopilot's state: what its passes have started and seen, whether the weekly run is due, the running jobs, and the satellite's loops and holds.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
        ),
    ]
}

/// MCP-2: every write tool's `(name, schema)`; `tools` lists the ones a role holds.
fn write_schemas() -> [(&'static str, Value); 9] {
    let repo = json!({ "type": "string", "description": "The repo's name or owner/name" });
    [
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
            "request_archive",
            json!({
                "name": "request_archive",
                "description": "Ask to archive a repo (read-only on GitHub). This only files a Waiting on you request; the owner confirms it in the app.",
                "inputSchema": { "type": "object", "properties": { "repo": repo }, "required": ["repo"] }
            }),
        ),
        (
            "request_restore",
            json!({
                "name": "request_restore",
                "description": "Ask to restore a repo's backup over the live repo. This only files a Waiting on you request; the owner confirms it in the app.",
                "inputSchema": { "type": "object", "properties": { "repo": repo }, "required": ["repo"] }
            }),
        ),
        (
            "drop_request",
            json!({
                "name": "drop_request",
                "description": "Turn down a waiting archive or restore request: it leaves Waiting on you and nothing else happens.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "description": "archive or restore" },
                        "repo": repo
                    },
                    "required": ["kind", "repo"]
                }
            }),
        ),
        (
            "set_pool_cap",
            json!({
                "name": "set_pool_cap",
                "description": "Set one budget pool's dollar cap; the reply states the new cap.",
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
        (
            "pool_edit",
            json!({
                "name": "pool_edit",
                "description": "Edit one budget pool in place (plan, cap, reset, cost), as the Budget panel does.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pool": { "type": "string", "description": "The pool's name" },
                        "plan": { "type": "string" },
                        "cap": { "type": "number", "minimum": 0 },
                        "reset": { "type": "string", "description": "e.g. Sun 6:00am" },
                        "cost_usd": { "type": "number", "minimum": 0 }
                    },
                    "required": ["pool"]
                }
            }),
        ),
        (
            "budget_refresh",
            json!({
                "name": "budget_refresh",
                "description": "Re-read every pool that has a reader now, then return the budget rows.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
        ),
        (
            "autopilot_pass",
            json!({
                "name": "autopilot_pass",
                "description": "Run one autopilot pass now, as the app's poll does, and return its report. With dry_run it starts no job, merges nothing and writes no state, and reports what it would do.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pass": { "type": "string", "enum": ["roadmap", "review", "merge", "weekly"] },
                        "dry_run": { "type": "boolean" }
                    },
                    "required": ["pass"]
                }
            }),
        ),
        (
            "satellite_tick",
            json!({
                "name": "satellite_tick",
                "description": "Run one tick of the satellite loop on this machine and return its report; the loops it starts run on while this server lives. With dry_run it starts nothing and reports the lanes it would start.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "dry_run": { "type": "boolean" } }
                }
            }),
        ),
    ]
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
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": limit,
                    "project": { "type": "string" },
                    "outcome": { "type": "string" },
                    "pool": { "type": "string" }
                }
            }
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

/// `serve` outside a job worktree whatever the current directory is: for tests.
pub fn serve_outside_job_worktree<R: BufRead, W: Write>(
    reader: R,
    writer: W,
    core: &dyn ReadCore,
    role: Option<Role>,
    write: &dyn WriteCore,
) -> std::io::Result<()> {
    serve_with(reader, writer, core, role, write, false)
}
