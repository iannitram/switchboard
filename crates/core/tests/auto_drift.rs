//! AUTO-8: `drifted` lists the hand-kept copies whose text differs from the source. Texts are
//! compared with `\r\n` read as `\n` and trailing whitespace at the end trimmed. Pure: no file is
//! read.

use switchboard_core::autopilot::instructions::drifted;

const SOURCE: &str = "Read AGENTS.md first.\nThen work.\n";

fn copy(path: &str, text: &str) -> (String, String) {
    (path.to_string(), text.to_string())
}

#[test]
fn copies_equal_to_source_give_no_drift() {
    let copies = vec![copy("CLAUDE.md", SOURCE), copy("GEMINI.md", SOURCE)];
    assert!(drifted(SOURCE, &copies).is_empty());
}

#[test]
fn two_of_three_differing_copies_are_listed_in_input_order() {
    let copies = vec![
        copy("A.md", "Read AGENTS.md first.\nThen work.\nExtra.\n"),
        copy("B.md", SOURCE),
        copy("C.md", "Read AGENTS.md first.\nThen rest.\n"),
    ];
    assert_eq!(drifted(SOURCE, &copies), vec!["A.md", "C.md"]);
}

#[test]
fn crlf_line_endings_are_not_drift() {
    let copies = vec![copy("CLAUDE.md", "Read AGENTS.md first.\r\nThen work.\r\n")];
    assert!(drifted(SOURCE, &copies).is_empty());
}

#[test]
fn trailing_newline_difference_is_not_drift() {
    let copies = vec![
        copy("A.md", "Read AGENTS.md first.\nThen work."),
        copy("B.md", "Read AGENTS.md first.\nThen work.\n\n  \n"),
    ];
    assert!(drifted(SOURCE, &copies).is_empty());
}

#[test]
fn one_word_difference_is_drift() {
    let copies = vec![copy("CLAUDE.md", "Read AGENTS.md first.\nThen play.\n")];
    assert_eq!(drifted(SOURCE, &copies), vec!["CLAUDE.md"]);
}

#[test]
fn leading_whitespace_difference_is_drift() {
    let copies = vec![copy("CLAUDE.md", " Read AGENTS.md first.\nThen work.\n")];
    assert_eq!(drifted(SOURCE, &copies), vec!["CLAUDE.md"]);
}

#[test]
fn empty_copies_give_empty_list() {
    let copies: Vec<(String, String)> = Vec::new();
    assert!(drifted(SOURCE, &copies).is_empty());
}
