//! Tests for WAIT-2 (ticking a CalDAV task in Waiting on you). A fake `CalDav` records the
//! `mark_done` calls; the real client waits on U7.

use std::sync::Mutex;

use switchboard_core::waiting::{tick_caldav, CalDav, Item, Source};
use switchboard_core::CoreError;

#[derive(Default)]
struct FakeCalDav {
    done: Mutex<Vec<String>>,
    fail: Option<CoreError>,
}

impl FakeCalDav {
    fn failing(error: CoreError) -> Self {
        Self {
            done: Mutex::new(Vec::new()),
            fail: Some(error),
        }
    }

    fn done(&self) -> Vec<String> {
        self.done.lock().unwrap().clone()
    }
}

impl CalDav for FakeCalDav {
    fn open_tasks(&self) -> Result<Vec<(String, String)>, CoreError> {
        Ok(Vec::new())
    }

    fn mark_done(&self, uid: &str) -> Result<(), CoreError> {
        self.done.lock().unwrap().push(uid.to_string());
        match &self.fail {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

fn item(source: Source) -> Item {
    Item {
        what: "Reply to Sam".to_string(),
        why: "asked me for the figures".to_string(),
        source,
        link: "caldav:uid-1".to_string(),
        action: "Mark done".to_string(),
        since: 1_700_000_000,
    }
}

#[test]
fn a_caldav_item_marks_its_uid_done_once() {
    let fake = FakeCalDav::default();
    let source = item(Source::CalDavTask {
        uid: "uid-1".to_string(),
    });

    let result = tick_caldav(&fake, &source);

    assert_eq!(fake.done(), vec!["uid-1".to_string()]);
    assert_eq!(
        result,
        Ok(Source::CalDavTask {
            uid: "uid-1".to_string(),
        })
    );
}

#[test]
fn a_handoff_item_calls_nothing_and_errors() {
    let fake = FakeCalDav::default();
    let source = item(Source::Handoff {
        handoff_id: "h-1".to_string(),
    });

    let result = tick_caldav(&fake, &source);

    assert!(fake.done().is_empty());
    assert!(matches!(result, Err(CoreError::Parse(_))));
}

#[test]
fn a_mark_done_error_comes_back_unchanged() {
    let error = CoreError::Io("caldav: timed out".to_string());
    let fake = FakeCalDav::failing(error.clone());
    let source = item(Source::CalDavTask {
        uid: "uid-1".to_string(),
    });

    let result = tick_caldav(&fake, &source);

    assert_eq!(fake.done(), vec!["uid-1".to_string()]);
    assert_eq!(result, Err(error));
}
