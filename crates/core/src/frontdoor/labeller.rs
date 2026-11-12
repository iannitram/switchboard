//! FD-2: the labeller names a message's request kind. Rules first, then the local model.
//! A failed local request tries the smallest paid model with a key and budget (#330).
//! It labels; it never answers.

use super::adapters::{ChatAdapter, ChatTransport};
use super::answer::{key_var, paid_adapter_for};
use super::packet::Turn;
use super::router::{any_model, BudgetGate, CHEAPEST_FIRST};
use crate::abilities::AbilitiesIndex;
use crate::CoreError;

/// The rule table: a lowercase phrase and the request kind it names. A phrase matches only as
/// whole words (case and punctuation ignored, so "trade-off" matches "Trade-off."). When more
/// than one row matches, the first row in this table wins. Add a kind by adding a row.
const RULES: &[(&str, &str)] = &[
    ("search the web", "web-research"),
    ("web search", "web-research"),
    ("proofread", "writing-editing"),
    ("rewrite", "writing-editing"),
    ("should we", "judgement-planning"),
    ("trade-off", "judgement-planning"),
    ("define", "quick-lookup"),
    ("what is", "quick-lookup"),
    ("rename", "small-code-change-exact-brief"),
];

/// The words of `text`, lowercased and joined by single spaces (every run of other characters
/// is a separator).
fn words(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `text` as whole words, padded with a space on each side so a phrase found by `contains`
/// starts and ends on a word boundary.
fn padded(text: &str) -> String {
    format!(" {} ", words(text))
}

/// Rules: the request kind a message's words name, or `None` when the rules can't tell.
pub fn by_rules(message: &str) -> Option<String> {
    let haystack = padded(message);
    RULES
        .iter()
        .find(|rule| haystack.contains(&padded(rule.0)))
        .map(|rule| rule.1.to_string())
}

/// A model that labels, behind a trait so tests stub it.
pub trait SmallLabeller: Send + Sync {
    fn label(&self, message: &str, kinds: &[String]) -> Option<String>;
}

/// FD-2: rules, then the small model if there is one. The small model is asked only when the
/// rules name nothing. A kind not in `kinds` (the abilities index's request kinds) is dropped.
pub fn label(message: &str, kinds: &[String], small: Option<&dyn SmallLabeller>) -> Option<String> {
    by_rules(message)
        .or_else(|| small.and_then(|model| model.label(message, kinds)))
        .filter(|kind| kinds.contains(kind))
}

/// FD-2: the small model in its own llama-swap group (the group that never swaps out). `model`
/// is that group's entry name, passed in (U8 picks it). It is asked for one kind id, or `none`,
/// and its answer is kept only when it equals a kind; a transport error or an unclear reply is
/// None. It labels; it never answers the message.
pub struct LlamaLabeller {
    base_url: String,
    model: String,
    transport: Box<dyn ChatTransport>,
}

impl LlamaLabeller {
    /// A labeller posting to `<base_url>/v1/chat/completions` (a trailing `/` is dropped).
    pub fn new(base_url: &str, model: &str, transport: Box<dyn ChatTransport>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            transport,
        }
    }
}

impl LlamaLabeller {
    // Keep a failed request distinct from a successful reply of `none`: only a failure
    // should spend paid budget trying the second labeller.
    fn try_label(&self, message: &str, kinds: &[String]) -> Result<Option<String>, CoreError> {
        let system = prompt(kinds);
        let body = serde_json::json!({
            "model": self.model,
            "temperature": 0,
            "max_tokens": 16,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": message},
            ],
        });
        let url = format!("{}/v1/chat/completions", self.base_url);
        let reply = self.transport.post_json(&url, &body)?;
        let content = reply["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| CoreError::Refused("labeller: no reply content".to_string()))?;
        Ok(known_kind(content, kinds))
    }
}

impl SmallLabeller for LlamaLabeller {
    fn label(&self, message: &str, kinds: &[String]) -> Option<String> {
        self.try_label(message, kinds).ok().flatten()
    }
}

fn prompt(kinds: &[String]) -> String {
    format!(
        "You name the request kind of the user's message. The kinds, one per line:\n{}\n\
         Answer with exactly one id from that list, or `none` if no id fits.",
        kinds.join("\n")
    )
}

fn known_kind(content: &str, kinds: &[String]) -> Option<String> {
    let answer = content.trim().to_lowercase();
    kinds.iter().find(|kind| **kind == answer).cloned()
}

/// #330: a second labeller, using the same small-request order, model rows, budget gate
/// and paid adapters as the front door. No key or no budget means no paid request.
pub struct PaidLabeller {
    adapter: Box<dyn ChatAdapter>,
    model: String,
}

impl PaidLabeller {
    pub fn new(
        index: &AbilitiesIndex,
        budget: &dyn BudgetGate,
        env: &dyn Fn(&str) -> Option<String>,
        transport: &dyn Fn() -> Box<dyn ChatTransport>,
    ) -> Option<Self> {
        for service in CHEAPEST_FIRST
            .into_iter()
            .filter(|service| *service != "qwen")
        {
            let Some(model) = any_model(index, service, "") else {
                continue;
            };
            let Some(key) = key_var(service)
                .and_then(env)
                .filter(|k| !k.trim().is_empty())
            else {
                continue;
            };
            if !budget.allows(service) {
                continue;
            }
            // Use the key already checked, rather than looking it up a second time.
            let adapter = paid_adapter_for(service, &|_| Some(key.clone()), transport).ok()?;
            return Some(Self { adapter, model });
        }
        None
    }
}

impl SmallLabeller for PaidLabeller {
    fn label(&self, message: &str, kinds: &[String]) -> Option<String> {
        let reply = self
            .adapter
            .send(
                &self.model,
                &prompt(kinds),
                &[Turn {
                    role: "user".to_string(),
                    text: message.to_string(),
                }],
            )
            .ok()?;
        known_kind(&reply.text, kinds)
    }
}

/// Local first; construct and ask the paid labeller only after a failed local request.
pub struct FallbackLabeller<F> {
    local: LlamaLabeller,
    paid: F,
}

impl<F> FallbackLabeller<F> {
    pub fn new(local: LlamaLabeller, paid: F) -> Self {
        Self { local, paid }
    }
}

impl<F: Fn() -> Option<PaidLabeller> + Send + Sync> SmallLabeller for FallbackLabeller<F> {
    fn label(&self, message: &str, kinds: &[String]) -> Option<String> {
        match self.local.try_label(message, kinds) {
            Ok(kind) => kind,
            Err(_) => (self.paid)().and_then(|paid| paid.label(message, kinds)),
        }
    }
}
