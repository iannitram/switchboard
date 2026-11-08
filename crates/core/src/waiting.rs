//! `waiting`: Waiting on you. One list of everything that needs me, each item with what, why,
//! where it came from, and one action (WAIT-1); ticking a CalDAV task marks it done (WAIT-2);
//! an item whose source clears drops at the next read (WAIT-3).

use crate::clock::UnixSeconds;
use crate::github::RepoSnapshot;
use crate::CoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    CalDavTask {
        uid: String,
    },
    /// An issue or PR carrying the label that marks it as needing me.
    NeedsMe {
        repo: String,
        number: u64,
    },
    /// A PR whose R25 review posted a FAIL without a fix.
    ReviewFail {
        repo: String,
        pr: u64,
    },
    /// A PR whose review ladder reached Fable.
    LadderAtFable {
        repo: String,
        pr: u64,
    },
    /// A Claude handoff not yet opened (FD-8).
    Handoff {
        handoff_id: String,
    },
    /// A guarded action asked for from the CLI or MCP (CORE-3).
    GuardedAction {
        text: String,
    },
    /// An IT ticket that needs a setting, a delete or a credential.
    ItTicket {
        number: u64,
    },
    /// A backup BAK-2 refused.
    RefusedBackup {
        repo: String,
    },
    /// The priority copy is stale (SKY-9).
    StalePriority,
    /// A pool edit to land through review (U5).
    PoolEdit {
        pool: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub what: String,
    pub why: String,
    pub source: Source,
    /// A link to the source (GitHub URL, `caldav:` uid, file path).
    pub link: String,
    /// The one action's label, e.g. "Mark done", "Open PR", "Open in Claude".
    pub action: String,
    pub since: UnixSeconds,
}

/// WAIT-1: the items GitHub's snapshot gives (needing-me labels, review FAILs without a fix,
/// ladders at Fable), for every repo.
pub fn from_snapshots(snaps: &[RepoSnapshot]) -> Vec<Item> {
    todo!("WAIT-1")
}

/// WAIT-1: merge every source's items into one list, newest first; ties by `what`.
pub fn merge(lists: Vec<Vec<Item>>) -> Vec<Item> {
    todo!("WAIT-1")
}

/// WAIT-3: the items of `next` only: an item in `previous` whose source is not in `next` is
/// dropped; one still there keeps its `since` from `previous`.
pub fn carry(previous: &[Item], next: Vec<Item>) -> Vec<Item> {
    todo!("WAIT-3")
}

/// WAIT-1: one item per open CalDAV task, `since` = `at`.
pub fn caldav_items(tasks: &[(String, String)], at: UnixSeconds) -> Vec<Item> {
    tasks
        .iter()
        .map(|(uid, summary)| Item {
            what: summary.clone(),
            why: "A task of mine in CalDAV (U7).".to_string(),
            source: Source::CalDavTask { uid: uid.clone() },
            link: format!("caldav:{uid}"),
            action: "Mark done".to_string(),
            since: at,
        })
        .collect()
}

/// WAIT-1: ask `caldav` for its open tasks; its error is returned as it is.
pub fn from_caldav(caldav: &dyn CalDav, at: UnixSeconds) -> Result<Vec<Item>, CoreError> {
    Ok(caldav_items(&caldav.open_tasks()?, at))
}

/// WAIT-1, WAIT-2: my CalDAV tasks (U7: waits for the address and my yes for a credential).
pub trait CalDav: Send + Sync {
    fn open_tasks(&self) -> Result<Vec<(String, String)>, CoreError>;
    fn mark_done(&self, uid: &str) -> Result<(), CoreError>;
}
