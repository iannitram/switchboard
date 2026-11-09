//! Tests for FD-8 (`as_prompt` gives the summary, then the last turns as `user:` / `assistant:`
//! lines oldest first, then the links one per line, under the headings `Summary`, `Last turns`,
//! `Links`). Packets are built in code.

use switchboard_core::frontdoor::packet::{as_prompt, build, Packet, Turn};

fn turn(role: &str, text: &str) -> Turn {
    Turn {
        role: role.to_string(),
        text: text.to_string(),
    }
}

fn packet(summary: &str, turns: Vec<Turn>, links: &[&str]) -> Packet {
    let links: Vec<String> = links.iter().map(|s| s.to_string()).collect();
    build(summary, &turns, &links)
}

#[test]
fn full_packet_is_exact() {
    let p = packet(
        "Fixing the labeller.",
        vec![
            turn("user", "why does it fail"),
            turn("assistant", "the rule is off"),
        ],
        &["docs/a.md", "src/b.rs"],
    );
    assert_eq!(
        as_prompt(&p),
        "Summary\nFixing the labeller.\n\nLast turns\nuser: why does it fail\nassistant: the rule is off\n\nLinks\ndocs/a.md\nsrc/b.rs"
    );
}

#[test]
fn headings_come_in_order() {
    let p = packet("s", vec![turn("user", "u")], &["x.md"]);
    let text = as_prompt(&p);
    let summary = text.find("Summary\n").unwrap();
    let turns = text.find("Last turns\n").unwrap();
    let links = text.find("Links\n").unwrap();
    assert!(summary < turns && turns < links);
}

#[test]
fn turns_are_oldest_first_with_their_role() {
    let p = packet(
        "s",
        vec![
            turn("user", "one"),
            turn("assistant", "two"),
            turn("user", "three"),
        ],
        &[],
    );
    let text = as_prompt(&p);
    let one = text.find("user: one").unwrap();
    let two = text.find("assistant: two").unwrap();
    let three = text.find("user: three").unwrap();
    assert!(one < two && two < three);
}

#[test]
fn only_the_last_six_turns_go_in() {
    let turns: Vec<Turn> = (0..8).map(|i| turn("user", &format!("t{i}"))).collect();
    let text = as_prompt(&packet("s", turns, &[]));
    assert!(!text.contains("user: t1\n"));
    assert!(text.contains("user: t2\n"));
    assert!(text.contains("user: t7"));
}

#[test]
fn each_link_is_on_its_own_line_after_the_links_heading() {
    let p = packet("s", vec![], &["a.md", "b.md", "c.md"]);
    let text = as_prompt(&p);
    let (_, links) = text.split_once("\nLinks\n").unwrap();
    assert_eq!(links, "a.md\nb.md\nc.md");
}

#[test]
fn no_links_keeps_the_heading_and_drops_only_the_link_lines() {
    let p = packet("s", vec![turn("user", "hi")], &[]);
    assert_eq!(
        as_prompt(&p),
        "Summary\ns\n\nLast turns\nuser: hi\n\nLinks\n"
    );
}
