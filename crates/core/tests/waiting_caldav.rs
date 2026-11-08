//! Locked tests for #59 (WAIT-1: CalDAV tasks as Waiting items). Written before the build
//! (R28); the builder never edits this file.

use switchboard_core::waiting::{caldav_items, from_caldav, CalDav, Item, Source};
use switchboard_core::CoreError;

/// A fake `CalDav`: `open_tasks` returns what it was given, error included.
struct FakeCalDav {
    tasks: Result<Vec<(String, String)>, CoreError>,
}

impl CalDav for FakeCalDav {
    fn open_tasks(&self) -> Result<Vec<(String, String)>, CoreError> {
        self.tasks.clone()
    }

    fn mark_done(&self, _uid: &str) -> Result<(), CoreError> {
        Ok(())
    }
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(uid, summary)| (uid.to_string(), summary.to_string()))
        .collect()
}

#[test]
fn no_tasks_gives_an_empty_list() {
    let fake = FakeCalDav {
        tasks: Ok(Vec::new()),
    };
    assert_eq!(from_caldav(&fake, 1_000), Ok(Vec::new()));
    assert!(caldav_items(&[], 1_000).is_empty());
}

#[test]
fn two_tasks_give_two_items_in_order_with_the_fields_above() {
    let fake = FakeCalDav {
        tasks: Ok(pairs(&[
            ("uid-a", "Renew the domain"),
            ("uid-b", "Sign the form"),
        ])),
    };
    let expected = vec![
        Item {
            what: "Renew the domain".to_string(),
            why: "A task of mine in CalDAV (U7).".to_string(),
            source: Source::CalDavTask {
                uid: "uid-a".to_string(),
            },
            link: "caldav:uid-a".to_string(),
            action: "Mark done".to_string(),
            since: 1_000,
        },
        Item {
            what: "Sign the form".to_string(),
            why: "A task of mine in CalDAV (U7).".to_string(),
            source: Source::CalDavTask {
                uid: "uid-b".to_string(),
            },
            link: "caldav:uid-b".to_string(),
            action: "Mark done".to_string(),
            since: 1_000,
        },
    ];
    assert_eq!(from_caldav(&fake, 1_000), Ok(expected.clone()));
    let input = pairs(&[("uid-a", "Renew the domain"), ("uid-b", "Sign the form")]);
    assert_eq!(caldav_items(&input, 1_000), expected);
}

#[test]
fn a_trait_error_comes_back_unchanged() {
    let error = CoreError::GitHub("CalDAV server refused the login".to_string());
    let fake = FakeCalDav {
        tasks: Err(error.clone()),
    };
    assert_eq!(from_caldav(&fake, 1_000), Err(error));
}

#[test]
fn an_empty_summary_is_kept_as_given() {
    let fake = FakeCalDav {
        tasks: Ok(pairs(&[("uid-empty", "")])),
    };
    let items = from_caldav(&fake, 1_000).expect("no error from the fake");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].what, "");
    assert_eq!(items[0].link, "caldav:uid-empty");
}
