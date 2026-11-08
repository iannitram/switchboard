//! Tests for #61 (WAIT-2: ticking a CalDAV task in Waiting on you marks it done). A fake
//! `CalDav` records the `mark_done` calls; the real client waits on U7.

use std::sync::Mutex;

use switchboard_core::waiting::{tick_caldav, CalDav, Item, Source};
use switchboard_core::CoreError;

/// A fake `CalDav`: records every `mark_done` uid and returns `fail` when it is set.
struct FakeCalDav {
    done: Mutex<Vec<String>>,
    fail: Option<CoreError>,
}

impl FakeCalDav {
    fn new(fail: Option<CoreError>) -> Self {
        Self {
            done: Mutex::new(Vec::new()),
            fail,
        }
    }

    fn calls(&self) -> Vec<String> {
        self.done.lock().expect("fake lock").clone()
    }
}

impl CalDav for FakeCalDav {
    fn open_tasks(&self) -> Result<Vec<(String, String)>, CoreError> {
        Ok(Vec::new())
    }

    fn mark_done(&self, uid: &str) -> Result<(), CoreError> {
        self.done.lock().expect("fake lock").push(uid.to_string());
        match &self.fail {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

fn item(source: Source) -> Item {
    Item {
        what: "Renew the domain".to_string(),
        why: "A task of mine in CalDAV (U7).".to_string(),
        source,
        link: "caldav:uid-a".to_string(),
        action: "Mark done".to_string(),
        since: 1_000,
    }
}

#[test]
fn a_caldav_item_marks_its_uid_done_once() {
    let fake = FakeCalDav::new(None);
    let source = Source::CalDavTask {
        uid: "uid-a".to_string(),
    };
    assert_eq!(tick_caldav(&fake, &item(source.clone())), Ok(source));
    assert_eq!(fake.calls(), vec!["uid-a".to_string()]);
}

#[test]
fn a_handoff_item_calls_nothing_and_errors() {
    let fake = FakeCalDav::new(None);
    let handoff = item(Source::Handoff {
        handoff_id: "h-1".to_string(),
    });
    assert!(matches!(
        tick_caldav(&fake, &handoff),
        Err(CoreError::Parse(_))
    ));
    assert!(fake.calls().is_empty());
}

#[test]
fn a_mark_done_error_comes_back_unchanged() {
    let error = CoreError::GitHub("CalDAV server refused the login".to_string());
    let fake = FakeCalDav::new(Some(error.clone()));
    let task = item(Source::CalDavTask {
        uid: "uid-b".to_string(),
    });
    assert_eq!(tick_caldav(&fake, &task), Err(error));
    assert_eq!(fake.calls(), vec!["uid-b".to_string()]);
}
