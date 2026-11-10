//! FD-1 for the Chat composer (#177): route one message and say which step picked. Routing only:
//! no adapter is called, nothing is logged and no thread is stored.

use crate::abilities::{AbilitiesIndex, FleetModels};
use crate::autopilot::may_spend;
use crate::fleet::FleetBridge;
use crate::frontdoor::router::{self, BudgetGate, Complexity, Pick, Route, Step};
use crate::CoreError;
use serde::Serialize;
use std::cell::RefCell;

/// The face sends only the message and no issue judges complexity yet, so the chat routes as
/// a small request: the cheapest pool in budget (FD-1).
pub const DEFAULT_COMPLEXITY: Complexity = Complexity::Small;

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

/// FD-1: route one Chat message with the real router (#169).
pub fn route_message(
    fleet: &FleetBridge,
    message: &str,
    complexity: Complexity,
) -> Result<ChatRoute, CoreError> {
    route_message_with(fleet, message, complexity, router::route)
}
