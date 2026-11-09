//! FD-1, FD-3, FD-4: where a message goes.

use crate::abilities::AbilitiesIndex;
use crate::joblog::{JobRow, RowSource};
use std::collections::BTreeMap;

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
///
/// Rules, in order: an id that is not a request kind waits (`unknown request kind <id>`); pass
/// one walks `first` over the resetting or free services (every service but the paid pools);
/// pass two walks it again over the paid pools (`deepseek`, `claudeapi`). In each pass a
/// service that is braked, or that has no model row in the index, is skipped; `claude` under
/// budget is the handoff. With nothing taken: `waiting for Claude's budget` when `first` lists
/// `claude`, else `waiting for budget: <kind>`. Reads no clock and no file.
pub fn pick_for_kind(index: &AbilitiesIndex, kind: &str, budget: &dyn BudgetGate) -> Pick {
    let Some(ability) = index.get(kind).filter(|a| a.request) else {
        return Pick::Wait(format!("unknown request kind {kind}"));
    };
    let try_service = |service: &str| -> Option<Pick> {
        if !budget.allows(service) {
            return None;
        }
        if service == CLAUDE_SUBSCRIPTION {
            return Some(Pick::ClaudeHandoff(format!(
                "{kind}: the Claude subscription is the kind's pick and under budget"
            )));
        }
        let model = index.model(kind, service)?;
        Some(Pick::Service {
            service: service.to_string(),
            model: model.to_string(),
        })
    };
    for paid in [false, true] {
        let pass = ability
            .first
            .iter()
            .filter(|service| PAID_POOLS.contains(&service.as_str()) == paid);
        for service in pass {
            if let Some(pick) = try_service(service) {
                return pick;
            }
        }
    }
    if ability.first.iter().any(|s| s == CLAUDE_SUBSCRIPTION) {
        Pick::Wait("waiting for Claude's budget".to_string())
    } else {
        Pick::Wait(format!("waiting for budget: {kind}"))
    }
}

/// The Claude subscription's lane: never called by the core, only handed off (FD-8).
const CLAUDE_SUBSCRIPTION: &str = "claude";

/// The paid pools FD-3 uses only as overflow, after every resetting or free service.
const PAID_POOLS: [&str; 2] = ["deepseek", "claudeapi"];

/// FD-4: the sample size: N starts at 6 and rises by 1 for every 20 rows the kind has logged,
/// to a cap of 30.
pub fn sample_size(rows_logged: usize) -> usize {
    (6 + rows_logged / 20).min(30)
}

/// FD-4: with N or more rated rows for a model, the cheapest model with at least 80% passes
/// over its last N rated rows (implicit rows not counted); `None` leaves the index's guess.
///
/// N is `sample_size` of every row logged for `kind` (rated or not, ledger or chat). Only chat
/// rows of `kind` that are rated and not implicit count toward a model's rated rows. Models are
/// keyed by (service, model); a row without a lane or a model cannot be picked and is skipped.
/// A pass is rating "pass" or "up". The mean cost is over those last N rows (None is 0.0);
/// ties on the mean go to the smaller model name, then the smaller service.
pub fn learned_pick(kind: &str, rows: &[JobRow]) -> Option<(String, String)> {
    let logged = rows
        .iter()
        .filter(|row| row.kind.as_deref() == Some(kind))
        .count();
    let n = sample_size(logged);

    let mut groups: BTreeMap<(String, String), Vec<&JobRow>> = BTreeMap::new();
    for row in rows {
        if row.kind.as_deref() != Some(kind)
            || row.rating.is_none()
            || row.implicit
            || row.source != RowSource::Chat
        {
            continue;
        }
        let (Some(lane), Some(model)) = (row.lane.as_ref(), row.model.as_ref()) else {
            continue;
        };
        groups
            .entry((lane.clone(), model.clone()))
            .or_default()
            .push(row);
    }

    let mut candidates: Vec<(f64, String, String)> = Vec::new();
    for ((lane, model), list) in &groups {
        if list.len() < n {
            continue;
        }
        let last = &list[list.len() - n..];
        let passes = last
            .iter()
            .filter(|row| matches!(row.rating.as_deref(), Some("pass") | Some("up")))
            .count();
        // passes / n >= 80%, kept in whole numbers.
        if passes * 5 < n * 4 {
            continue;
        }
        let total: f64 = last.iter().map(|row| row.cost_usd.unwrap_or(0.0)).sum();
        let mean = total / n as f64;
        candidates.push((mean, model.clone(), lane.clone()));
    }

    candidates
        .into_iter()
        .min_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
        })
        .map(|(_, model, lane)| (lane, model))
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
