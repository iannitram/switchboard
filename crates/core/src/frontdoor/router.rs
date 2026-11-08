//! FD-1, FD-3, FD-4: where a message goes.

use crate::abilities::AbilitiesIndex;
use crate::joblog::JobRow;

/// FD-1: which step picked the model (the chat says it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A trigger word: an agent's name, bound to a model.
    Trigger { agent: String },
    /// An explicit ask matched a `needs` tag ("search the web" -> `web`).
    ExplicitAsk { need: String },
    /// The labeller named the request kind.
    Labeller { kind: String },
    /// The budget and complexity rule.
    BudgetRule,
}

/// FD-1's last step: small, big without tools, big with tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Small,
    BigNoTools,
    BigWithTools,
}

/// Where the message goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// A service and model the core calls through an adapter (FD-11).
    Service { service: String, model: String },
    /// A Claude handoff: "Open in Claude" (FD-8). Holds why.
    ClaudeHandoff(String),
    /// Nothing in budget: the ask waits, and the chat says so.
    Wait(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub step: Step,
    pub kind: Option<String>,
    pub pick: Pick,
}

/// What the router needs to know about budget: `fleet budget allow <lane>` per service.
pub trait BudgetGate {
    fn allows(&self, service: &str) -> bool;
}

/// FD-3: take the first service in the kind's `first` list whose pool is under budget,
/// preferring resetting quotas (claude, chatgpt, gemini) and using paid pools only as overflow.
/// The `claude` subscription is never called: under budget it is a `ClaudeHandoff`; braked, the
/// kind's next service; none left, `Wait`.
pub fn pick_for_kind(index: &AbilitiesIndex, kind: &str, budget: &dyn BudgetGate) -> Pick {
    todo!("FD-3")
}

/// FD-4: the sample size: N starts at 6 and rises by 1 for every 20 rows the kind has logged,
/// to a cap of 30.
pub fn sample_size(rows_logged: usize) -> usize {
    todo!("FD-4")
}

/// FD-4: with N or more rated rows for a model, the cheapest model with at least 80% passes
/// over its last N rated rows (implicit rows not counted); `None` leaves the index's guess.
pub fn learned_pick(kind: &str, rows: &[JobRow]) -> Option<(String, String)> {
    todo!("FD-4")
}

/// FD-1: route one message, stopping at the first step that matches.
pub fn route(
    message: &str,
    index: &AbilitiesIndex,
    budget: &dyn BudgetGate,
    complexity: Complexity,
) -> Route {
    todo!("FD-1")
}
