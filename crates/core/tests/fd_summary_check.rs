//! Tests for FD-7 (`missing_from_summary` lists the links and open questions the summary does not
//! name, links first, each in the order given; `checked_summary` asks once more with the missing
//! items when the first summary leaves any out, and uses the second answer as it comes). Summaries,
//! links and questions are built in code; no model is called.

use switchboard_core::frontdoor::packet::{checked_summary, missing_from_summary};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

const FULL: &str = "Fixed src/a.rs and src/b.rs. Open: Which port should the sidecar use?";

#[test]
fn a_summary_naming_every_link_and_question_gives_an_empty_list() {
    let links = strings(&["src/a.rs", "src/b.rs"]);
    let questions = strings(&["Which port should the sidecar use?"]);
    assert!(missing_from_summary(FULL, &links, &questions).is_empty());
}

#[test]
fn a_missing_file_link_is_listed() {
    let summary = "Fixed src/a.rs. Open: Which port should the sidecar use?";
    let links = strings(&["src/a.rs", "src/b.rs"]);
    let questions = strings(&["Which port should the sidecar use?"]);
    assert_eq!(
        missing_from_summary(summary, &links, &questions),
        strings(&["src/b.rs"])
    );
}

#[test]
fn a_missing_question_is_listed() {
    let summary = "Fixed src/a.rs and src/b.rs.";
    let links = strings(&["src/a.rs", "src/b.rs"]);
    let questions = strings(&["Which port should the sidecar use?"]);
    assert_eq!(
        missing_from_summary(summary, &links, &questions),
        strings(&["Which port should the sidecar use?"])
    );
}

#[test]
fn links_come_before_questions_each_in_the_order_given() {
    let summary = "Nothing named here.";
    let links = strings(&["b.md", "a.md"]);
    let questions = strings(&["Q2?", "Q1?"]);
    assert_eq!(
        missing_from_summary(summary, &links, &questions),
        strings(&["b.md", "a.md", "Q2?", "Q1?"])
    );
}

#[test]
fn a_name_must_appear_exactly() {
    let summary = "Fixed SRC/A.RS.";
    let links = strings(&["src/a.rs"]);
    assert_eq!(
        missing_from_summary(summary, &links, &[]),
        strings(&["src/a.rs"])
    );
}

#[test]
fn checked_summary_with_every_item_named_never_asks_again() {
    let links = strings(&["src/a.rs", "src/b.rs"]);
    let questions = strings(&["Which port should the sidecar use?"]);
    let mut calls = 0;
    let mut ask = |_: &[String]| {
        calls += 1;
        String::new()
    };
    let (summary, still) = checked_summary(FULL.to_string(), &links, &questions, &mut ask);
    assert_eq!(calls, 0);
    assert_eq!(summary, FULL);
    assert!(still.is_empty());
}

#[test]
fn checked_summary_asks_once_with_the_missing_file_and_returns_the_second_answer() {
    let first = "Fixed src/a.rs. Open: Which port should the sidecar use?".to_string();
    let links = strings(&["src/a.rs", "src/b.rs"]);
    let questions = strings(&["Which port should the sidecar use?"]);
    let mut asked: Vec<Vec<String>> = Vec::new();
    let mut ask = |missing: &[String]| {
        asked.push(missing.to_vec());
        FULL.to_string()
    };
    let (summary, still) = checked_summary(first, &links, &questions, &mut ask);
    assert_eq!(asked, vec![strings(&["src/b.rs"])]);
    assert_eq!(summary, FULL);
    assert!(still.is_empty());
}

#[test]
fn a_second_answer_still_short_is_returned_as_it_is_and_not_asked_again() {
    let first = "Nothing named.".to_string();
    let links = strings(&["src/a.rs", "src/b.rs"]);
    let questions = strings(&["Which port should the sidecar use?"]);
    let second = "Fixed src/a.rs.".to_string();
    let mut calls = 0;
    let mut ask = |_: &[String]| {
        calls += 1;
        second.clone()
    };
    let (summary, still) = checked_summary(first, &links, &questions, &mut ask);
    assert_eq!(calls, 1);
    assert_eq!(summary, second);
    assert_eq!(
        still,
        strings(&["src/b.rs", "Which port should the sidecar use?"])
    );
}
