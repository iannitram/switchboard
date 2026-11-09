//! FD-11: one adapter per service behind one chat interface: local Qwen through llama-swap,
//! Gemini, DeepSeek, the Claude API and ChatGPT. Each logs service, model, tokens and cost.
//! There is no adapter for the Claude subscription: it is reached only through FD-8.

use super::packet::Turn;
use crate::joblog::{JobRow, RowSource};
use crate::CoreError;

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

/// FD-11: the one HTTP call an adapter makes, behind a trait so tests record it. `post_json`
/// sends `body` as JSON to `url` and returns the parsed reply.
pub trait ChatTransport: Send + Sync {
    fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, CoreError>;

    /// As `post_json`, with extra request headers (the key-in-header services need this). A
    /// transport that does not override it refuses the call.
    fn post_json_headers(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        _body: &serde_json::Value,
    ) -> Result<serde_json::Value, CoreError> {
        Err(CoreError::Refused(
            "this transport sends no headers".to_string(),
        ))
    }
}

/// The real transport: a JSON POST over HTTP with the same client the github module uses.
/// A failed call or a reply that is not JSON is `CoreError::Refused` with the reason.
pub struct HttpChatTransport {
    agent: ureq::Agent,
}

impl HttpChatTransport {
    pub fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(300))
                .build(),
        }
    }
}

impl Default for HttpChatTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatTransport for HttpChatTransport {
    fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, CoreError> {
        self.agent
            .post(url)
            .send_json(body.clone())
            .map_err(|e| CoreError::Refused(format!("POST {url}: {e}")))?
            .into_json()
            .map_err(|e| CoreError::Refused(format!("POST {url}: {e}")))
    }

    fn post_json_headers(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, CoreError> {
        let mut request = self.agent.post(url);
        for (name, value) in headers {
            request = request.set(name, value);
        }
        request
            .send_json(body.clone())
            .map_err(|e| CoreError::Refused(format!("POST {url}: {e}")))?
            .into_json()
            .map_err(|e| CoreError::Refused(format!("POST {url}: {e}")))
    }
}

/// FD-11: local Qwen through llama-swap's OpenAI-style endpoint on this machine. Local Qwen
/// costs nothing, so every answer's cost is 0.0, not unknown.
pub struct QwenAdapter {
    base_url: String,
    transport: Box<dyn ChatTransport>,
}

impl QwenAdapter {
    /// An adapter posting to `<base_url>/v1/chat/completions` (a trailing `/` is dropped).
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

    fn send(&self, model: &str, system: &str, turns: &[Turn]) -> Result<Answer, CoreError> {
        let mut messages = vec![serde_json::json!({"role": "system", "content": system})];
        for turn in turns {
            messages.push(serde_json::json!({"role": turn.role, "content": turn.text}));
        }
        let body = serde_json::json!({"model": model, "messages": messages});
        let url = format!("{}/v1/chat/completions", self.base_url);
        let reply = self.transport.post_json(&url, &body).map_err(|e| match e {
            CoreError::Refused(why) => CoreError::Refused(why),
            other => CoreError::Refused(other.to_string()),
        })?;
        let Some(content) = reply["choices"][0]["message"]["content"].as_str() else {
            return Err(CoreError::Refused(format!(
                "qwen: no reply content from {url}"
            )));
        };
        let (text, handoff) = split_handoff(content);
        Ok(Answer {
            text,
            service: self.service().to_string(),
            model: model.to_string(),
            tokens_in: reply["usage"]["prompt_tokens"].as_u64(),
            tokens_out: reply["usage"]["completion_tokens"].as_u64(),
            cost_usd: Some(0.0),
            handoff,
        })
    }
}

/// FD-11: DeepSeek through its OpenAI-style endpoint, with the user's key in the
/// `Authorization` header. DeepSeek is a paid pool: the cost is the token counts priced at the
/// USD-per-million rates given to `new`, and None when the reply carries no token counts.
pub struct DeepSeekAdapter {
    base_url: String,
    api_key: String,
    usd_per_million_in: f64,
    usd_per_million_out: f64,
    transport: Box<dyn ChatTransport>,
}

impl DeepSeekAdapter {
    /// An adapter posting to `<base_url>/chat/completions` (a trailing `/` is dropped).
    pub fn new(
        base_url: &str,
        api_key: &str,
        usd_per_million_in: f64,
        usd_per_million_out: f64,
        transport: Box<dyn ChatTransport>,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            usd_per_million_in,
            usd_per_million_out,
            transport,
        }
    }

    /// The text with the api key taken out, so no error reason carries it.
    fn redact(&self, text: &str) -> String {
        if self.api_key.is_empty() {
            text.to_string()
        } else {
            text.replace(&self.api_key, "[key]")
        }
    }
}

impl ChatAdapter for DeepSeekAdapter {
    fn service(&self) -> &'static str {
        "deepseek"
    }

    fn send(&self, model: &str, system: &str, turns: &[Turn]) -> Result<Answer, CoreError> {
        let mut messages = vec![serde_json::json!({"role": "system", "content": system})];
        for turn in turns {
            messages.push(serde_json::json!({"role": turn.role, "content": turn.text}));
        }
        let body = serde_json::json!({"model": model, "messages": messages});
        let url = format!("{}/chat/completions", self.base_url);
        let headers = [(
            "Authorization".to_string(),
            format!("Bearer {}", self.api_key),
        )];
        let reply = self
            .transport
            .post_json_headers(&url, &headers, &body)
            .map_err(|e| match e {
                CoreError::Refused(why) => CoreError::Refused(self.redact(&why)),
                other => CoreError::Refused(self.redact(&other.to_string())),
            })?;
        let Some(content) = reply["choices"][0]["message"]["content"].as_str() else {
            return Err(CoreError::Refused(format!(
                "deepseek: no reply content from {url}"
            )));
        };
        let (text, handoff) = split_handoff(content);
        let tokens_in = reply["usage"]["prompt_tokens"].as_u64();
        let tokens_out = reply["usage"]["completion_tokens"].as_u64();
        let cost_usd = match (tokens_in, tokens_out) {
            (Some(input), Some(output)) => Some(
                input as f64 / 1e6 * self.usd_per_million_in
                    + output as f64 / 1e6 * self.usd_per_million_out,
            ),
            _ => None,
        };
        Ok(Answer {
            text,
            service: self.service().to_string(),
            model: model.to_string(),
            tokens_in,
            tokens_out,
            cost_usd,
            handoff,
        })
    }
}

/// FD-11: ChatGPT through its OpenAI-style endpoint, with the user's key in the `Authorization`
/// header. ChatGPT is a quota that resets, not a pool priced per token, so the cost is None
/// ("not recorded"), never 0.0.
pub struct ChatGptAdapter {
    base_url: String,
    api_key: String,
    transport: Box<dyn ChatTransport>,
}

impl ChatGptAdapter {
    /// An adapter posting to `<base_url>/v1/chat/completions` (a trailing `/` is dropped).
    pub fn new(base_url: &str, api_key: &str, transport: Box<dyn ChatTransport>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            transport,
        }
    }

    /// The text with the api key taken out, so no error reason carries it.
    fn redact(&self, text: &str) -> String {
        if self.api_key.is_empty() {
            text.to_string()
        } else {
            text.replace(&self.api_key, "[key]")
        }
    }
}

impl ChatAdapter for ChatGptAdapter {
    fn service(&self) -> &'static str {
        "chatgpt"
    }

    fn send(&self, model: &str, system: &str, turns: &[Turn]) -> Result<Answer, CoreError> {
        let mut messages = vec![serde_json::json!({"role": "system", "content": system})];
        for turn in turns {
            messages.push(serde_json::json!({"role": turn.role, "content": turn.text}));
        }
        let body = serde_json::json!({"model": model, "messages": messages});
        let url = format!("{}/v1/chat/completions", self.base_url);
        let headers = [(
            "Authorization".to_string(),
            format!("Bearer {}", self.api_key),
        )];
        let reply = self
            .transport
            .post_json_headers(&url, &headers, &body)
            .map_err(|e| match e {
                CoreError::Refused(why) => CoreError::Refused(self.redact(&why)),
                other => CoreError::Refused(self.redact(&other.to_string())),
            })?;
        let Some(content) = reply["choices"][0]["message"]["content"].as_str() else {
            return Err(CoreError::Refused(format!(
                "chatgpt: no reply content from {url}"
            )));
        };
        let (text, handoff) = split_handoff(content);
        Ok(Answer {
            text,
            service: self.service().to_string(),
            model: model.to_string(),
            tokens_in: reply["usage"]["prompt_tokens"].as_u64(),
            tokens_out: reply["usage"]["completion_tokens"].as_u64(),
            cost_usd: None,
            handoff,
        })
    }
}
