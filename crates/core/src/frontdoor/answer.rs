//! FD-11, FD-13: answer one routed chat turn. The route's pick is a service and model: build that
//! service's adapter, send the turn with the thread's earlier turns, and store the user turn and
//! the answer turn (with its log row) in the thread. A Claude handoff or a wait is not answered:
//! nothing is sent and nothing is stored (FD-8).
//!
//! The transport and the key lookup are passed in, so a test sends nothing and reads no
//! environment. A key is never logged, stored or returned.

use super::adapters::{
    log_row, Answer, ChatAdapter, ChatGptAdapter, ChatTransport, ClaudeApiAdapter, DeepSeekAdapter,
    GeminiAdapter, QwenAdapter,
};
use super::chat::{ChatPick, ChatRoute};
use super::packet::Turn;
use super::thread::ThreadStore;
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
}

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

/// The thread's earlier user and answer turns, oldest first.
fn history(store: &ThreadStore, handoff_id: &str) -> Result<Vec<Turn>, CoreError> {
    Ok(store
        .read(handoff_id)?
        .into_iter()
        .filter_map(|row| match (row.role, row.text) {
            (Some(role), Some(text)) => Some(Turn { role, text }),
            _ => None,
        })
        .collect())
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
        ..log_row(answer, kind, handoff_id)
    }
}

/// FD-11: answer `message` for `route`. A service pick is sent through its adapter with the
/// thread's earlier turns (`handoff_id`, or a new thread when None); then the user turn and the
/// answer turn are stored. A missing key, a missing qwen URL or a failed call is the error, and
/// nothing is stored. A Claude handoff or a wait answers nothing: the reply is the route alone.
pub fn answer_route(
    route: ChatRoute,
    message: &str,
    handoff_id: Option<&str>,
    ctx: &AnswerContext,
) -> Result<ChatReply, CoreError> {
    let ChatPick::Service { service, model } = &route.pick else {
        return Ok(ChatReply {
            route,
            handoff_id: None,
            text: None,
            service: None,
            model: None,
            handoff: None,
        });
    };
    let adapter = adapter_for(service, ctx)?;
    let store = ThreadStore::new(ctx.data_dir);
    let id = handoff_id.map_or_else(new_handoff_id, str::to_string);
    let mut turns = history(&store, &id)?;
    turns.push(Turn {
        role: "user".to_string(),
        text: message.to_string(),
    });
    let answer = adapter.send(model, SYSTEM_PROMPT, &turns)?;
    let kind = route.kind.as_deref();
    store.append(&id, &user_row(message, kind, &id))?;
    store.append(&id, &answer_row(&answer, kind, &id))?;
    Ok(ChatReply {
        handoff_id: Some(id),
        text: Some(answer.text),
        service: Some(answer.service),
        model: Some(answer.model),
        handoff: answer.handoff,
        route,
    })
}
