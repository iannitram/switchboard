//! FD-2: the labeller names a message's request kind. Rules first, then (once U8 picks it) a
//! small model in a llama-swap group that never swaps out and never takes the 27B job's slot.
//! It labels; it never answers.

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
