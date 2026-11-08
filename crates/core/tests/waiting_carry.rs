//! Tests for WAIT-3 (an item whose source clears drops at the next read; one still there keeps
//! its `since`). `carry` is pure, so the tests build plain items and need no fakes.

use switchboard_core::clock::UnixSeconds;
use switchboard_core::waiting::{carry, Item, Source};

fn item(source: Source, what: &str, since: UnixSeconds) -> Item {
    Item {
        what: what.to_string(),
        why: "A test item.".to_string(),
        source,
        link: "https://example.invalid/item".to_string(),
        action: "Open PR".to_string(),
        since,
    }
}

fn needs_me(number: u64) -> Source {
    Source::NeedsMe {
        repo: "owner/repo".to_string(),
        number,
    }
}

#[test]
fn an_item_whose_source_is_missing_from_next_is_dropped() {
    let previous = vec![
        item(needs_me(1), "Label one", 100),
        item(needs_me(2), "Label two", 200),
    ];
    let next = vec![item(needs_me(2), "Label two", 900)];
    let carried = carry(&previous, next);
    assert_eq!(carried.len(), 1);
    assert_eq!(carried[0].source, needs_me(2));
}

#[test]
fn an_item_in_both_keeps_the_older_since() {
    let previous = vec![item(needs_me(1), "Label one", 100)];
    let next = vec![item(needs_me(1), "Label one", 900)];
    let carried = carry(&previous, next);
    assert_eq!(carried[0].since, 100);
}

#[test]
fn a_new_item_keeps_its_own_since() {
    let previous = vec![item(needs_me(1), "Label one", 100)];
    let next = vec![item(needs_me(2), "Label two", 900)];
    let carried = carry(&previous, next);
    assert_eq!(carried.len(), 1);
    assert_eq!(carried[0].source, needs_me(2));
    assert_eq!(carried[0].since, 900);
}

#[test]
fn an_empty_next_gives_an_empty_list() {
    let previous = vec![item(needs_me(1), "Label one", 100)];
    assert!(carry(&previous, Vec::new()).is_empty());
}

#[test]
fn an_empty_previous_returns_next_unchanged() {
    let next = vec![
        item(needs_me(1), "Label one", 900),
        item(
            Source::ReviewFail {
                repo: "owner/repo".to_string(),
                pr: 7,
            },
            "Review FAIL",
            950,
        ),
    ];
    assert_eq!(carry(&[], next.clone()), next);
}

#[test]
fn a_changed_what_shows_the_new_text_with_the_old_since() {
    let previous = vec![item(needs_me(1), "Old title", 100)];
    let next = vec![item(needs_me(1), "New title", 900)];
    let carried = carry(&previous, next);
    assert_eq!(carried[0].what, "New title");
    assert_eq!(carried[0].since, 100);
}
