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

/// The label that marks an issue or PR as needing me.
pub const NEEDS_ME_LABEL: &str = "needs-ian";
/// The label AUTO-3 puts on a PR when the review ladder reaches Fable.
pub const AT_FABLE_LABEL: &str = "review:fable";

fn has_label(labels: &[String], label: &str) -> bool {
    labels.iter().any(|l| l == label)
}

/// WAIT-1: the items GitHub's snapshot gives (needing-me labels, review FAILs without a fix,
/// ladders at Fable), for every repo.
pub fn from_snapshots(snaps: &[RepoSnapshot]) -> Vec<Item> {
    let mut items = Vec::new();
    for snap in snaps.iter().filter(|s| !s.archived) {
        let repo = &snap.repo;
        for issue in snap
            .issues
            .iter()
            .filter(|i| i.state == "open" && has_label(&i.labels, NEEDS_ME_LABEL))
        {
            items.push(Item {
                what: format!("{repo}#{}: {}", issue.number, issue.title),
                why: format!("Labelled {NEEDS_ME_LABEL}."),
                source: Source::NeedsMe {
                    repo: repo.clone(),
                    number: issue.number,
                },
                link: format!("https://github.com/{repo}/issues/{}", issue.number),
                action: "Open issue".to_string(),
                since: snap.read_at,
            });
        }
        for pr in snap.pulls.iter().filter(|p| p.state == "open") {
            let what = format!("{repo}#{}: {}", pr.number, pr.title);
            let link = format!("https://github.com/{repo}/pull/{}", pr.number);
            if has_label(&pr.labels, NEEDS_ME_LABEL) {
                items.push(Item {
                    what: what.clone(),
                    why: format!("Labelled {NEEDS_ME_LABEL}."),
                    source: Source::NeedsMe {
                        repo: repo.clone(),
                        number: pr.number,
                    },
                    link: link.clone(),
                    action: "Open PR".to_string(),
                    since: snap.read_at,
                });
            }
            if pr
                .verdicts
                .last()
                .is_some_and(|v| v.verdict == "FAIL" && v.needs_me)
            {
                items.push(Item {
                    what: what.clone(),
                    why: "The review posted a FAIL without a fix: a design question or a decision of mine."
                        .to_string(),
                    source: Source::ReviewFail {
                        repo: repo.clone(),
                        pr: pr.number,
                    },
                    link: link.clone(),
                    action: "Open PR".to_string(),
                    since: snap.read_at,
                });
            }
            if has_label(&pr.labels, AT_FABLE_LABEL) {
                items.push(Item {
                    what,
                    why: "The review ladder reached Fable.".to_string(),
                    source: Source::LadderAtFable {
                        repo: repo.clone(),
                        pr: pr.number,
                    },
                    link,
                    action: "Open PR".to_string(),
                    since: snap.read_at,
                });
            }
        }
    }
    items
}

/// WAIT-1: merge every source's items into one list, newest first; ties by `what`.
pub fn merge(lists: Vec<Vec<Item>>) -> Vec<Item> {
    todo!("WAIT-1")
}

/// WAIT-3: the items of `next` only: an item in `previous` whose source is not in `next` is
/// dropped; one still there keeps its `since` from `previous`.
pub fn carry(previous: &[Item], next: Vec<Item>) -> Vec<Item> {
    next.into_iter()
        .map(|mut item| {
            if let Some(old) = previous.iter().find(|old| old.source == item.source) {
                item.since = old.since;
            }
            item
        })
        .collect()
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

/// WAIT-2: mark the CalDAV task behind `item` done; Err(CoreError::Parse(..)) for an item that is not a CalDAV task.
pub fn tick_caldav(caldav: &dyn CalDav, item: &Item) -> Result<Source, CoreError> {
    match &item.source {
        Source::CalDavTask { uid } => {
            caldav.mark_done(uid)?;
            Ok(item.source.clone())
        }
        other => Err(CoreError::Parse(format!(
            "only a CalDAV task can be ticked done, not {other:?}"
        ))),
    }
}

/// WAIT-1, WAIT-2: my CalDAV tasks (U7: waits for the address and my yes for a credential).
pub trait CalDav: Send + Sync {
    fn open_tasks(&self) -> Result<Vec<(String, String)>, CoreError>;
    fn mark_done(&self, uid: &str) -> Result<(), CoreError>;
}
