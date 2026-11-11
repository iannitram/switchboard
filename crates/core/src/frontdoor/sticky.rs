//! FD-5, FD-9, FD-10: a chat stays on its model until a signal moves it; ratings and implicit
//! signals are logged as their own rows.

use super::adapters::{split_handoff, Answer};
use super::rating_log::rating_id;
use super::thread::{is_turn, ThreadStore};
use crate::joblog::{JobRow, RowSource};
use crate::CoreError;

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
///
/// Matching is case-insensitive and on whole words. The order is: `AskClaude`, one `NamedAgent`
/// per entry of `agents` found (in `agents` order, spelled as in `agents`), `ThumbsDown`, then
/// `HandoffMarker` with the reason `split_handoff` finds. No signal gives an empty vec.
pub fn signals(my_message: &str, agents: &[String], thumbs_down: bool, reply: &str) -> Vec<Signal> {
    let message_words = words(my_message);
    let mut found = Vec::new();
    if has_phrase(&message_words, &["ask".to_string(), "claude".to_string()]) {
        found.push(Signal::AskClaude);
    }
    for agent in agents {
        if has_phrase(&message_words, &words(agent)) {
            found.push(Signal::NamedAgent(agent.clone()));
        }
    }
    if thumbs_down {
        found.push(Signal::ThumbsDown);
    }
    if let (_, Some(reason)) = split_handoff(reply) {
        found.push(Signal::HandoffMarker(reason));
    }
    found
}

/// The lowercased words of `text`: runs of letters and digits; everything else separates them.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Whether `phrase` (one or more words) appears as consecutive whole words in `words`. An empty
/// phrase never matches.
fn has_phrase(words: &[String], phrase: &[String]) -> bool {
    !phrase.is_empty() && words.windows(phrase.len()).any(|window| window == phrase)
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

/// FD-10: set `moved` on a rating row whose rating is "down"; any other row is returned unchanged. `rating` and `outcome` are never touched.
pub fn with_move(row: JobRow, moved: bool) -> JobRow {
    if row.rating.as_deref() != Some("down") {
        return row;
    }
    JobRow {
        moved: Some(moved),
        ..row
    }
}

/// FD-10: over the rows, (thumbs-downs that did not move the chat, thumbs-downs that did). Rows with `implicit` true, thumbs-ups and rows with `moved` None are not counted.
pub fn thumbs_down_counts(rows: &[JobRow]) -> (usize, usize) {
    rows.iter()
        .filter(|row| !row.implicit && row.rating.as_deref() == Some("down"))
        .fold((0, 0), |(not_moved, moved), row| match row.moved {
            Some(false) => (not_moved + 1, moved),
            Some(true) => (not_moved, moved + 1),
            None => (not_moved, moved),
        })
}

/// FD-5: the step a sticky chat's route shows: the chat stayed on its model.
pub const STICKY_STEP: &str = "sticky chat";

/// FD-5: the model a thread is open on: its last answer's service, model and kind, that
/// answer's text with its `[[handoff: <reason>]]` marker put back when it had one, its turn
/// index, and whether that answer's rating is a thumbs down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenChat {
    pub service: String,
    pub model: String,
    pub kind: Option<String>,
    pub reply: String,
    pub turn: u32,
    pub thumbs_down: bool,
}

/// The `escalation_step` an answer row carries for a reply that had a handoff marker, so the
/// next turn still sees the marker (FD-5): `handoff: <reason>`.
pub fn handoff_step(reason: &str) -> String {
    format!("handoff: {reason}")
}

/// FD-5: the chat `handoff_id` is open on, or None when the thread has no answer yet.
pub fn open_chat(store: &ThreadStore, handoff_id: &str) -> Result<Option<OpenChat>, CoreError> {
    let rows = store.read(handoff_id)?;
    let turns: Vec<&JobRow> = rows.iter().filter(|row| is_turn(row)).collect();
    let Some((turn, last)) = turns
        .iter()
        .enumerate()
        .rev()
        .find(|(_, row)| row.role.as_deref() == Some("assistant"))
    else {
        return Ok(None);
    };
    let (Some(service), Some(model)) = (last.lane.clone(), last.model.clone()) else {
        return Ok(None);
    };
    let turn = turn as u32;
    let rating = rating_id(handoff_id, turn);
    let thumbs_down = rows.iter().any(|row| {
        row.id.as_deref() == Some(rating.as_str()) && row.rating.as_deref() == Some("down")
    });
    let mut reply = last.text.clone().unwrap_or_default();
    if let Some(reason) = last
        .escalation_step
        .as_deref()
        .and_then(|step| step.strip_prefix("handoff: "))
    {
        reply.push_str(&format!(" [[handoff: {reason}]]"));
    }
    Ok(Some(OpenChat {
        service,
        model,
        kind: last.kind.clone(),
        reply,
        turn,
        thumbs_down,
    }))
}

/// FD-5: the signals that move `open` for my next message, with `agents` the names that count.
pub fn move_signals(open: &OpenChat, message: &str, agents: &[String]) -> Vec<Signal> {
    signals(message, agents, open.thumbs_down, &open.reply)
}

/// FD-5: whether a signal means the open model must not take the next turn: a thumbs down or
/// its own handoff marker.
pub fn leaves_the_model(found: &[Signal]) -> bool {
    found
        .iter()
        .any(|s| matches!(s, Signal::ThumbsDown | Signal::HandoffMarker(_)))
}
