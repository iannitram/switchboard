//! FD-5, FD-9, FD-10: a chat stays on its model until a signal moves it; ratings and implicit
//! signals are logged as their own rows.

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
