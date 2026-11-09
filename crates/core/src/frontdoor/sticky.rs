//! FD-5, FD-9, FD-10: a chat stays on its model until a signal moves it; ratings and implicit
//! signals are logged as their own rows.

use super::adapters::split_handoff;

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
    let words = words_of(my_message);
    let mut found = Vec::new();
    if has_words(&words, "ask claude") {
        found.push(Signal::AskClaude);
    }
    for agent in agents {
        if has_words(&words, agent) {
            found.push(Signal::NamedAgent(agent.clone()));
        }
    }
    if thumbs_down {
        found.push(Signal::ThumbsDown);
    }
    if let Some(reason) = split_handoff(reply).1 {
        found.push(Signal::HandoffMarker(reason));
    }
    found
}

/// The words of a text, lowercased: each run of letters and digits.
fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// True when the words of `phrase` appear in `words` one after another. An empty phrase
/// matches nothing.
fn has_words(words: &[String], phrase: &str) -> bool {
    let wanted = words_of(phrase);
    !wanted.is_empty() && words.windows(wanted.len()).any(|run| run == wanted.as_slice())
}

/// FD-9: implicit signals, logged apart and not counted by FD-4 in 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Implicit {
    OpenedInClaude,
    ReAsked,
    Abandoned,
    CopiedAnswer,
}
