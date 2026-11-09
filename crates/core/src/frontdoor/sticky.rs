//! FD-5, FD-9, FD-10: a chat stays on its model until a signal moves it; ratings and implicit
//! signals are logged as their own rows.

use super::adapters::Answer;
use crate::joblog::{JobRow, RowSource};

/// FD-5: what moves a sticky chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// I said "ask Claude".
    AskClaude,
    /// I named an agent.
    NamedAgent(String),
    ThumbsDown,
    /// The model wrote `[[handoff: <reason>]]`. Holds the reason.
    HandoffMarker(String),
}

/// FD-5: the signals in one turn: my message (`ask Claude`, an agent's name from `agents`),
/// my rating, and the model's reply (`[[handoff: <reason>]]`).
pub fn signals(my_message: &str, agents: &[String], thumbs_down: bool, reply: &str) -> Vec<Signal> {
    todo!("FD-5")
}

/// FD-9: implicit signals, logged apart and not counted by FD-4 in 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Implicit {
    OpenedInClaude,
    ReAsked,
    Abandoned,
    CopiedAnswer,
}

impl Implicit {
    /// The signal's name, as it appears after `#implicit-` in a row id.
    pub fn name(&self) -> &'static str {
        match self {
            Implicit::OpenedInClaude => "opened-in-claude",
            Implicit::ReAsked => "re-asked",
            Implicit::Abandoned => "abandoned",
            Implicit::CopiedAnswer => "copied-answer",
        }
    }
}

/// FD-9: a thumbs row for one answer. rating "up" or "down"; outcome "passed" or "failed"; source
/// Chat; implicit false; lane = the answer's service, model, kind and handoff_id from the arguments.
pub fn rating_row(
    answer: &Answer,
    kind: Option<&str>,
    handoff_id: &str,
    thumbs_up: bool,
) -> JobRow {
    let (rating, outcome) = if thumbs_up {
        ("up", "passed")
    } else {
        ("down", "failed")
    };
    JobRow {
        lane: Some(answer.service.clone()),
        model: Some(answer.model.clone()),
        kind: kind.map(str::to_string),
        handoff_id: Some(handoff_id.to_string()),
        rating: Some(rating.to_string()),
        outcome: Some(outcome.to_string()),
        source: RowSource::Chat,
        implicit: false,
        ..JobRow::default()
    }
}

/// FD-9: an implicit signal row. implicit true; rating None; outcome None; source Chat; id =
/// "<handoff_id>#implicit-<name>"; lane/model/kind from the arguments when known.
pub fn implicit_row(
    signal: Implicit,
    service: Option<&str>,
    model: Option<&str>,
    kind: Option<&str>,
    handoff_id: &str,
) -> JobRow {
    JobRow {
        id: Some(format!("{handoff_id}#implicit-{}", signal.name())),
        lane: service.map(str::to_string),
        model: model.map(str::to_string),
        kind: kind.map(str::to_string),
        handoff_id: Some(handoff_id.to_string()),
        source: RowSource::Chat,
        implicit: true,
        ..JobRow::default()
    }
}
