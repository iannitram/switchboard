//! Tests for FD-5 (`signals` reads one turn: my message for "ask Claude" and named agents, my
//! rating, and the reply's handoff marker, in that order). No model and no clock are called.

use switchboard_core::frontdoor::sticky::{signals, Signal};

fn agents() -> Vec<String> {
    vec!["Harmon".to_string(), "Therapist".to_string()]
}

#[test]
fn ask_claude_is_found_inside_a_sentence() {
    assert_eq!(signals("Please ask Claude about this", &[], false, ""), vec![Signal::AskClaude]);
}

#[test]
fn ask_claude_needs_both_words_apart() {
    assert!(signals("askClaude", &[], false, "").is_empty());
    assert!(signals("ask claudette", &[], false, "").is_empty());
}

#[test]
fn named_agent_matches_a_whole_word_only() {
    assert_eq!(
        signals("harmon, what next?", &agents(), false, ""),
        vec![Signal::NamedAgent("Harmon".to_string())]
    );
    assert!(signals("harmonic", &agents(), false, "").is_empty());
}

#[test]
fn both_names_come_in_agents_order() {
    assert_eq!(
        signals("therapist and HARMON", &agents(), false, ""),
        vec![
            Signal::NamedAgent("Harmon".to_string()),
            Signal::NamedAgent("Therapist".to_string()),
        ]
    );
}

#[test]
fn thumbs_down_is_added_when_set() {
    assert_eq!(signals("ok", &[], true, ""), vec![Signal::ThumbsDown]);
}

#[test]
fn handoff_marker_gives_its_reason() {
    assert_eq!(
        signals("", &[], false, "Use the notes. [[handoff: needs the repo]]"),
        vec![Signal::HandoffMarker("needs the repo".to_string())]
    );
}

#[test]
fn all_four_signals_come_in_the_stated_order() {
    assert_eq!(
        signals(
            "ask claude, harmon",
            &agents(),
            true,
            "Take it. [[handoff: needs the repo]]",
        ),
        vec![
            Signal::AskClaude,
            Signal::NamedAgent("Harmon".to_string()),
            Signal::ThumbsDown,
            Signal::HandoffMarker("needs the repo".to_string()),
        ]
    );
}

#[test]
fn a_plain_turn_gives_nothing() {
    assert!(signals("what is the weather", &agents(), false, "Sunny.").is_empty());
}
