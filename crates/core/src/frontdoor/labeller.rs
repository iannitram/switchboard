//! FD-2: the labeller names a message's request kind. Rules first, then (once U8 picks it) a
//! small model in a llama-swap group that never swaps out and never takes the 27B job's slot.
//! It labels; it never answers.

use super::adapters::ChatTransport;

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

/// The small model, behind a trait so tests stub it; `None` until U8 is answered.
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

impl SmallLabeller for LlamaLabeller {
    fn label(&self, message: &str, kinds: &[String]) -> Option<String> {
        let system = format!(
            "You name the request kind of the user's message. The kinds, one per line:\n{}\n\
             Answer with exactly one id from that list, or `none` if no id fits.",
            kinds.join("\n")
        );
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
        let reply = self.transport.post_json(&url, &body).ok()?;
        let content = reply["choices"][0]["message"]["content"].as_str()?;
        let answer = content.trim().to_lowercase();
        kinds.iter().find(|kind| **kind == answer).cloned()
    }
}
