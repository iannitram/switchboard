//! FD-6 (#171): the packet a chat carries when it moves to another model. The summary, the last
//! 6 turns word for word, and every file link gathered so far; never the full transcript.

use switchboard_core::frontdoor::packet::{build, Turn, LAST_TURNS};

fn turn(i: usize) -> Turn {
    Turn {
        role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
        text: format!("turn {i}"),
    }
}

fn turns(n: usize) -> Vec<Turn> {
    (0..n).map(turn).collect()
}

fn texts(packet_turns: &[Turn]) -> Vec<String> {
    packet_turns.iter().map(|t| t.text.clone()).collect()
}

#[test]
fn last_turns_is_six() {
    assert_eq!(LAST_TURNS, 6);
}

#[test]
fn ten_turns_give_the_last_six_in_order() {
    let all = turns(10);
    let packet = build("summary", &all, &[]);
    assert_eq!(
        texts(&packet.last_turns),
        vec!["turn 4", "turn 5", "turn 6", "turn 7", "turn 8", "turn 9"]
    );
    assert_eq!(packet.last_turns, all[4..].to_vec());
}

#[test]
fn three_turns_give_all_three() {
    let all = turns(3);
    let packet = build("", &all, &[]);
    assert_eq!(packet.last_turns, all);
}

#[test]
fn no_turns_give_an_empty_last_turns() {
    let packet = build("summary", &[], &[]);
    assert!(packet.last_turns.is_empty());
}

#[test]
fn exactly_six_turns_are_all_kept() {
    let all = turns(6);
    let packet = build("", &all, &[]);
    assert_eq!(packet.last_turns, all);
}

#[test]
fn turn_text_is_kept_byte_for_byte() {
    let all = vec![
        Turn {
            role: "user".to_string(),
            text: "  leading and trailing spaces  \n\n".to_string(),
        },
        Turn {
            role: "assistant".to_string(),
            text: "tabs\tand\r\nCRLF, unicode: café — 日本語 ✓".to_string(),
        },
        Turn {
            role: "user".to_string(),
            text: String::new(),
        },
    ];
    let packet = build("", &all, &[]);
    assert_eq!(packet.last_turns.len(), 3);
    for (got, want) in packet.last_turns.iter().zip(all.iter()) {
        assert_eq!(got.role, want.role);
        assert_eq!(got.text.as_bytes(), want.text.as_bytes());
    }
}

#[test]
fn packet_never_holds_a_turn_older_than_the_last_six() {
    for n in 0..=20 {
        let all = turns(n);
        let packet = build("", &all, &[]);
        assert!(packet.last_turns.len() <= LAST_TURNS);
        let oldest_allowed = n.saturating_sub(LAST_TURNS);
        for (offset, got) in packet.last_turns.iter().enumerate() {
            assert_eq!(got.text, format!("turn {}", oldest_allowed + offset));
        }
    }
}

#[test]
fn links_keep_first_seen_order_without_repeats() {
    let links: Vec<String> = ["a", "b", "a", "c", "b"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let packet = build("", &[], &links);
    assert_eq!(packet.links, vec!["a", "b", "c"]);
}

#[test]
fn links_match_on_exact_text() {
    let links: Vec<String> = ["notes/a.md", "notes/a.md ", "notes/A.md", "notes/a.md"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let packet = build("", &[], &links);
    assert_eq!(packet.links, vec!["notes/a.md", "notes/a.md ", "notes/A.md"]);
}

#[test]
fn no_links_give_no_links() {
    let packet = build("summary", &turns(2), &[]);
    assert!(packet.links.is_empty());
}

#[test]
fn summary_comes_back_unchanged() {
    let summary = "  Rolling summary:\n- point one\n- point two  ";
    assert_eq!(build(summary, &[], &[]).summary, summary);
}

#[test]
fn empty_summary_stays_empty() {
    assert_eq!(build("", &turns(4), &[]).summary, "");
}
