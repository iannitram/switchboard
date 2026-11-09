//! AUTO-2: `roadmap_changed` names the repos whose ROADMAP.md text changed between two snapshots.
//! A repo is changed when it has text in `next` that differs from `previous` (missing or `None`
//! counts as no text). Output follows `next`'s order, each repo once. Pure: no file is read.

use switchboard_core::autopilot::weekly::roadmap_changed;

fn entry(repo: &str, text: Option<&str>) -> (String, Option<String>) {
    (repo.to_string(), text.map(str::to_string))
}

#[test]
fn equal_text_in_both_snapshots_gives_no_change() {
    let previous = vec![entry("alpha", Some("# Plan\n"))];
    let next = vec![entry("alpha", Some("# Plan\n"))];
    assert!(roadmap_changed(&previous, &next).is_empty());
}

#[test]
fn only_the_repo_with_different_text_is_listed_out_of_three() {
    let previous = vec![
        entry("alpha", Some("# Plan\n")),
        entry("beta", Some("# Plan\n")),
        entry("gamma", Some("# Plan\n")),
    ];
    let next = vec![
        entry("alpha", Some("# Plan\n")),
        entry("beta", Some("# Plan, edited\n")),
        entry("gamma", Some("# Plan\n")),
    ];
    assert_eq!(roadmap_changed(&previous, &next), vec!["beta"]);
}

#[test]
fn comparison_is_exact_byte_for_byte() {
    let previous = vec![entry("alpha", Some("Plan\r\n"))];
    let next = vec![entry("alpha", Some("Plan\n"))];
    assert_eq!(roadmap_changed(&previous, &next), vec!["alpha"]);
}

#[test]
fn new_repo_with_text_is_listed_and_new_repo_without_text_is_not() {
    let previous = vec![entry("alpha", Some("# Plan\n"))];
    let next = vec![
        entry("alpha", Some("# Plan\n")),
        entry("beta", Some("# New\n")),
        entry("gamma", None),
    ];
    assert_eq!(roadmap_changed(&previous, &next), vec!["beta"]);
}

#[test]
fn previous_entry_of_none_counts_as_changed_when_next_has_text() {
    let previous = vec![entry("alpha", None)];
    let next = vec![entry("alpha", Some("# Plan\n"))];
    assert_eq!(roadmap_changed(&previous, &next), vec!["alpha"]);
}

#[test]
fn next_text_of_none_is_never_a_change() {
    let previous = vec![entry("alpha", Some("# Plan\n"))];
    let next = vec![entry("alpha", None)];
    assert!(roadmap_changed(&previous, &next).is_empty());
}

#[test]
fn result_follows_next_order_and_repeats_count_once() {
    let previous = vec![entry("alpha", Some("old a\n")), entry("beta", Some("old b\n"))];
    let next = vec![
        entry("beta", Some("new b\n")),
        entry("alpha", Some("new a\n")),
        entry("beta", Some("new b again\n")),
    ];
    assert_eq!(roadmap_changed(&previous, &next), vec!["beta", "alpha"]);
}

#[test]
fn repeated_repo_uses_its_first_entry_even_when_that_entry_is_none() {
    let previous: Vec<(String, Option<String>)> = Vec::new();
    let next = vec![entry("alpha", None), entry("alpha", Some("# Plan\n"))];
    assert!(roadmap_changed(&previous, &next).is_empty());
}
