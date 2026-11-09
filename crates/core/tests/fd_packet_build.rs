//! Tests for FD-6 (`build` carries the summary as given, the last `LAST_TURNS` turns word for
//! word, oldest first, and every file link in first-seen order with repeats dropped; never the
//! full transcript). Turns and links are built in code.

use switchboard_core::frontdoor::packet::{build, Turn, LAST_TURNS};

fn turn(role: &str, text: &str) -> Turn {
    Turn {
        role: role.to_string(),
        text: text.to_string(),
    }
}

/// `n` turns, alternating user and assistant, each with text that keeps its edge whitespace.
fn turns(n: usize) -> Vec<Turn> {
    (0..n)
        .map(|i| {
            let role = if i % 2 == 0 { "user" } else { "assistant" };
            turn(role, &format!("  turn {i}\n\twith  spacing \n"))
        })
        .collect()
}

fn links(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn last_turns_is_six() {
    assert_eq!(LAST_TURNS, 6);
}

#[test]
fn ten_turns_give_the_last_six_in_order_byte_for_byte() {
    let all = turns(10);
    let packet = build("sum", &all, &[]);
    assert_eq!(packet.last_turns.len(), 6);
    assert_eq!(packet.last_turns, all[4..].to_vec());
    for (got, want) in packet.last_turns.iter().zip(&all[4..]) {
        assert_eq!(got.text.as_bytes(), want.text.as_bytes());
        assert_eq!(got.role, want.role);
    }
}

#[test]
fn three_turns_give_all_three() {
    let all = turns(3);
    let packet = build("sum", &all, &[]);
    assert_eq!(packet.last_turns, all);
}

#[test]
fn exactly_six_turns_give_all_six() {
    let all = turns(6);
    let packet = build("sum", &all, &[]);
    assert_eq!(packet.last_turns, all);
}

#[test]
fn seven_turns_drop_the_first() {
    let all = turns(7);
    let packet = build("sum", &all, &[]);
    assert_eq!(packet.last_turns, all[1..].to_vec());
}

#[test]
fn no_turns_give_empty_last_turns() {
    let packet = build("sum", &[], &[]);
    assert!(packet.last_turns.is_empty());
}

#[test]
fn never_holds_a_turn_older_than_the_last_six() {
    let all: Vec<Turn> = (0..25)
        .map(|i| turn("user", &format!("unique-{i:02}")))
        .collect();
    let packet = build("sum", &all, &[]);
    assert!(packet.last_turns.len() <= LAST_TURNS);
    for old in &all[..all.len() - LAST_TURNS] {
        assert!(
            !packet.last_turns.contains(old),
            "old turn {:?} kept",
            old.text
        );
    }
}

#[test]
fn repeated_links_keep_first_seen_order() {
    let packet = build("sum", &[], &links(&["a", "b", "a", "c", "b"]));
    assert_eq!(packet.links, links(&["a", "b", "c"]));
}

#[test]
fn links_match_by_exact_string() {
    let packet = build("sum", &[], &links(&["a", "A", "a ", "a"]));
    assert_eq!(packet.links, links(&["a", "A", "a "]));
}

#[test]
fn no_links_give_empty_links() {
    let packet = build("sum", &turns(2), &[]);
    assert!(packet.links.is_empty());
}

#[test]
fn summary_comes_back_unchanged() {
    let summary = "  The plan so far:\n- step one\n\n";
    let packet = build(summary, &turns(4), &links(&["x"]));
    assert_eq!(packet.summary, summary);
}

#[test]
fn empty_summary_comes_back_empty() {
    let packet = build("", &turns(4), &[]);
    assert_eq!(packet.summary, "");
}
