//! AUTO-2: `roadmap_changed` names the repos whose ROADMAP.md text changed between two
//! snapshots, in `next`'s order with no repeats.

use switchboard_core::autopilot::weekly::roadmap_changed;

fn snap(pairs: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
    pairs
        .iter()
        .map(|(repo, text)| (repo.to_string(), text.map(str::to_string)))
        .collect()
}

#[test]
fn equal_text_gives_an_empty_list() {
    let previous = snap(&[("alpha", Some("# Roadmap\n")), ("beta", Some("v1"))]);
    let next = snap(&[("alpha", Some("# Roadmap\n")), ("beta", Some("v1"))]);
    assert!(roadmap_changed(&previous, &next).is_empty());
}

#[test]
fn different_text_gives_only_that_repo() {
    let previous = snap(&[
        ("alpha", Some("a")),
        ("beta", Some("b")),
        ("gamma", Some("c")),
    ]);
    let next = snap(&[
        ("alpha", Some("a")),
        ("beta", Some("b2")),
        ("gamma", Some("c")),
    ]);
    assert_eq!(roadmap_changed(&previous, &next), vec!["beta".to_string()]);
}

#[test]
fn text_is_compared_byte_for_byte() {
    let previous = snap(&[("alpha", Some("Roadmap"))]);
    let next = snap(&[("alpha", Some("Roadmap "))]);
    assert_eq!(roadmap_changed(&previous, &next), vec!["alpha".to_string()]);
}

#[test]
fn new_repo_with_text_is_listed_and_none_in_next_is_not() {
    let previous = snap(&[("alpha", Some("a")), ("beta", None), ("gamma", Some("c"))]);
    let next = snap(&[
        ("alpha", Some("a")),
        ("beta", Some("b")),
        ("gamma", None),
        ("delta", Some("d")),
        ("epsilon", None),
    ]);
    assert_eq!(
        roadmap_changed(&previous, &next),
        vec!["beta".to_string(), "delta".to_string()]
    );
}

#[test]
fn result_follows_next_order_and_repeats_count_once() {
    let previous = snap(&[("alpha", Some("a")), ("beta", Some("b"))]);
    let next = snap(&[
        ("gamma", Some("c")),
        ("beta", Some("b2")),
        ("alpha", Some("a2")),
        ("beta", Some("b3")),
        ("gamma", Some("c2")),
    ]);
    assert_eq!(
        roadmap_changed(&previous, &next),
        vec!["gamma".to_string(), "beta".to_string(), "alpha".to_string()]
    );
}

#[test]
fn a_repeat_uses_its_first_entry() {
    let previous = snap(&[("alpha", Some("a"))]);
    let next = snap(&[("alpha", Some("a")), ("alpha", Some("changed"))]);
    assert!(roadmap_changed(&previous, &next).is_empty());
}
