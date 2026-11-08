//! FD-11: one adapter per service behind one chat interface: local Qwen through llama-swap,
//! Gemini, DeepSeek, the Claude API and ChatGPT. Each logs service, model, tokens and cost.
//! There is no adapter for the Claude subscription: it is reached only through FD-8.

use super::packet::Turn;
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

/// Pull `[[handoff: <reason>]]` out of a reply: the reply without the marker, and the reason.
pub fn split_handoff(reply: &str) -> (String, Option<String>) {
    todo!("FD-11")
}
