//! Tests for FD-5 (`signals`: the things in one turn that move a sticky chat off its model).
//! Matching is case-insensitive and on whole words; the order is AskClaude, NamedAgent (in
//! `agents` order), ThumbsDown, HandoffMarker. No model or clock is read.

use switchboard_core::frontdoor::sticky::{signals, Signal};

fn agents() -> Vec<String> {
    vec!["Harmon".to_string(), "Therapist".to_string()]
}

#[test]
fn ask_claude_in_a_sentence_gives_ask_claude() {
    assert_eq!(
        signals("Please ask Claude about this", &[], false, "Sure."),
        vec![Signal::AskClaude]
    );
}

#[test]
fn ask_claude_run_together_or_a_longer_word_gives_nothing() {
    assert_eq!(signals("askClaude", &[], false, ""), Vec::<Signal>::new());
    assert_eq!(
        signals("ask claudette", &[], false, ""),
        Vec::<Signal>::new()
    );
}

#[test]
fn ask_claude_any_case_gives_ask_claude() {
    assert_eq!(
        signals("ASK claude, then tell me.", &[], false, ""),
        vec![Signal::AskClaude]
    );
}

#[test]
fn agent_name_in_lower_case_gives_named_agent_spelled_as_in_agents() {
    assert_eq!(
        signals("harmon, what next?", &agents(), false, ""),
        vec![Signal::NamedAgent("Harmon".to_string())]
    );
}

#[test]
fn agent_name_inside_a_longer_word_gives_nothing() {
    assert_eq!(
        signals("harmonic", &agents(), false, ""),
        Vec::<Signal>::new()
    );
}

#[test]
fn both_agent_names_give_both_in_agents_order() {
    assert_eq!(
        signals("therapist and then harmon", &agents(), false, ""),
        vec![
            Signal::NamedAgent("Harmon".to_string()),
            Signal::NamedAgent("Therapist".to_string()),
        ]
    );
}

#[test]
fn thumbs_down_comes_after_the_message_signals() {
    assert_eq!(
        signals("ask claude or harmon", &agents(), true, "Done."),
        vec![
            Signal::AskClaude,
            Signal::NamedAgent("Harmon".to_string()),
            Signal::ThumbsDown,
        ]
    );
}

#[test]
fn handoff_marker_at_the_end_of_the_reply_comes_last() {
    assert_eq!(
        signals(
            "what now?",
            &agents(),
            true,
            "I can't see it. [[handoff: needs the repo]]"
        ),
        vec![
            Signal::ThumbsDown,
            Signal::HandoffMarker("needs the repo".to_string()),
        ]
    );
}

#[test]
fn all_four_signals_come_in_the_stated_order() {
    assert_eq!(
        signals(
            "Therapist: ask Claude, or Harmon?",
            &agents(),
            true,
            "Not mine. [[handoff: needs the repo]]"
        ),
        vec![
            Signal::AskClaude,
            Signal::NamedAgent("Harmon".to_string()),
            Signal::NamedAgent("Therapist".to_string()),
            Signal::ThumbsDown,
            Signal::HandoffMarker("needs the repo".to_string()),
        ]
    );
}

#[test]
fn a_plain_turn_gives_an_empty_vec() {
    assert_eq!(
        signals("What is the weather today?", &agents(), false, "Sunny."),
        Vec::<Signal>::new()
    );
}
