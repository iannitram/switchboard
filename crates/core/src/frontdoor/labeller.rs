//! FD-2: the labeller names a message's request kind. Rules first, then (once U8 picks it) a
//! small model in a llama-swap group that never swaps out and never takes the 27B job's slot.
//! It labels; it never answers.

/// Rules: the request kind a message's words name, or `None` when the rules can't tell.
pub fn by_rules(message: &str) -> Option<String> {
    todo!("FD-2")
}

/// The small model, behind a trait so tests stub it; `None` until U8 is answered.
pub trait SmallLabeller: Send + Sync {
    fn label(&self, message: &str, kinds: &[String]) -> Option<String>;
}

/// FD-2: rules, then the small model if there is one.
pub fn label(message: &str, kinds: &[String], small: Option<&dyn SmallLabeller>) -> Option<String> {
    todo!("FD-2")
}
