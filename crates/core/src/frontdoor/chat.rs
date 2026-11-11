//! FD-1 for the Chat composer (#177): route one message and say which step picked; FD-5: in an
//! open thread, keep the chat on its model until a signal moves it. Routing only: no adapter is
//! called, nothing is logged and no thread is stored.

use crate::abilities::{AbilitiesIndex, FleetModels};
use crate::autopilot::may_spend;
use crate::fleet::FleetBridge;
use crate::frontdoor::adapters::HttpChatTransport;
use crate::frontdoor::labeller::{LlamaLabeller, SmallLabeller};
use crate::frontdoor::router::{self, BudgetGate, Complexity, Pick, Route, Step};
use crate::frontdoor::sticky::{leaves_the_model, move_signals, open_chat, STICKY_STEP};
use crate::frontdoor::thread::ThreadStore;
use crate::CoreError;
use serde::Serialize;
use std::cell::RefCell;
use std::path::Path;

/// The complexity a caller with no estimate routes as: a small request, the cheapest pool in
/// budget (FD-1). The app no longer uses it: `chat_route` calls `estimate_complexity`.
pub const DEFAULT_COMPLEXITY: Complexity = Complexity::Small;

/// Phrases that mean work in a repo or with tools, so the budget rule hands the chat to Claude
/// (FD-1, big with tools). Matched as whole words, case and punctuation ignored.
const TOOL_PHRASES: [&str; 18] = [
    "refactor",
    "implement",
    "run the tests",
    "run tests",
    "fix the bug",
    "fix this bug",
    "debug",
    "commit",
    "open a pr",
    "pull request",
    "edit the file",
    "in the repo",
    "in this repo",
    "deploy",
    "build the",
    "write the code",
    "add a test",
    "add tests",
];

/// Phrases that mean a deep ask without tools (FD-1, big without tools), matched like
/// `TOOL_PHRASES`.
const DEEP_PHRASES: [&str; 11] = [
    "explain in detail",
    "in depth",
    "design a",
    "design the",
    "analyse",
    "analyze",
    "compare",
    "write a report",
    "write an essay",
    "write a plan",
    "step by step",
];

/// More words than this make a message a big ask without tools.
const LONG_MESSAGE_WORDS: usize = 60;

/// `text` as lowercase words joined by single spaces, padded so `contains` matches whole words.
fn padded(text: &str) -> String {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    format!(" {} ", words.join(" "))
}

/// Estimate how big a chat message is, for the budget rule (FD-1). A message with a phrase from
/// `TOOL_PHRASES` is `BigWithTools`; else one over `LONG_MESSAGE_WORDS` words or with a phrase
/// from `DEEP_PHRASES` is `BigNoTools`; else `Small`. Tool phrases win over length.
pub fn estimate_complexity(message: &str) -> Complexity {
    let haystack = padded(message);
    let has = |phrases: &[&str]| phrases.iter().any(|p| haystack.contains(&padded(p)));
    if has(&TOOL_PHRASES) {
        Complexity::BigWithTools
    } else if haystack.split_whitespace().count() > LONG_MESSAGE_WORDS || has(&DEEP_PHRASES) {
        Complexity::BigNoTools
    } else {
        Complexity::Small
    }
}

/// What the Chat view shows for one routed message (#177: `{ step, kind, pick }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatRoute {
    /// The line the chat shows: `trigger word <agent>`, `explicit ask <need>`, `labeller`,
    /// `budget rule`.
    pub step: String,
    pub kind: Option<String>,
    pub pick: ChatPick,
}

/// Where the message goes, tagged by `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "to", rename_all = "kebab-case")]
pub enum ChatPick {
    Service { service: String, model: String },
    ClaudeHandoff { reason: String },
    Wait { reason: String },
}

impl From<&Route> for ChatRoute {
    fn from(route: &Route) -> Self {
        let step = match &route.step {
            Step::Trigger { agent } => format!("trigger word {agent}"),
            Step::ExplicitAsk { need } => format!("explicit ask {need}"),
            Step::Labeller { .. } => "labeller".to_string(),
            Step::BudgetRule => "budget rule".to_string(),
        };
        let pick = match &route.pick {
            Pick::Service { service, model } => ChatPick::Service {
                service: service.clone(),
                model: model.clone(),
            },
            Pick::ClaudeHandoff(reason) => ChatPick::ClaudeHandoff {
                reason: reason.clone(),
            },
            Pick::Wait(reason) => ChatPick::Wait {
                reason: reason.clone(),
            },
        };
        Self {
            step,
            kind: route.kind.clone(),
            pick,
        }
    }
}

/// The router's budget gate over `fleet budget allow <service>` (AUTO-1). A fleet failure is
/// never allowed: the gate answers `false` and keeps the first error for the caller.
pub struct FleetGate<'a> {
    fleet: &'a FleetBridge,
    error: RefCell<Option<CoreError>>,
}

impl<'a> FleetGate<'a> {
    pub fn new(fleet: &'a FleetBridge) -> Self {
        Self {
            fleet,
            error: RefCell::new(None),
        }
    }

    /// The first fleet error the gate met, if any.
    pub fn into_error(self) -> Option<CoreError> {
        self.error.into_inner()
    }
}

impl BudgetGate for FleetGate<'_> {
    fn allows(&self, service: &str) -> bool {
        match may_spend(self.fleet, service) {
            Ok(allowed) => allowed,
            Err(err) => {
                let mut slot = self.error.borrow_mut();
                if slot.is_none() {
                    *slot = Some(err);
                }
                false
            }
        }
    }
}

/// `route_message` with the router passed in, so the message, the index, the gate and the
/// complexity can be checked without the real router. A fleet error met by the index read or
/// the gate is the error; a braked pool is just the router's answer. The index takes its qwen
/// and Claude API models for this machine (`FleetModels::local_machine`).
pub fn route_message_with(
    fleet: &FleetBridge,
    message: &str,
    complexity: Complexity,
    route: impl Fn(&str, &AbilitiesIndex, &dyn BudgetGate, Complexity) -> Route,
) -> Result<ChatRoute, CoreError> {
    route_message_on(
        fleet,
        FleetModels::local_machine().as_deref(),
        message,
        complexity,
        route,
    )
}

/// `route_message_with` for the machine the caller names (`None`: no qwen model, the Claude API
/// tiers only), so a test can pick the machine.
pub fn route_message_on(
    fleet: &FleetBridge,
    machine: Option<&str>,
    message: &str,
    complexity: Complexity,
    route: impl Fn(&str, &AbilitiesIndex, &dyn BudgetGate, Complexity) -> Route,
) -> Result<ChatRoute, CoreError> {
    if message.trim().is_empty() {
        return Err(CoreError::Refused("chat: empty message".to_string()));
    }
    let index = AbilitiesIndex::load_with(fleet, machine)?;
    let gate = FleetGate::new(fleet);
    let picked = route(message, &index, &gate, complexity);
    if let Some(err) = gate.into_error() {
        return Err(err);
    }
    Ok(ChatRoute::from(&picked))
}

/// How long the labeller may take before the route goes on without it (a slow local model must
/// not hold up Chat).
pub const LABELLER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// The labeller for `machine`: its local qwen model on the server the fleet config names, over
/// a transport that gives up after `LABELLER_TIMEOUT`. `None` without a model and a URL (rules
/// only). A server that cannot be reached makes the labeller answer `None`, never an error.
pub fn live_labeller(fleet: &FleetBridge, machine: Option<&str>) -> Option<LlamaLabeller> {
    let (model, base_url) = FleetModels::read_qwen_endpoint(fleet, machine)?;
    Some(LlamaLabeller::new(
        &base_url,
        &model,
        Box::new(HttpChatTransport::with_timeout(LABELLER_TIMEOUT)),
    ))
}

/// FD-1: route one Chat message with the real router (#169), the live labeller behind the
/// rules (#293).
pub fn route_message(
    fleet: &FleetBridge,
    message: &str,
    complexity: Complexity,
) -> Result<ChatRoute, CoreError> {
    let machine = FleetModels::local_machine();
    let small = live_labeller(fleet, machine.as_deref());
    route_message_on(
        fleet,
        machine.as_deref(),
        message,
        complexity,
        |message, index, gate, complexity| {
            router::route(
                message,
                index,
                gate,
                complexity,
                small.as_ref().map(|s| s as &dyn SmallLabeller),
            )
        },
    )
}

/// A gate that refuses one service and asks `gate` for every other (FD-5: a thumbs down or a
/// handoff marker moves the chat off its model).
struct Without<'a> {
    gate: &'a dyn BudgetGate,
    service: Option<&'a str>,
}

impl BudgetGate for Without<'_> {
    fn allows(&self, service: &str) -> bool {
        self.service != Some(service) && self.gate.allows(service)
    }
}

/// FD-5 (#378): route a message in thread `handoff_id`. While the thread is open on a model
/// (`sticky::open_chat`) and no signal moves it (`sticky::move_signals`, with the index's agents
/// and the built-in Claude agents) and its pool is in budget, the chat stays: step
/// `sticky chat`, the thread's kind, the same service and model, and `route` is not asked.
/// Otherwise `route` picks as for a new chat; after a thumbs down or the model's own handoff
/// marker it may not pick the open service. No thread, or one with no answer, is a new chat.
#[allow(clippy::too_many_arguments)]
pub fn route_in_thread_on(
    fleet: &FleetBridge,
    machine: Option<&str>,
    data_dir: &Path,
    handoff_id: Option<&str>,
    message: &str,
    complexity: Complexity,
    route: impl Fn(&str, &AbilitiesIndex, &dyn BudgetGate, Complexity) -> Route,
) -> Result<ChatRoute, CoreError> {
    let open = match handoff_id {
        Some(id) => open_chat(&ThreadStore::new(data_dir), id)?,
        None => None,
    };
    let Some(open) = open else {
        return route_message_on(fleet, machine, message, complexity, route);
    };
    if message.trim().is_empty() {
        return Err(CoreError::Refused("chat: empty message".to_string()));
    }
    let index = AbilitiesIndex::load_with(fleet, machine)?;
    let mut agents: Vec<String> = index.agents.keys().cloned().collect();
    agents.extend(router::CLAUDE_AGENTS.iter().map(|a| a.to_string()));
    let found = move_signals(&open, message, &agents);
    let gate = FleetGate::new(fleet);
    let chat = if found.is_empty() && gate.allows(&open.service) {
        ChatRoute {
            step: STICKY_STEP.to_string(),
            kind: open.kind.clone(),
            pick: ChatPick::Service {
                service: open.service.clone(),
                model: open.model.clone(),
            },
        }
    } else {
        let without = Without {
            gate: &gate,
            service: leaves_the_model(&found).then_some(open.service.as_str()),
        };
        ChatRoute::from(&route(message, &index, &without, complexity))
    };
    if let Some(err) = gate.into_error() {
        return Err(err);
    }
    Ok(chat)
}

/// FD-5 (#378): `route_in_thread_on` with the real router and the live labeller, as
/// `route_message` routes a new chat.
pub fn route_in_thread(
    fleet: &FleetBridge,
    data_dir: &Path,
    handoff_id: Option<&str>,
    message: &str,
    complexity: Complexity,
) -> Result<ChatRoute, CoreError> {
    let machine = FleetModels::local_machine();
    let small = live_labeller(fleet, machine.as_deref());
    route_in_thread_on(
        fleet,
        machine.as_deref(),
        data_dir,
        handoff_id,
        message,
        complexity,
        |message, index, gate, complexity| {
            router::route(
                message,
                index,
                gate,
                complexity,
                small.as_ref().map(|s| s as &dyn SmallLabeller),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_complexity_table() {
        let long = vec!["word"; 61].join(" ");
        let exactly_sixty = vec!["word"; 60].join(" ");
        let long_with_tools = format!("{long} then commit it");
        let rows: Vec<(&str, Complexity)> = vec![
            (
                "refactor this module and run the tests",
                Complexity::BigWithTools,
            ),
            ("Please DEBUG the parser.", Complexity::BigWithTools),
            ("open a PR for this", Complexity::BigWithTools),
            ("add tests for the router", Complexity::BigWithTools),
            (long_with_tools.as_str(), Complexity::BigWithTools),
            (long.as_str(), Complexity::BigNoTools),
            ("Compare these two approaches", Complexity::BigNoTools),
            ("explain in detail, how it works", Complexity::BigNoTools),
            ("hi", Complexity::Small),
            ("summarize this paragraph", Complexity::Small),
            (exactly_sixty.as_str(), Complexity::Small),
            ("our commitment to quality", Complexity::Small),
            ("", Complexity::Small),
        ];
        for (message, want) in rows {
            assert_eq!(estimate_complexity(message), want, "{message}");
        }
    }
}
