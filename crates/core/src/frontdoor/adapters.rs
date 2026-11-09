//! FD-11: one adapter per service behind one chat interface: local Qwen through llama-swap,
//! Gemini, DeepSeek, the Claude API and ChatGPT. Each logs service, model, tokens and cost.
//! There is no adapter for the Claude subscription: it is reached only through FD-8.

use super::packet::Turn;
use crate::joblog::{JobRow, RowSource};
use crate::CoreError;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub text: String,
    pub service: String,
    pub model: String,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub cost_usd: Option<f64>,
    /// The reason in `[[handoff: <reason>]]`, if the reply carried one.
    pub handoff: Option<String>,
}

pub trait ChatAdapter: Send + Sync {
    /// The service id, as the abilities index and the ledger name it (`qwen`, `gemini`,
    /// `deepseek`, `claudeapi`, `chatgpt`).
    fn service(&self) -> &'static str;
    fn send(&self, model: &str, system: &str, turns: &[Turn]) -> Result<Answer, CoreError>;
}

const HANDOFF_OPEN: &str = "[[handoff:";
const HANDOFF_CLOSE: &str = "]]";

/// FD-11: the reply without its `[[handoff: <reason>]]` marker (trailing space trimmed), and the
/// reason. No marker: the reply as is and None. Only the last marker counts; the text is not
/// otherwise changed. A marker with no closing `]]` is left in the text.
pub fn split_handoff(reply: &str) -> (String, Option<String>) {
    let mut kept = String::new();
    let mut reason = None;
    let mut found = false;
    let mut rest = reply;
    while let Some(open) = rest.find(HANDOFF_OPEN) {
        let after = &rest[open + HANDOFF_OPEN.len()..];
        let Some(close) = after.find(HANDOFF_CLOSE) else {
            break;
        };
        kept.push_str(&rest[..open]);
        reason = Some(after[..close].trim().to_string());
        found = true;
        rest = &after[close + HANDOFF_CLOSE.len()..];
    }
    if !found {
        return (reply.to_string(), None);
    }
    kept.push_str(rest);
    (kept.trim_end_matches(' ').to_string(), reason)
}

/// FD-11: the job-log row for an answer: lane = service, model, tokens_in, tokens_out, cost_usd,
/// kind and handoff_id from the arguments. Tokens are their sum when both are known. Any field
/// the answer lacks stays `None` ("not recorded", CORE-4).
pub fn log_row(answer: &Answer, kind: Option<&str>, handoff_id: &str) -> JobRow {
    let tokens = match (answer.tokens_in, answer.tokens_out) {
        (Some(input), Some(output)) => input.checked_add(output),
        _ => None,
    };
    JobRow {
        lane: Some(answer.service.clone()),
        model: Some(answer.model.clone()),
        tokens,
        tokens_in: answer.tokens_in,
        tokens_out: answer.tokens_out,
        cost_usd: answer.cost_usd,
        kind: kind.map(str::to_string),
        handoff_id: Some(handoff_id.to_string()),
        source: RowSource::Chat,
        ..JobRow::default()
    }
}

/// FD-11: the one HTTP call the Qwen adapter makes, behind a trait so tests record it. `url` is
/// the full address; the reply is the parsed JSON body. A failed call is an error, not a reply.
pub trait ChatTransport: Send + Sync {
    fn post_json(&self, url: &str, body: &Value) -> Result<Value, CoreError>;
}

/// FD-11: local Qwen through llama-swap's OpenAI-style endpoint. It runs on this machine, so its
/// cost is 0.0, not unknown.
pub struct QwenAdapter {
    base_url: String,
    transport: Box<dyn ChatTransport>,
}

impl QwenAdapter {
    pub fn new(base_url: &str, transport: Box<dyn ChatTransport>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            transport,
        }
    }
}

impl ChatAdapter for QwenAdapter {
    fn service(&self) -> &'static str {
        "qwen"
    }

    /// Posts `{model, messages: [system, ...turns]}` to `<base_url>/v1/chat/completions`. A
    /// transport error, or a reply with no `choices[0].message.content`, is `Refused` with the
    /// reason. Tokens come from `usage` and are None when it is missing.
    fn send(&self, model: &str, system: &str, turns: &[Turn]) -> Result<Answer, CoreError> {
        let mut messages = vec![json!({"role": "system", "content": system})];
        messages.extend(turns.iter().map(|turn| json!({"role": turn.role, "content": turn.text})));
        let body = json!({"model": model, "messages": messages});
        let url = format!("{}/v1/chat/completions", self.base_url);
        let reply = self
            .transport
            .post_json(&url, &body)
            .map_err(|e| CoreError::Refused(format!("llama-swap call failed: {e}")))?;
        let content = reply["choices"][0]["message"]["content"]
            .as_str()
            .filter(|content| !content.is_empty())
            .ok_or_else(|| {
                CoreError::Refused("llama-swap reply has no choices[0].message.content".to_string())
            })?;
        let (text, handoff) = split_handoff(content);
        Ok(Answer {
            text,
            service: "qwen".to_string(),
            model: model.to_string(),
            tokens_in: reply["usage"]["prompt_tokens"].as_u64(),
            tokens_out: reply["usage"]["completion_tokens"].as_u64(),
            cost_usd: Some(0.0),
            handoff,
        })
    }
}

/// A test double for `ChatAdapter`: `send` returns a clone of `answer` and ignores its input.
/// `service` is what `service()` reports.
pub struct StubAdapter {
    pub service: &'static str,
    pub answer: Answer,
}

impl ChatAdapter for StubAdapter {
    fn service(&self) -> &'static str {
        self.service
    }

    fn send(&self, _model: &str, _system: &str, _turns: &[Turn]) -> Result<Answer, CoreError> {
        Ok(self.answer.clone())
    }
}
