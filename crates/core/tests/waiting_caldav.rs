//! Locked tests for #59 (WAIT-1: CalDAV tasks as Waiting on you items). Written before the build
//! (R28); the builder never edits this file.

use switchboard_core::waiting::{caldav_items, from_caldav, CalDav, Item, Source};
use switchboard_core::CoreError;

/// A fake CalDAV: returns the tasks it was given, or the error it was given.
struct FakeCalDav {
    tasks: Result<Vec<(String, String)>, CoreError>,
}

impl FakeCalDav {
    fn with_tasks(tasks: &[(&str, &str)]) -> Self {
        FakeCalDav {
            tasks: Ok(tasks
                .iter()
                .map(|(uid, summary)| (uid.to_string(), summary.to_string()))
                .collect()),
        }
    }

    fn failing(err: CoreError) -> Self {
        FakeCalDav { tasks: Err(err) }
    }
}

impl CalDav for FakeCalDav {
    fn open_tasks(&self) -> Result<Vec<(String, String)>, CoreError> {
        self.tasks.clone()
    }

    fn mark_done(&self, _uid: &str) -> Result<(), CoreError> {
        Ok(())
    }
}

fn item(uid: &str, what: &str, at: i64) -> Item {
    Item {
        what: what.to_string(),
        why: "A task of mine in CalDAV (U7).".to_string(),
        source: Source::CalDavTask {
            uid: uid.to_string(),
        },
        link: format!("caldav:{uid}"),
        action: "Mark done".to_string(),
        since: at,
    }
}

#[test]
fn no_tasks_gives_no_items() {
    let caldav = FakeCalDav::with_tasks(&[]);
    assert_eq!(from_caldav(&caldav, 1_700_000_000), Ok(vec![]));
    assert_eq!(caldav_items(&[], 1_700_000_000), vec![]);
}

#[test]
fn two_tasks_give_two_items_in_order_with_their_fields() {
    let caldav =
        FakeCalDav::with_tasks(&[("uid-1", "Renew the passport"), ("uid-2", "Call the bank")]);
    let at = 1_700_000_500;
    let expected = vec![
        item("uid-1", "Renew the passport", at),
        item("uid-2", "Call the bank", at),
    ];
    assert_eq!(from_caldav(&caldav, at), Ok(expected.clone()));

    let pairs = vec![
        ("uid-1".to_string(), "Renew the passport".to_string()),
        ("uid-2".to_string(), "Call the bank".to_string()),
    ];
    assert_eq!(caldav_items(&pairs, at), expected);
}

#[test]
fn a_trait_error_comes_back_unchanged() {
    let err = CoreError::GitHub("CalDAV server refused the login".to_string());
    let caldav = FakeCalDav::failing(err.clone());
    assert_eq!(from_caldav(&caldav, 1_700_000_000), Err(err));
}

#[test]
fn an_empty_summary_is_kept_as_given() {
    let caldav = FakeCalDav::with_tasks(&[("uid-3", "")]);
    let at = 1_700_000_900;
    assert_eq!(from_caldav(&caldav, at), Ok(vec![item("uid-3", "", at)]));
}
