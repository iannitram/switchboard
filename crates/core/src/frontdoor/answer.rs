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
use super::chat::{
    claude_label, qwen_failed_line, ChatPick, ChatRoute, Fallback, BRAKE_LIFTED_STEP,
};
use super::packet::{self, Packet, Turn};
use super::rating_log::rating_id;
use super::sticky::{handoff_step, open_chat, signals, with_move, OpenChat, Signal};
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
    /// FD-5 (#378): on a move, why the chat moved: with `moved_from` and where the turn went,
    /// the move's line in the work tree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved_why: Option<String>,
    /// FD-3 (#377): set when the picked pool was braked or over budget and the local Qwen lane
    /// answered instead: why, said plainly. The chat is not moved by it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    /// FD-8 (#378): on a Claude handoff that moves an open chat, the text the new Claude
    /// session starts from: the packet (`packet::as_prompt`), then the ask.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packet: Option<String>,
    /// #433: on a Claude handoff, how its model shows: `Claude <model>`, or
    /// `chat::CLAUDE_MODEL_NOT_CHOSEN` when Switchboard chose none (`chat::claude_label`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_label: Option<String>,
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
    /// FD-5: why the chat moves (the work tree's line).
    why: String,
    /// #428: how many of the thread's turns the summary covers (`JobRow::covers_turns`).
    covers: Option<u32>,
    /// #433: the content hash of those turns (`JobRow::covers_hash`, `turns_hash`).
    covers_hash: Option<String>,
}

/// #433: a stable content hash of `turns` (each one's role, text and outcome), so a cached
/// summary is only reused for the very turns it was written from: 64-bit FNV-1a, as 16 hex
/// digits. Not for security; it only has to change when a turn is edited or removed.
pub fn turns_hash(turns: &[&JobRow]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for turn in turns {
        for field in [&turn.role, &turn.text, &turn.outcome] {
            match field {
                Some(value) => {
                    feed(b"\x01");
                    feed(value.as_bytes());
                }
                None => feed(b"\x00"),
            }
            feed(b"\x1f");
        }
        feed(b"\x1e");
    }
    format!("{hash:016x}")
}

/// The lane a move row names when the chat went to a Claude session (FD-8): the Claude
/// subscription, which has no model the core picks.
pub const CLAUDE_LANE: &str = "claude";
/// The lane a move row names when the chat was sent as a fleet job (FD-12); its model is the
/// request kind.
pub const FLEET_JOB_LANE: &str = "fleet-job";

/// FD-5 (#393, #428): why a chat moved off `service` when no signal moved it: its pool is
/// braked or over budget. A move with this why is moved back once the pool is in budget again.
pub fn braked_why(service: &str) -> String {
    format!("{service} is braked or over budget")
}

/// #428: the model the thread's latest Claude handoff move went to, if it stored one.
pub fn claude_model_of(rows: &[JobRow]) -> Option<String> {
    rows.iter()
        .rev()
        .filter(|row| row.role.as_deref() == Some(MOVE_ROLE))
        .filter(|row| row.lane.as_deref() == Some(CLAUDE_LANE))
        .find_map(|row| row.model.clone())
}

/// FD-5: why an open chat moves with `message` routed as `route`, in the words the work tree
/// shows: each signal that moved it (`ask Claude`, a named agent, a thumbs down, the model's
/// handoff marker with its reason), or, with no signal, its pool being braked (FD-3), or (step
/// `brake lifted`, #428) the pool it left being back in budget.
pub fn move_why(route: &ChatRoute, from: &OpenChat, message: &str) -> String {
    if route.step == BRAKE_LIFTED_STEP {
        if let ChatPick::Service { service, .. } = &route.pick {
            return format!("{service} is back in budget, so the chat moved back");
        }
    }
    let mut why: Vec<String> = Vec::new();
    for signal in signals(message, &[], from.thumbs_down, &from.reply) {
        match signal {
            Signal::AskClaude => why.push("I said \"ask Claude\"".to_string()),
            Signal::NamedAgent(agent) => why.push(format!("I named {agent}")),
            Signal::ThumbsDown => why.push("thumbs down".to_string()),
            Signal::HandoffMarker(reason) => why.push(format!("the model handed off: {reason}")),
        }
    }
    // The router's first step is the trigger word, so a named agent shows in the route's step.
    if let Some(agent) = route.step.strip_prefix("trigger word ") {
        why.insert(
            usize::from(why.first().is_some_and(|w| w.contains("ask Claude"))),
            format!("I named {agent}"),
        );
    }
    if why.is_empty() {
        return braked_why(&from.service);
    }
    why.join("; ")
}

/// FD-7: the open questions the core has logged for a thread: the asks it stored as queued
/// (FD-3: nothing was in budget, so no model answered) that were not asked again and answered
/// since, oldest first, no repeats. Two asks are the same question when they match after
/// trimming, lower-casing and collapsing spaces (`packet::normalised`, #428); no fuzzy matching.
pub fn open_questions(rows: &[JobRow]) -> Vec<String> {
    let mut open: Vec<String> = Vec::new();
    for (at, row) in rows.iter().enumerate() {
        if row.role.as_deref() != Some("user") || row.outcome.as_deref() != Some(QUEUED) {
            continue;
        }
        let Some(text) = row.text.as_ref() else {
            continue;
        };
        let asked = packet::normalised(text);
        let answered_later = rows[at + 1..].iter().any(|later| {
            later.role.as_deref() == Some("user")
                && later.outcome.as_deref() != Some(QUEUED)
                && later.text.as_deref().map(packet::normalised) == Some(asked.clone())
        });
        if !answered_later && !open.iter().any(|q| packet::normalised(q) == asked) {
            open.push(text.clone());
        }
    }
    open
}

/// FD-6, FD-7: build the packet for moving the chat in `handoff_id` off `from`: every turn of
/// the thread (answered ones; the packet keeps the last 6), every link gathered so far, and the
/// rolling summary the outgoing model rewrites, checked by `packet::checked_summary` (asked once
/// more when it leaves out a link or an open question the core logged for the thread,
/// `open_questions`; the outgoing model is told the open questions, since a queued ask is not
/// among the turns it saw). When the outgoing model cannot be reached, the earlier summary is
/// kept as it is.
fn plan_move(
    store: &ThreadStore,
    handoff_id: &str,
    from: OpenChat,
    message: &str,
    why: String,
    ctx: &AnswerContext,
) -> Result<Move, CoreError> {
    let rows = store.read(handoff_id)?;
    let summary_row = rows
        .iter()
        .find(|row| row.role.as_deref() == Some(SUMMARY_ROLE));
    let earlier = summary_row
        .and_then(|row| row.text.clone())
        .unwrap_or_default();
    let stored_covered = summary_row.and_then(|row| row.covers_turns);
    let stored_hash = summary_row.and_then(|row| row.covers_hash.clone());
    let thread_turns: Vec<&JobRow> = rows.iter().filter(|row| is_turn(row)).collect();
    let now = u32::try_from(thread_turns.len()).unwrap_or(u32::MAX);
    // #433: the summary covers its turns only while they are the turns it was written from (the
    // hash of the first `covered` turns matches). A summary that counts turns but has no hash
    // (stored before #433), or whose turns changed, is a miss: it is written again from every
    // turn. A summary stored before #428 (no count) is summarised on top of, as before.
    let unchanged = stored_covered.is_some_and(|covered| {
        thread_turns
            .get(..covered as usize)
            .is_some_and(|turns| stored_hash.as_deref() == Some(turns_hash(turns).as_str()))
    });
    let (from_text, covered) = match stored_covered {
        Some(_) if !unchanged => (String::new(), Some(0)),
        _ => (earlier.clone(), stored_covered),
    };
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
    let questions = open_questions(&rows);
    let rewritten = if unchanged && covered == Some(now) {
        // #428: no new turn since the summary: it is reused, and no model is asked.
        None
    } else {
        rewrite_summary(
            store,
            handoff_id,
            &from,
            &from_text,
            covered,
            &thread_turns,
            &links,
            &questions,
            ctx,
        )?
    };
    // The model could not be reached (or nothing changed): the stored summary is kept as it is.
    let (summary, covers, covers_hash) = match rewritten {
        Some(summary) => (summary, Some(now), Some(turns_hash(&thread_turns))),
        None => (earlier, stored_covered, stored_hash),
    };
    Ok(Move {
        packet: packet::build(&summary, &turns, &links),
        from,
        why,
        covers,
        covers_hash,
    })
}

/// FD-6, FD-7 (#428): the outgoing model rewrites the summary `earlier`, which covers the
/// first `covered` of the thread's turns: it is sent only the turns since (answered ones), or,
/// for a summary stored before #428 or none, the turns since the last move. Returns the new
/// summary, which covers every turn of the thread, or None when the model cannot be reached.
#[allow(clippy::too_many_arguments)]
fn rewrite_summary(
    store: &ThreadStore,
    handoff_id: &str,
    from: &OpenChat,
    earlier: &str,
    covered: Option<u32>,
    thread_turns: &[&JobRow],
    links: &[String],
    questions: &[String],
    ctx: &AnswerContext,
) -> Result<Option<String>, CoreError> {
    let summary = match adapter_for(&from.service, ctx) {
        Ok(outgoing) => {
            let (system, since_move) = context(store, handoff_id)?;
            let mut asked: Vec<Turn> = match covered {
                Some(covered) => thread_turns
                    .get(covered as usize..)
                    .unwrap_or_default()
                    .iter()
                    .filter(|row| row.outcome.as_deref() != Some(QUEUED))
                    .filter_map(|row| {
                        Some(Turn {
                            role: row.role.clone()?,
                            text: row.text.clone()?,
                        })
                    })
                    .collect(),
                None => since_move,
            };
            let system = format!("{SUMMARY_SYSTEM}\n\n{system}");
            let mut ask = format!("The summary so far:\n{earlier}\n\nWrite the new summary.");
            if !questions.is_empty() {
                ask.push_str(
                    "\n\nOpen questions logged for this chat (asked, not answered yet). \
                     Name each one word for word:",
                );
                for question in questions {
                    ask.push_str(&format!("\n- {question}"));
                }
            }
            asked.push(Turn {
                role: "user".to_string(),
                text: ask,
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
                    let checked =
                        packet::checked_summary(first.text.clone(), links, questions, &mut again);
                    Some(checked.0)
                }
                Err(_) => None,
            }
        }
        Err(_) => None,
    };
    Ok(summary)
}

/// Store a move: the rolling summary (replaced, never stacked), the move row, and on a thumbs
/// down the rating's `moved` (FD-10). The move row is the move's line in the work tree (FD-5,
/// REP-2): the packet as its text, where the chat went as its `lane` and `model` (`to`: the
/// incoming service and model, `CLAUDE_LANE` with no model, or `FLEET_JOB_LANE` and the kind),
/// the model it left as `moved_from` and why as `moved_why`.
fn record_move(
    store: &ThreadStore,
    handoff_id: &str,
    planned: &Move,
    to: (&str, Option<&str>),
) -> Result<(), CoreError> {
    let summary = JobRow {
        id: Some(format!("{handoff_id}#summary")),
        role: Some(SUMMARY_ROLE.to_string()),
        text: Some(planned.packet.summary.clone()),
        lane: Some(planned.from.service.clone()),
        model: Some(planned.from.model.clone()),
        handoff_id: Some(handoff_id.to_string()),
        source: RowSource::Chat,
        covers_turns: planned.covers,
        covers_hash: planned.covers_hash.clone(),
        ..JobRow::default()
    };
    store.upsert(handoff_id, &summary)?;
    let moved = JobRow {
        id: Some(format!("{handoff_id}#move-{}", planned.from.turn)),
        role: Some(MOVE_ROLE.to_string()),
        text: Some(packet::as_prompt(&planned.packet)),
        lane: Some(to.0.to_string()),
        model: to.1.map(str::to_string),
        moved_from: Some(moved_from(&planned.from)),
        moved_why: Some(planned.why.clone()),
        handoff_id: Some(handoff_id.to_string()),
        source: RowSource::Chat,
        ..JobRow::default()
    };
    store.append(handoff_id, &moved)?;
    mark_rating(store, handoff_id, &planned.from, true)
}

/// The model a chat moved off, as the reply and the move row name it: `<service> <model>`.
fn moved_from(from: &OpenChat) -> String {
    format!("{} {}", from.service, from.model)
}

/// FD-10: set `moved` on the open answer's thumbs-down rating, if it has one. The rating is
/// changed under the thread's lock (#419), so a press made meanwhile is not written over.
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
    store
        .change(handoff_id, &id, |row| with_move(row, moved))
        .map(|_| ())
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

/// #427: a thumbs down on an answer the local Qwen lane gave as a fallback holds the same ask
/// for Claude: the question before turn `answer_turn` is queued again at the thread's end (the
/// held queue, `queued_asks`), unless it is already the thread's last turn, held. True when it
/// was queued. Sending it stays manual; the chat is not moved.
pub fn hold_fallback_ask(
    store: &ThreadStore,
    handoff_id: &str,
    answer_turn: u32,
) -> Result<bool, CoreError> {
    let turns = store.turns(handoff_id)?;
    let Some(ask) = answer_turn
        .checked_sub(1)
        .and_then(|at| turns.get(at as usize))
        .filter(|row| row.role.as_deref() == Some("user"))
    else {
        return Ok(false);
    };
    let Some(text) = ask.text.as_deref() else {
        return Ok(false);
    };
    let row = JobRow {
        outcome: Some(QUEUED.to_string()),
        ..user_row(text, ask.kind.as_deref(), handoff_id)
    };
    store.append_if(handoff_id, &row, |rows| {
        let last = rows.iter().rev().find(|row| is_turn(row));
        !last.is_some_and(|last| {
            last.role.as_deref() == Some("user")
                && last.outcome.as_deref() == Some(QUEUED)
                && last.text.as_deref() == Some(text)
        })
    })
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
/// nothing is stored. A wait (nothing in budget and no Qwen fallback, FD-3) queues the ask: the
/// user turn is stored marked `queued` and the reply carries the thread and the reason (#377);
/// a queued ask stays manual: nothing sends it later. A Claude handoff answers nothing: the
/// reply is the route alone.
///
/// FD-5, FD-6 (#378): when the thread is open on another service or model, the turn moves the
/// chat: the incoming model is sent the packet (in its system prompt) and this message, never
/// the full transcript; the summary and the move are stored with the turn, and a thumbs down
/// on the open answer gets `moved` (FD-10). A Claude handoff that moves an open chat stores the
/// summary and the move and replies with the packet for "Open in Claude" (FD-8). The reply
/// carries the move's work-tree line: `moved_from`, where the turn went, `moved_why`.
pub fn answer_route(
    route: ChatRoute,
    message: &str,
    handoff_id: Option<&str>,
    ctx: &AnswerContext,
) -> Result<ChatReply, CoreError> {
    answer(route, None, message, handoff_id, ctx)
}

/// FD-3 (#377): `answer_route` for a route the local Qwen lane took because the picked pool
/// was braked or over budget, or Claude's budget was spent (`chat::qwen_fallback_on`): its
/// `reason` says so in the reply (`fallback`) and on the answer's row. It is one answer, not a
/// move: Qwen is sent the thread's turns as any turn is, no summary is asked for, no move is
/// stored, and an open chat stays open on its own model (`sticky::open_chat` passes over a
/// fallback's answer). #427: when Qwen fails mid-answer the ask is held for Claude exactly as
/// if Qwen had been down: the reply is the wait (`fallback.waited`) with the ask queued and
/// `queued` saying Qwen failed mid-answer (`chat::qwen_failed_line`); nothing of the failed
/// answer is kept, and sending the held ask stays manual.
pub fn answer_fallback(
    fallback: Fallback,
    message: &str,
    handoff_id: Option<&str>,
    ctx: &AnswerContext,
) -> Result<ChatReply, CoreError> {
    answer(
        fallback.route,
        Some((&fallback.reason, fallback.waited)),
        message,
        handoff_id,
        ctx,
    )
}

/// FD-3 (#377, #427): queue `message` in its thread (a new one when `handoff_id` is None):
/// the user turn marked `queued`; the reply is `route` with the thread and `why`.
fn hold(
    route: ChatRoute,
    why: String,
    message: &str,
    handoff_id: Option<&str>,
    ctx: &AnswerContext,
) -> Result<ChatReply, CoreError> {
    let id = handoff_id.map_or_else(new_handoff_id, str::to_string);
    let row = JobRow {
        outcome: Some(QUEUED.to_string()),
        ..user_row(message, route.kind.as_deref(), &id)
    };
    ThreadStore::new(ctx.data_dir).append(&id, &row)?;
    Ok(ChatReply {
        handoff_id: Some(id),
        queued: Some(why),
        ..route_alone(route)
    })
}

/// A reply that is the route alone.
pub(crate) fn route_alone(route: ChatRoute) -> ChatReply {
    let claude_label = match &route.pick {
        ChatPick::ClaudeHandoff { model, .. } => Some(claude_label(model.as_deref())),
        _ => None,
    };
    ChatReply {
        claude_label,
        route,
        handoff_id: None,
        text: None,
        service: None,
        model: None,
        handoff: None,
        queued: None,
        moved_from: None,
        moved_why: None,
        fallback: None,
        packet: None,
    }
}

/// `fallback`: the fallback's reason and the route it waited on (#377, #427).
fn answer(
    route: ChatRoute,
    fallback: Option<(&str, ChatRoute)>,
    message: &str,
    handoff_id: Option<&str>,
    ctx: &AnswerContext,
) -> Result<ChatReply, CoreError> {
    if let ChatPick::Wait { reason } = &route.pick {
        let why = reason.clone();
        return hold(route, why, message, handoff_id, ctx);
    }
    let store = ThreadStore::new(ctx.data_dir);
    let open = match handoff_id {
        Some(id) => open_chat(&store, id)?,
        None => None,
    };
    let ChatPick::Service { service, model } = &route.pick else {
        // A Claude handoff: nothing is sent to it. In an open chat it is a move.
        let (Some(id), Some(from)) = (handoff_id, open) else {
            return Ok(route_alone(route));
        };
        // #428, #433: the move stores a Claude model only when one was chosen: the route's
        // (named in the ask or by its kind), else the thread's earlier one; else none.
        let mut route = route;
        let thread_model = claude_model_of(&store.read(id)?);
        if let ChatPick::ClaudeHandoff { model, .. } = &mut route.pick {
            *model = model.take().or(thread_model);
        }
        let to = match &route.pick {
            ChatPick::ClaudeHandoff { model, .. } => model.clone(),
            _ => None,
        };
        let why = move_why(&route, &from, message);
        let planned = plan_move(&store, id, from, message, why, ctx)?;
        record_move(&store, id, &planned, (CLAUDE_LANE, to.as_deref()))?;
        let prompt = format!("{}\n\nAsk\n{message}", packet::as_prompt(&planned.packet));
        return Ok(ChatReply {
            handoff_id: Some(id.to_string()),
            moved_from: Some(moved_from(&planned.from)),
            moved_why: Some(planned.why),
            packet: Some(prompt),
            ..route_alone(route)
        });
    };
    let adapter = adapter_for(service, ctx)?;
    let id = handoff_id.map_or_else(new_handoff_id, str::to_string);
    let planned = match open {
        // A fallback is one answer from Qwen, never a move.
        Some(from) if fallback.is_none() && (&from.service, &from.model) != (service, model) => {
            let why = move_why(&route, &from, message);
            Some(plan_move(&store, &id, from, message, why, ctx)?)
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
    let answer = match adapter.send(model, &system, &turns) {
        Ok(answer) => answer,
        Err(e) => match fallback {
            // #427: Qwen failed mid-answer: held for Claude, as if Qwen had been down.
            Some((_, waited)) => {
                let why = match &waited.pick {
                    ChatPick::Wait { reason } => qwen_failed_line(reason),
                    _ => qwen_failed_line("no answer"),
                };
                return hold(waited, why, message, handoff_id, ctx);
            }
            None => return Err(e),
        },
    };
    let fallback = fallback.map(|(reason, _)| reason);
    let kind = route.kind.as_deref();
    if let Some(planned) = &planned {
        record_move(&store, &id, planned, (service, Some(model)))?;
    }
    // The question and its answer go in under one hold of the thread's lock (#419).
    let answered = JobRow {
        fallback: fallback.map(str::to_string),
        ..answer_row(&answer, kind, &id)
    };
    store.append_all(&id, &[user_row(message, kind, &id), answered])?;
    Ok(ChatReply {
        handoff_id: Some(id),
        text: Some(answer.text),
        service: Some(answer.service),
        model: Some(answer.model),
        handoff: answer.handoff,
        queued: None,
        moved_from: planned.as_ref().map(|p| moved_from(&p.from)),
        moved_why: planned.map(|p| p.why),
        fallback: fallback.map(str::to_string),
        packet: None,
        claude_label: None,
        route,
    })
}

/// FD-6, FD-8, FD-12 (#378): send a chat as a fleet job with its packet. `send` queues the job
/// from the request text it is given and answers the job. A thread open on a model is handed
/// over as any move is: the outgoing model rewrites the rolling summary (checked, FD-7), the
/// job's request is the packet (`packet::as_prompt`: the summary, the last 6 turns, the links)
/// and then the ask, never the full transcript, and once the job is queued the summary and
/// the move are stored (the move's line in the work tree: to `FLEET_JOB_LANE` and the kind,
/// why `why`). A thread with no answer yet has nothing to carry: its request goes as it is.
/// A thread that is not stored is refused before anything is sent.
pub fn send_thread_as_job(
    handoff_id: &str,
    kind: &str,
    request: &str,
    why: &str,
    ctx: &AnswerContext,
    send: &mut dyn FnMut(&str) -> Result<Value, CoreError>,
) -> Result<Value, CoreError> {
    let store = ThreadStore::new(ctx.data_dir);
    if !store.exists(handoff_id)? {
        return Err(CoreError::Refused(format!("no thread {handoff_id}")));
    }
    let Some(from) = open_chat(&store, handoff_id)? else {
        return send(request);
    };
    let planned = plan_move(&store, handoff_id, from, request, why.to_string(), ctx)?;
    let text = format!("{}\n\nAsk\n{request}", packet::as_prompt(&planned.packet));
    let job = send(&text)?;
    record_move(&store, handoff_id, &planned, (FLEET_JOB_LANE, Some(kind)))?;
    Ok(job)
}
