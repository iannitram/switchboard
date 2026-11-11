//! FD-11, FD-13: answer one routed chat turn. The route's pick is a service and model: build that
//! service's adapter, send the turn with the thread's earlier turns, and store the user turn and
//! the answer turn (with its log row) in the thread. A Claude handoff or a wait is not answered:
//! nothing is sent (FD-8); a wait's ask is queued in the thread (FD-3, #377).
//!
//! The transport and the key lookup are passed in, so a test sends nothing and reads no
//! environment. A key is never logged, stored or returned.

use super::adapters::{
    log_row, Answer, ChatAdapter, ChatGptAdapter, ChatTransport, ClaudeApiAdapter, DeepSeekAdapter,
    GeminiAdapter, QwenAdapter,
};
use super::chat::{ChatPick, ChatRoute};
use super::packet::{self, Packet, Turn};
use super::rating_log::rating_id;
use super::sticky::{handoff_step, open_chat, with_move, OpenChat};
use super::thread::{is_turn, ThreadStore};
use crate::budget::config_path;
use crate::fleet::FleetBridge;
use crate::joblog::{JobRow, RowSource};
use crate::CoreError;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

/// The environment variable each paid service's key is read from.
pub const KEY_VARS: [(&str, &str); 4] = [
    ("deepseek", "DEEPSEEK_API_KEY"),
    ("chatgpt", "OPENAI_API_KEY"),
    ("gemini", "GEMINI_API_KEY"),
    ("claudeapi", "ANTHROPIC_API_KEY"),
];

const DEEPSEEK_URL: &str = "https://api.deepseek.com";
const OPENAI_URL: &str = "https://api.openai.com";
const GEMINI_URL: &str = "https://generativelanguage.googleapis.com";
const ANTHROPIC_URL: &str = "https://api.anthropic.com";

/// USD per million tokens (input, output) for the paid pools that price by token. Estimates:
/// the cost column is only as good as these.
const DEEPSEEK_USD_PER_MILLION: (f64, f64) = (0.27, 1.10);
const CLAUDE_API_USD_PER_MILLION: (f64, f64) = (3.0, 15.0);

const SYSTEM_PROMPT: &str = "You are a helpful assistant answering in a chat.";

/// The environment variable that holds `service`'s key, or None for a service with no key.
pub fn key_var(service: &str) -> Option<&'static str> {
    KEY_VARS
        .iter()
        .find(|(name, _)| *name == service)
        .map(|(_, var)| *var)
}

/// What answering a turn needs from the caller.
pub struct AnswerContext<'a> {
    /// The app data folder: the thread store lives under it.
    pub data_dir: &'a Path,
    /// The local qwen server's base URL (see `qwen_base_url`), None when the fleet config has none.
    pub qwen_base_url: Option<&'a str>,
    /// The value of an environment variable, by name (the real one is `std::env::var`).
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// Makes a transport for one call (the real one is `HttpChatTransport::new`).
    pub transport: &'a dyn Fn() -> Box<dyn ChatTransport>,
}

/// What the Chat view reads for one message: the route, and the answer when the core made one.
/// A Claude handoff or a wait carries no answer fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatReply {
    #[serde(flatten)]
    pub route: ChatRoute,
    /// The thread the turn is stored in; the id the rating and implicit-signal rows go under.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The reason in the model's `[[handoff: <reason>]]` marker, if its reply had one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff: Option<String>,
    /// FD-3 (#377): set when nothing was in budget: the ask is queued in its thread (under
    /// `handoff_id`) and this says why, for the chat to show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued: Option<String>,
    /// FD-6 (#378): set when this turn moved the chat off another model: that model, as
    /// `<service> <model>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved_from: Option<String>,
    /// FD-8 (#378): on a Claude handoff that moves an open chat, the text the new Claude
    /// session starts from: the packet (`packet::as_prompt`), then the ask.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packet: Option<String>,
}

/// The `role` of a thread's rolling-summary row (one per thread, rewritten at each move).
pub const SUMMARY_ROLE: &str = "summary";
/// The `role` of a move row: the packet the incoming model was sent, as its text.
pub const MOVE_ROLE: &str = "move";

const SUMMARY_SYSTEM: &str = "You are handing this chat to another model. Write the rolling \
summary of the chat so far: rewrite the earlier summary with what the chat added, never stack \
summaries. Name every file link and open question. Answer with the summary only.";

/// The `outcome` of a user turn that was queued because nothing was in budget (FD-3).
pub const QUEUED: &str = "queued";

/// A fresh thread id from the clock: `chat-<unix nanoseconds>`.
pub fn new_handoff_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("chat-{nanos}")
}

/// The qwen base URL for `machine` from a parsed fleet `config.json`:
/// `machines.<machine>.qwen_server_url` without its `/v1/models` tail and trailing slash.
pub fn qwen_base_from_config(config: &Value, machine: Option<&str>) -> Option<String> {
    let url = config
        .get("machines")?
        .get(machine?)?
        .get("qwen_server_url")?
        .as_str()?;
    let url = url.trim_end_matches('/');
    let url = url.strip_suffix("/v1/models").unwrap_or(url);
    (!url.is_empty()).then(|| url.trim_end_matches('/').to_string())
}

/// `qwen_base_from_config` over `<fleet root>/tools/fleet/config.json`; None when the file is
/// missing, unreadable or not JSON.
pub fn qwen_base_url(fleet: &FleetBridge, machine: Option<&str>) -> Option<String> {
    let text = std::fs::read_to_string(config_path(fleet)).ok()?;
    let config: Value = serde_json::from_str(&text).ok()?;
    qwen_base_from_config(&config, machine)
}

/// The adapter for `service`, with its key read through `ctx.env`. A paid service with no key
/// is a refusal that names the service and the variable; nothing is sent.
fn adapter_for(service: &str, ctx: &AnswerContext) -> Result<Box<dyn ChatAdapter>, CoreError> {
    if service == "qwen" {
        let Some(base) = ctx.qwen_base_url else {
            return Err(CoreError::Refused(
                "no qwen server url for this machine in the fleet config".to_string(),
            ));
        };
        return Ok(Box::new(QwenAdapter::new(base, (ctx.transport)())));
    }
    let Some(var) = key_var(service) else {
        return Err(CoreError::Refused(format!(
            "no chat adapter for service {service}"
        )));
    };
    let key = (ctx.env)(var).filter(|k| !k.trim().is_empty());
    let Some(key) = key else {
        return Err(CoreError::Refused(format!(
            "no key for {service}: set {var}"
        )));
    };
    let transport = (ctx.transport)();
    Ok(match service {
        "deepseek" => Box::new(DeepSeekAdapter::new(
            DEEPSEEK_URL,
            &key,
            DEEPSEEK_USD_PER_MILLION.0,
            DEEPSEEK_USD_PER_MILLION.1,
            transport,
        )),
        "chatgpt" => Box::new(ChatGptAdapter::new(OPENAI_URL, &key, transport)),
        "gemini" => Box::new(GeminiAdapter::new(GEMINI_URL, &key, transport)),
        _ => Box::new(ClaudeApiAdapter::new(
            ANTHROPIC_URL,
            &key,
            CLAUDE_API_USD_PER_MILLION.0,
            CLAUDE_API_USD_PER_MILLION.1,
            transport,
        )),
    })
}

/// The thread's earlier user and answer turns since its last move, oldest first, and the
/// system prompt: after a move it carries the packet the incoming model was sent (FD-6). A
/// queued ask that was never answered is left out, so the model sees questions and answers in
/// turn.
fn context(store: &ThreadStore, handoff_id: &str) -> Result<(String, Vec<Turn>), CoreError> {
    let rows = store.read(handoff_id)?;
    let start = rows
        .iter()
        .rposition(|row| row.role.as_deref() == Some(MOVE_ROLE));
    let system = match start {
        Some(at) => moved_system(rows[at].text.as_deref().unwrap_or_default()),
        None => SYSTEM_PROMPT.to_string(),
    };
    let turns = rows
        .into_iter()
        .skip(start.map_or(0, |at| at + 1))
        .filter(is_turn)
        .filter(|row| row.outcome.as_deref() != Some(QUEUED))
        .filter_map(|row| match (row.role, row.text) {
            (Some(role), Some(text)) => Some(Turn { role, text }),
            _ => None,
        })
        .collect();
    Ok((system, turns))
}

/// FD-6: the system prompt of a model a chat moved to: the packet it was sent.
fn moved_system(packet: &str) -> String {
    format!("{SYSTEM_PROMPT}\n\nThis chat moved to you from another model. What it carried:\n\n{packet}")
}

/// FD-6: the file links in `text`, in order: words that start with `http://`, `https://` or
/// `file://`, without trailing punctuation.
pub fn links_in(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| word.trim_end_matches(['.', ',', ';', ':', ')', ']', '>', '"', '\'']))
        .filter(|word| {
            ["http://", "https://", "file://"]
                .iter()
                .any(|scheme| word.starts_with(scheme) && word.len() > scheme.len())
        })
        .map(str::to_string)
        .collect()
}

/// A chat move about to happen: the packet for the incoming model, and the rows to store once
/// the turn is answered (the rewritten summary and the move).
struct Move {
    from: OpenChat,
    packet: Packet,
}

/// FD-6, FD-7: build the packet for moving the chat in `handoff_id` off `from`: every turn of
/// the thread (answered ones; the packet keeps the last 6), every link gathered so far, and the
/// rolling summary the outgoing model rewrites, checked by `packet::checked_summary` (asked once
/// more when it leaves a link out). When the outgoing model cannot be reached, the earlier
/// summary is kept as it is.
fn plan_move(
    store: &ThreadStore,
    handoff_id: &str,
    from: OpenChat,
    message: &str,
    ctx: &AnswerContext,
) -> Result<Move, CoreError> {
    let rows = store.read(handoff_id)?;
    let earlier = rows
        .iter()
        .find(|row| row.role.as_deref() == Some(SUMMARY_ROLE))
        .and_then(|row| row.text.clone())
        .unwrap_or_default();
    let turns: Vec<Turn> = rows
        .iter()
        .filter(|row| is_turn(row) && row.outcome.as_deref() != Some(QUEUED))
        .filter_map(|row| {
            Some(Turn {
                role: row.role.clone()?,
                text: row.text.clone()?,
            })
        })
        .collect();
    let mut links: Vec<String> = turns.iter().flat_map(|t| links_in(&t.text)).collect();
    links.extend(links_in(message));
    let summary = match adapter_for(&from.service, ctx) {
        Ok(outgoing) => {
            let (system, mut asked) = context(store, handoff_id)?;
            let system = format!("{SUMMARY_SYSTEM}\n\n{system}");
            asked.push(Turn {
                role: "user".to_string(),
                text: format!("The summary so far:\n{earlier}\n\nWrite the new summary."),
            });
            match outgoing.send(&from.model, &system, &asked) {
                Ok(first) => {
                    let mut again = |missing: &[String]| -> String {
                        let mut turns = asked.clone();
                        turns.push(Turn {
                            role: "assistant".to_string(),
                            text: first.text.clone(),
                        });
                        turns.push(Turn {
                            role: "user".to_string(),
                            text: format!(
                                "The summary leaves out: {}. Write it again and name them.",
                                missing.join(", ")
                            ),
                        });
                        outgoing
                            .send(&from.model, &system, &turns)
                            .map_or_else(|_| first.text.clone(), |a| a.text)
                    };
                    packet::checked_summary(first.text.clone(), &links, &[], &mut again).0
                }
                Err(_) => earlier,
            }
        }
        Err(_) => earlier,
    };
    Ok(Move {
        packet: packet::build(&summary, &turns, &links),
        from,
    })
}

/// Store a move: the rolling summary (replaced, never stacked), the move row (the packet, under
/// the incoming service and model), and on a thumbs down the rating's `moved` (FD-10).
fn record_move(
    store: &ThreadStore,
    handoff_id: &str,
    planned: &Move,
    to: Option<(&str, &str)>,
) -> Result<(), CoreError> {
    let summary = JobRow {
        id: Some(format!("{handoff_id}#summary")),
        role: Some(SUMMARY_ROLE.to_string()),
        text: Some(planned.packet.summary.clone()),
        lane: Some(planned.from.service.clone()),
        model: Some(planned.from.model.clone()),
        handoff_id: Some(handoff_id.to_string()),
        source: RowSource::Chat,
        ..JobRow::default()
    };
    store.upsert(handoff_id, &summary)?;
    let moved = JobRow {
        id: Some(format!("{handoff_id}#move-{}", planned.from.turn)),
        role: Some(MOVE_ROLE.to_string()),
        text: Some(packet::as_prompt(&planned.packet)),
        lane: to.map(|(service, _)| service.to_string()),
        model: to.map(|(_, model)| model.to_string()),
        handoff_id: Some(handoff_id.to_string()),
        source: RowSource::Chat,
        ..JobRow::default()
    };
    store.append(handoff_id, &moved)?;
    mark_rating(store, handoff_id, &planned.from, true)
}

/// FD-10: set `moved` on the open answer's thumbs-down rating, if it has one.
fn mark_rating(
    store: &ThreadStore,
    handoff_id: &str,
    from: &OpenChat,
    moved: bool,
) -> Result<(), CoreError> {
    if !from.thumbs_down {
        return Ok(());
    }
    let id = rating_id(handoff_id, from.turn);
    let rows = store.read(handoff_id)?;
    match rows
        .into_iter()
        .find(|row| row.id.as_deref() == Some(id.as_str()))
    {
        Some(row) => store.upsert(handoff_id, &with_move(row, moved)),
        None => Ok(()),
    }
}

/// FD-3 (#377): every queued ask still waiting: the threads whose last turn is a user turn
/// marked `queued`, as (handoff id, the ask's row), threads in id order.
pub fn queued_asks(data_dir: &Path) -> Result<Vec<(String, JobRow)>, CoreError> {
    let store = ThreadStore::new(data_dir);
    let mut out = Vec::new();
    for id in store.list()? {
        if let Some(last) = store.turns(&id)?.pop() {
            if last.role.as_deref() == Some("user") && last.outcome.as_deref() == Some(QUEUED) {
                out.push((id, last));
            }
        }
    }
    Ok(out)
}

/// The user turn's row in the thread.
fn user_row(message: &str, kind: Option<&str>, handoff_id: &str) -> JobRow {
    JobRow {
        kind: kind.map(str::to_string),
        handoff_id: Some(handoff_id.to_string()),
        role: Some("user".to_string()),
        text: Some(message.to_string()),
        source: RowSource::Chat,
        ..JobRow::default()
    }
}

/// The answer turn's row: the log row of `log_row` (service, model, tokens, cost) with the text.
fn answer_row(answer: &Answer, kind: Option<&str>, handoff_id: &str) -> JobRow {
    JobRow {
        role: Some("assistant".to_string()),
        text: Some(answer.text.clone()),
        escalation_step: answer.handoff.as_deref().map(handoff_step),
        ..log_row(answer, kind, handoff_id)
    }
}

/// FD-11: answer `message` for `route`. A service pick is sent through its adapter with the
/// thread's earlier turns (`handoff_id`, or a new thread when None); then the user turn and the
/// answer turn are stored. A missing key, a missing qwen URL or a failed call is the error, and
/// nothing is stored. A wait (nothing in budget, FD-3) queues the ask: the user turn is stored
/// marked `queued` and the reply carries the thread and the reason (#377). A Claude handoff
/// answers nothing: the reply is the route alone.
///
/// FD-5, FD-6 (#378): when the thread is open on another service or model, the turn moves the
/// chat: the incoming model is sent the packet (in its system prompt) and this message, never
/// the full transcript; the summary and the move are stored with the turn, and a thumbs down
/// on the open answer gets `moved` (FD-10). A Claude handoff that moves an open chat stores the
/// summary and the move and replies with the packet for "Open in Claude" (FD-8).
pub fn answer_route(
    route: ChatRoute,
    message: &str,
    handoff_id: Option<&str>,
    ctx: &AnswerContext,
) -> Result<ChatReply, CoreError> {
    let empty = |route: ChatRoute| ChatReply {
        route,
        handoff_id: None,
        text: None,
        service: None,
        model: None,
        handoff: None,
        queued: None,
        moved_from: None,
        packet: None,
    };
    if let ChatPick::Wait { reason } = &route.pick {
        let id = handoff_id.map_or_else(new_handoff_id, str::to_string);
        let row = JobRow {
            outcome: Some(QUEUED.to_string()),
            ..user_row(message, route.kind.as_deref(), &id)
        };
        ThreadStore::new(ctx.data_dir).append(&id, &row)?;
        let queued = Some(reason.clone());
        return Ok(ChatReply {
            handoff_id: Some(id),
            queued,
            ..empty(route)
        });
    }
    let store = ThreadStore::new(ctx.data_dir);
    let open = match handoff_id {
        Some(id) => open_chat(&store, id)?,
        None => None,
    };
    let ChatPick::Service { service, model } = &route.pick else {
        // A Claude handoff: nothing is sent to it. In an open chat it is a move.
        let (Some(id), Some(from)) = (handoff_id, open) else {
            return Ok(empty(route));
        };
        let planned = plan_move(&store, id, from, message, ctx)?;
        record_move(&store, id, &planned, None)?;
        let prompt = format!("{}\n\nAsk\n{message}", packet::as_prompt(&planned.packet));
        return Ok(ChatReply {
            handoff_id: Some(id.to_string()),
            moved_from: Some(format!("{} {}", planned.from.service, planned.from.model)),
            packet: Some(prompt),
            ..empty(route)
        });
    };
    let adapter = adapter_for(service, ctx)?;
    let id = handoff_id.map_or_else(new_handoff_id, str::to_string);
    let planned = match open {
        Some(from) if (&from.service, &from.model) != (service, model) => {
            Some(plan_move(&store, &id, from, message, ctx)?)
        }
        _ => None,
    };
    let (system, mut turns) = match &planned {
        Some(planned) => (
            moved_system(&packet::as_prompt(&planned.packet)),
            Vec::new(),
        ),
        None => context(&store, &id)?,
    };
    turns.push(Turn {
        role: "user".to_string(),
        text: message.to_string(),
    });
    let answer = adapter.send(model, &system, &turns)?;
    let kind = route.kind.as_deref();
    if let Some(planned) = &planned {
        record_move(&store, &id, planned, Some((service, model)))?;
    }
    store.append(&id, &user_row(message, kind, &id))?;
    store.append(&id, &answer_row(&answer, kind, &id))?;
    Ok(ChatReply {
        handoff_id: Some(id),
        text: Some(answer.text),
        service: Some(answer.service),
        model: Some(answer.model),
        handoff: answer.handoff,
        queued: None,
        moved_from: planned.map(|p| format!("{} {}", p.from.service, p.from.model)),
        packet: None,
        route,
    })
}
