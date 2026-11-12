//! #384: the front door's MCP tools, each a thin call into the core the Chat view calls: `ask`
//! (route one message and answer it; `dry_run` routes and stops), `ask_as_job` (send a held
//! request as a fleet job another machine pulls) and `rate` (thumbs on one answer). The
//! project-manager role holds them (MCP-2); `mcp.rs` puts every call through
//! `rules::guard_front_door` first. Each answers with what the CLI command of the same name
//! prints with `--json`.

use std::path::Path;

use serde_json::{json, Value};
use switchboard_core::clock::{Clock, SystemClock};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::frontdoor::ask;
use switchboard_core::frontdoor::rating_log;
use switchboard_core::it;

/// The front door's tools, in the order `tools/list` gives them.
pub const TOOLS: [&str; 3] = ["ask", "ask_as_job", "rate"];

/// One front door tool call, its arguments read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Ask {
        message: String,
        thread: Option<String>,
        dry_run: bool,
    },
    AskAsJob {
        kind: String,
        request: String,
        /// #433: None when not given: the kind's needs this machine does not meet, read at
        /// the call (`ask::infer_unmet`).
        unmet: Option<Vec<String>>,
        /// FD-6: the chat this job is sent from; the job then carries its packet.
        thread: Option<String>,
    },
    Rate {
        thread: String,
        turn: u32,
        rating: String,
    },
}

impl Call {
    /// The tool's name.
    pub fn tool(&self) -> &'static str {
        match self {
            Call::Ask { .. } => "ask",
            Call::AskAsJob { .. } => "ask_as_job",
            Call::Rate { .. } => "rate",
        }
    }
}

/// Every front door tool's `(name, schema)`.
pub fn schemas() -> [(&'static str, Value); 3] {
    [
        (
            "ask",
            json!({
                "name": "ask",
                "description": "Route one message through the front door and answer it; dry_run returns the route and its reason and sends, stores and spends nothing.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" },
                        "thread": { "type": "string", "description": "An open thread's id; none starts a thread" },
                        "dry_run": { "type": "boolean" }
                    },
                    "required": ["message"]
                }
            }),
        ),
        (
            "ask_as_job",
            json!({
                "name": "ask_as_job",
                "description": "Send a request this machine cannot serve as a fleet job another machine pulls. With thread, the job carries that chat's packet (summary, last 6 turns, links) and the move is stored.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "description": "The request kind" },
                        "request": { "type": "string" },
                        "unmet": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "The needs this machine does not meet; left out, they are read from this machine's loadout" },
                        "thread": { "type": "string", "description": "The chat's thread id, to send the chat with its packet" }
                    },
                    "required": ["kind", "request"]
                }
            }),
        ),
        (
            "rate",
            json!({
                "name": "rate",
                "description": "Rate one answer of a thread up or down; a second rating of a turn replaces the first.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "thread": { "type": "string" },
                        "turn": { "type": "integer", "minimum": 0, "description": "The answer's index among the thread's turns" },
                        "rating": { "type": "string", "enum": ["up", "down"] }
                    },
                    "required": ["thread", "turn", "rating"]
                }
            }),
        ),
    ]
}

/// The call `name` and `args` ask for (`args` already checked against the schema's strings and
/// integers), or the invalid-params message.
pub fn call_from(name: &str, args: &Value) -> Result<Call, String> {
    let text = |key: &str| args[key].as_str().unwrap_or_default().to_string();
    match name {
        "ask" => {
            let dry_run = match &args["dry_run"] {
                Value::Null => false,
                Value::Bool(dry_run) => *dry_run,
                other => return Err(format!("ask: dry_run must be a boolean, got {other}")),
            };
            Ok(Call::Ask {
                message: text("message"),
                thread: args["thread"].as_str().map(str::to_string),
                dry_run,
            })
        }
        "ask_as_job" => {
            // #433: `unmet` left out is inferred; one given must be at least one string.
            let unmet = match &args["unmet"] {
                Value::Null => None,
                given => {
                    let unmet = given.as_array().and_then(|needs| {
                        needs
                            .iter()
                            .map(|need| need.as_str().map(str::to_string))
                            .collect::<Option<Vec<String>>>()
                    });
                    let Some(unmet) = unmet.filter(|needs| !needs.is_empty()) else {
                        return Err(format!(
                            "ask_as_job: unmet must be an array of at least one string, got {given}"
                        ));
                    };
                    Some(unmet)
                }
            };
            Ok(Call::AskAsJob {
                kind: text("kind"),
                request: text("request"),
                unmet,
                thread: args["thread"].as_str().map(str::to_string),
            })
        }
        "rate" => {
            let rating = text("rating");
            if rating_log::thumbs_from_name(&rating).is_none() {
                return Err(format!(
                    "rate: rating must be up or down, got {}",
                    args["rating"]
                ));
            }
            let Some(turn) = args["turn"].as_u64().and_then(|n| u32::try_from(n).ok()) else {
                return Err(format!(
                    "rate: turn must be an integer of at least 0, got {}",
                    args["turn"]
                ));
            };
            Ok(Call::Rate {
                thread: text("thread"),
                turn,
                rating,
            })
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

/// Run `call` in the core over the pipeline checkout and the app data folder (`None`: not
/// set, which is the error). The answer is the reply with `dry_run`, the queued job, or the
/// rating row; an error is the core's text after the tool's name.
pub fn run(call: &Call, fleet: Option<&FleetBridge>, data: Option<&Path>) -> Result<Value, String> {
    let tool = call.tool();
    let fleet = || {
        fleet.ok_or_else(|| format!("{tool}: set SWITCHBOARD_PIPELINE to your pipeline checkout"))
    };
    let data =
        || data.ok_or_else(|| format!("{tool}: set SWITCHBOARD_DATA to the app data folder"));
    let core = |e: switchboard_core::CoreError| format!("{tool}: {e}");
    let json = |e: serde_json::Error| format!("{tool}: {e}");
    match call {
        Call::Ask {
            message,
            thread,
            dry_run,
        } => {
            let asked = ask::ask_headless(fleet()?, data()?, message, thread.as_deref(), *dry_run)
                .map_err(core)?;
            serde_json::to_value(asked).map_err(json)
        }
        Call::AskAsJob {
            kind,
            request,
            unmet,
            thread,
        } => {
            let chat = match thread {
                Some(id) => Some((data()?, id.as_str())),
                None => None,
            };
            let fleet = fleet()?;
            let unmet = match unmet {
                Some(unmet) => unmet.clone(),
                None => {
                    let loadout = it::read_this_machine(&it::SystemProbeRunner, SystemClock.now());
                    ask::infer_unmet(fleet, kind, &loadout).map_err(core)?
                }
            };
            ask::send_as_job(fleet, kind, request, &unmet, chat).map_err(core)
        }
        Call::Rate {
            thread,
            turn,
            rating,
        } => {
            let row = rating_log::rate_turn(data()?, thread, *turn, rating).map_err(core)?;
            serde_json::to_value(row).map_err(json)
        }
    }
}
