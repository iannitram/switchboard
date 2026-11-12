//! FD-1 for the Chat composer (#177): route one message and say which step picked; FD-5: in an
//! open thread, keep the chat on its model until a signal moves it. The labeller may call a
//! model for the kind; no answer is sent, nothing is logged and no thread is stored.

use crate::abilities::{AbilitiesIndex, FleetModels};
use crate::autopilot::may_spend;
use crate::budget::{self, PoolRow};
use crate::fleet::FleetBridge;
use crate::frontdoor::adapters::HttpChatTransport;
use crate::frontdoor::answer::{braked_why, claude_model_of, MOVE_ROLE};
use crate::frontdoor::labeller::{FallbackLabeller, LlamaLabeller, PaidLabeller, SmallLabeller};
use crate::frontdoor::router::{self, BudgetGate, Complexity, Pick, Route, Step};
use crate::frontdoor::sticky::{leaves_the_model, move_signals, open_chat, OpenChat, STICKY_STEP};
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
    Service {
        service: String,
        model: String,
    },
    /// `model`: the Claude tier the handoff goes to (#428, #433), only when one was chosen: the
    /// tier the message names, the kind's `ok.claude` tier, or the thread's earlier Claude
    /// model. None: Switchboard chose no model (`CLAUDE_MODEL_NOT_CHOSEN`).
    ClaudeHandoff {
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    Wait {
        reason: String,
    },
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
                model: None,
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
    let gate = FleetGate::new(fleet);
    let routed = route_message_gated(fleet, machine, message, complexity, &gate, route);
    match gate.into_error() {
        Some(err) => Err(err),
        None => routed,
    }
}

/// `route_message_on` asking `gate` which pools are in budget (a dry run's gate reads the
/// budget snapshot, #430). The caller takes the gate's error.
fn route_message_gated(
    fleet: &FleetBridge,
    machine: Option<&str>,
    message: &str,
    complexity: Complexity,
    gate: &dyn BudgetGate,
    route: impl Fn(&str, &AbilitiesIndex, &dyn BudgetGate, Complexity) -> Route,
) -> Result<ChatRoute, CoreError> {
    if message.trim().is_empty() {
        return Err(CoreError::Refused("chat: empty message".to_string()));
    }
    let index = AbilitiesIndex::load_with(fleet, machine)?;
    let picked = route(message, &index, gate, complexity);
    Ok(with_claude_model(
        ChatRoute::from(&picked),
        &index,
        message,
        None,
    ))
}

/// #430: a budget gate over the snapshot on disk (`budget::read`, `fleet budget --json`),
/// which never refreshes a reading: a pool is in budget when its verdict is `push` or
/// `on-pace`; a pool the snapshot does not list is not. A dry run routes with it.
///
/// #433: it mirrors `fleet budget allow <lane>` as the real ask calls it (no `--machine`): the
/// local Qwen lane has no paid pool and is allowed whatever its snapshot verdict says (every
/// Qwen machine busy only brakes a job placed on a machine). It records the pools it was asked
/// about, so a dry run can say which stale auto-read reading a real ask would refresh first
/// (`refresh_lines`).
pub struct SnapshotGate {
    rows: Vec<PoolRow>,
    asked: RefCell<Vec<String>>,
}

/// #433: the pools whose reading `fleet budget allow` refreshes first when it is stale (fleet's
/// `AUTO_POOLS` and its agy-derived pool).
pub const AUTO_READ_POOLS: [&str; 4] = ["gemini", "chatgpt", "deepseek", "agy-claude"];

impl SnapshotGate {
    pub fn new(rows: Vec<PoolRow>) -> Self {
        Self {
            rows,
            asked: RefCell::new(Vec::new()),
        }
    }

    /// Read the snapshot. A fleet failure is the error.
    pub fn read(fleet: &FleetBridge) -> Result<Self, CoreError> {
        Ok(Self::new(budget::read(fleet)?))
    }

    /// When any reading in the snapshot is stale, how old: `stale (<pool> <age> min old, ...)`,
    /// pools in name order; an age the snapshot does not give is `age not recorded`.
    pub fn stale_line(&self) -> Option<String> {
        let stale: Vec<String> = self
            .rows
            .iter()
            .filter(|row| row.stale)
            .map(|row| match row.reading_age_min {
                Some(age) => format!("{} {age} min old", row.pool),
                None => format!("{} age not recorded", row.pool),
            })
            .collect();
        (!stale.is_empty()).then(|| format!("stale ({})", stale.join(", ")))
    }

    /// #433: one line per auto-read pool this gate was asked about whose reading is stale, in
    /// the order asked: `a real ask would refresh <pool> first; this answer uses the reading
    /// from <N> min ago`.
    pub fn refresh_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for pool in self.asked.borrow().iter() {
            let Some(row) = self.rows.iter().find(|row| &row.pool == pool) else {
                continue;
            };
            if !row.stale || !AUTO_READ_POOLS.contains(&pool.as_str()) {
                continue;
            }
            let age = match row.reading_age_min {
                Some(age) => format!("from {age} min ago"),
                None => "of unrecorded age".to_string(),
            };
            lines.push(format!(
                "a real ask would refresh {pool} first; this answer uses the reading {age}"
            ));
        }
        lines
    }
}

impl BudgetGate for SnapshotGate {
    fn allows(&self, service: &str) -> bool {
        {
            let mut asked = self.asked.borrow_mut();
            if !asked.iter().any(|pool| pool == service) {
                asked.push(service.to_string());
            }
        }
        if service == QWEN {
            return true;
        }
        self.rows
            .iter()
            .any(|row| row.pool == service && matches!(row.verdict.as_str(), "push" | "on-pace"))
    }
}

/// FD-5 (#428, #433): the label a Claude handoff's model shows as, in the CLI's text, the reply
/// and the app: `Claude <model>`, or this when Switchboard chose none.
pub const CLAUDE_MODEL_NOT_CHOSEN: &str = "Claude (model not chosen by Switchboard)";

/// #433: how a Claude handoff to `model` is shown.
pub fn claude_label(model: Option<&str>) -> String {
    match model {
        Some(model) => format!("Claude {model}"),
        None => CLAUDE_MODEL_NOT_CHOSEN.to_string(),
    }
}

/// #433: the Claude tier `message` names: a tier word (`haiku`, `sonnet`, `opus`) right after
/// the word `claude` (`ask Claude Opus`, `claude-sonnet`), the first such pair; case-insensitive,
/// whole words. A tier word alone (`write a sonnet`) names no model.
pub fn named_claude_tier(message: &str) -> Option<String> {
    let words: Vec<String> = message
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    words
        .windows(2)
        .find(|pair| pair[0] == "claude" && matches!(pair[1].as_str(), "haiku" | "sonnet" | "opus"))
        .map(|pair| pair[1].clone())
}

/// #428, #433: a Claude handoff route with its model filled in only when one was chosen: the
/// tier `message` names (`named_claude_tier`), else a model the route already carries, else the
/// route kind's `ok.claude` tier, else `thread_model` (the model the thread's earlier Claude
/// handoff went to); None when nothing names one. Any other route is returned as it is.
pub(crate) fn with_claude_model(
    mut route: ChatRoute,
    index: &AbilitiesIndex,
    message: &str,
    thread_model: Option<String>,
) -> ChatRoute {
    if let ChatPick::ClaudeHandoff { model, .. } = &mut route.pick {
        let tier = route
            .kind
            .as_deref()
            .and_then(|kind| index.claude_tiers.get(kind).cloned());
        *model = named_claude_tier(message)
            .or_else(|| model.take())
            .or(tier)
            .or(thread_model);
    }
    route
}

/// FD-5 (#428): the step a route shows when a chat moved off a braked pool goes back to it.
pub const BRAKE_LIFTED_STEP: &str = "brake lifted";

/// FD-5 (#428): the models a chat open on `open` moved off because their pool was braked
/// (`answer::braked_why`), home first: the trailing run of brake moves that ends on the open
/// model, each move's `moved_from` split into service and model. Empty when the chat did not
/// get here by a brake move.
fn brake_homes(rows: &[crate::joblog::JobRow], open: &OpenChat) -> Vec<(String, String)> {
    let mut homes = Vec::new();
    let mut at = (open.service.clone(), open.model.clone());
    for row in rows
        .iter()
        .rev()
        .filter(|row| row.role.as_deref() == Some(MOVE_ROLE))
    {
        let to = (
            row.lane.clone().unwrap_or_default(),
            row.model.clone().unwrap_or_default(),
        );
        let Some((service, model)) = row
            .moved_from
            .as_deref()
            .and_then(|from| from.split_once(' '))
        else {
            break;
        };
        if to != at || row.moved_why.as_deref() != Some(braked_why(service).as_str()) {
            break;
        }
        at = (service.to_string(), model.to_string());
        homes.push(at.clone());
    }
    homes.reverse();
    homes
}

/// How long each labeller HTTP request may take (a slow model must not hold up Chat
/// indefinitely). A failed local request can then try the paid labeller (#330).
pub const LABELLER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// The local labeller for `machine`, also used alone by a dry run so it cannot spend money.
/// Its local qwen model is on the server the fleet config names, over
/// a transport that gives up after `LABELLER_TIMEOUT`. `None` without a model and a URL (rules
/// only). A server that cannot be reached makes the labeller answer `None`, never an error.
pub fn local_labeller(fleet: &FleetBridge, machine: Option<&str>) -> Option<LlamaLabeller> {
    let (model, base_url) = FleetModels::read_qwen_endpoint(fleet, machine)?;
    Some(LlamaLabeller::new(
        &base_url,
        &model,
        Box::new(HttpChatTransport::with_timeout(LABELLER_TIMEOUT)),
    ))
}

/// #330: try the smallest paid model with budget and a key only if the local request fails.
/// Index or budget failures leave the rules-only result, just like missing keys or budget.
pub fn live_labeller<'a>(
    fleet: &'a FleetBridge,
    machine: Option<&str>,
) -> Option<impl SmallLabeller + 'a> {
    let local = local_labeller(fleet, machine)?;
    let machine = machine.map(str::to_string);
    Some(FallbackLabeller::new(local, move || {
        let index = AbilitiesIndex::load_with(fleet, machine.as_deref()).ok()?;
        PaidLabeller::new(
            &index,
            &FleetGate::new(fleet),
            &|name| std::env::var(name).ok(),
            &|| Box::new(HttpChatTransport::with_timeout(LABELLER_TIMEOUT)),
        )
    }))
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
///
/// #428: a chat that moved off a braked pool (#393) moves back when that pool is in budget
/// again and no signal moves it: step `brake lifted`, the thread's kind, the model it left
/// (`brake_homes`; the first in budget, home first), and `route` is not asked. A Claude handoff
/// carries its model (`with_claude_model`, the thread's earlier Claude model first).
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
    let gate = FleetGate::new(fleet);
    let routed = route_in_thread_gated(
        fleet, machine, data_dir, handoff_id, message, complexity, &gate, route,
    );
    match gate.into_error() {
        Some(err) => Err(err),
        None => routed,
    }
}

/// `route_in_thread_on` asking `gate` which pools are in budget (a dry run's gate reads the
/// budget snapshot, #430). The caller takes the gate's error.
#[allow(clippy::too_many_arguments)]
pub(crate) fn route_in_thread_gated(
    fleet: &FleetBridge,
    machine: Option<&str>,
    data_dir: &Path,
    handoff_id: Option<&str>,
    message: &str,
    complexity: Complexity,
    gate: &dyn BudgetGate,
    route: impl Fn(&str, &AbilitiesIndex, &dyn BudgetGate, Complexity) -> Route,
) -> Result<ChatRoute, CoreError> {
    let open = match handoff_id {
        Some(id) => open_chat(&ThreadStore::new(data_dir), id)?,
        None => None,
    };
    let Some(open) = open else {
        return route_message_gated(fleet, machine, message, complexity, gate, route);
    };
    let store = ThreadStore::new(data_dir);
    let rows = store.read(handoff_id.unwrap_or_default())?;
    if message.trim().is_empty() {
        return Err(CoreError::Refused("chat: empty message".to_string()));
    }
    let index = AbilitiesIndex::load_with(fleet, machine)?;
    let mut agents: Vec<String> = index.agents.keys().cloned().collect();
    agents.extend(router::CLAUDE_AGENTS.iter().map(|a| a.to_string()));
    let found = move_signals(&open, message, &agents);
    // #428: a chat that moved off a braked pool moves back once that pool is in budget again.
    let home = if found.is_empty() {
        brake_homes(&rows, &open)
            .into_iter()
            .find(|(service, _)| *service != open.service && gate.allows(service))
    } else {
        None
    };
    let chat = if let Some((service, model)) = home {
        ChatRoute {
            step: BRAKE_LIFTED_STEP.to_string(),
            kind: open.kind.clone(),
            pick: ChatPick::Service { service, model },
        }
    } else if found.is_empty() && gate.allows(&open.service) {
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
            gate,
            service: leaves_the_model(&found).then_some(open.service.as_str()),
        };
        let picked = ChatRoute::from(&route(message, &index, &without, complexity));
        with_claude_model(picked, &index, message, claude_model_of(&rows))
    };
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

/// FD-3 (#377): the local Qwen lane's answer to a wait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallback {
    /// The waited route with the local Qwen model as its pick; its step and kind are kept.
    pub route: ChatRoute,
    /// Why Qwen answers, said plainly: what the route waited for.
    pub reason: String,
    /// The route as the router left it (the wait): what the ask is held under when Qwen fails
    /// mid-answer (#427).
    pub waited: ChatRoute,
}

/// FD-3 (#377, #427): an ask that waits (its picked pool braked or over budget, or waiting for
/// Claude's budget) falls back to the local Qwen lane when that lane is up and its request kind
/// has a Qwen row in the routing table; otherwise it is queued for Claude. `waited` is the route
/// as the router left it; anything but a wait is None (the route stands). A route with no kind,
/// an unknown kind and a kind with no Qwen row are not answered by Qwen (#427). The Qwen lane is
/// up when the fleet config names this machine's Qwen model and server, `fleet budget allow
/// qwen` allows it, and the server answers `qwen_up` (asked with its `qwen_server_url`, as
/// Autopilot's probe is). A fleet error from the budget check is the error.
pub fn qwen_fallback_on(
    fleet: &FleetBridge,
    machine: Option<&str>,
    waited: &ChatRoute,
    qwen_up: &dyn Fn(&str) -> bool,
) -> Result<Option<Fallback>, CoreError> {
    let gate = FleetGate::new(fleet);
    let fallback = qwen_fallback_gated(fleet, machine, waited, &gate, qwen_up);
    match gate.into_error() {
        Some(err) => Err(err),
        None => fallback,
    }
}

/// `qwen_fallback_on` asking `gate` whether the Qwen lane may spend (a dry run's gate reads the
/// budget snapshot, #430). The caller takes the gate's error.
pub(crate) fn qwen_fallback_gated(
    fleet: &FleetBridge,
    machine: Option<&str>,
    waited: &ChatRoute,
    gate: &dyn BudgetGate,
    qwen_up: &dyn Fn(&str) -> bool,
) -> Result<Option<Fallback>, CoreError> {
    let ChatPick::Wait { reason } = &waited.pick else {
        return Ok(None);
    };
    let Some(kind) = waited.kind.as_deref() else {
        return Ok(None);
    };
    let Some((model, _)) = FleetModels::read_qwen_endpoint(fleet, machine) else {
        return Ok(None);
    };
    let Some(url) = FleetModels::read_qwen_server_url(fleet, machine) else {
        return Ok(None);
    };
    let index = AbilitiesIndex::load_with(fleet, machine)?;
    if index.model(kind, QWEN).is_none() {
        return Ok(None);
    }
    if !gate.allows(QWEN) || !qwen_up(&url) {
        return Ok(None);
    }
    Ok(Some(Fallback {
        route: ChatRoute {
            step: waited.step.clone(),
            kind: waited.kind.clone(),
            pick: ChatPick::Service {
                service: QWEN.to_string(),
                model,
            },
        },
        reason: format!("nothing else is in budget ({reason}), so the local Qwen lane answers"),
        waited: waited.clone(),
    }))
}

/// #427: the line a chat shows when Qwen, answering as the fallback, failed mid-answer: the ask
/// is held (queued) for Claude, as if Qwen had been down. `waited` is the route's wait reason.
pub fn qwen_failed_line(waited: &str) -> String {
    format!("Qwen failed mid-answer, so the ask is held for Claude ({waited})")
}

/// The local lane's service name.
const QWEN: &str = "qwen";
