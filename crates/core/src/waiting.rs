//! `waiting`: Waiting on you. One list of everything that needs me, each item with what, why,
//! where it came from, and one action (WAIT-1); ticking a CalDAV task marks it done (WAIT-2);
//! an item whose source clears drops at the next read (WAIT-3).

use std::path::{Path, PathBuf};

use crate::clock::{Clock, SystemClock, UnixSeconds};
use crate::github::snapshot::SnapshotStore;
use crate::github::RepoSnapshot;
use crate::rules::GuardedAction;
use crate::sky::model::PRIORITY_FILE;
use crate::sky::priority::{self, PriorityCopy, STALE_DAYS};
use crate::CoreError;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
/// ladders at Fable), for every repo. A FAIL counts only while it names the PR's head: a
/// commit after it answered it (#342).
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
                .is_some_and(|v| v.verdict == "FAIL" && v.needs_me && v.sha == pr.head_sha)
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

/// WAIT-1, FD-8: a Claude handoff not yet opened; `summary` is the chat's one-line ask.
pub fn handoff_item(handoff_id: &str, summary: &str, at: UnixSeconds) -> Item {
    Item {
        what: format!("Open the Claude handoff: {summary}"),
        why: "A chat was handed to Claude and is not opened yet (FD-8).".to_string(),
        source: Source::Handoff {
            handoff_id: handoff_id.to_string(),
        },
        link: format!("handoff:{handoff_id}"),
        action: "Open in Claude".to_string(),
        since: at,
    }
}

/// WAIT-1, CORE-3: a guarded action asked for from the CLI or MCP; `text` is the guard's
/// `Verdict::ToWaiting` text.
pub fn guarded_item(action: &GuardedAction, text: &str, at: UnixSeconds) -> Item {
    let link = match action {
        GuardedAction::Merge { repo, pr } => format!("https://github.com/{repo}/pull/{pr}"),
        GuardedAction::DeleteRepo { repo } | GuardedAction::RestoreOverLive { repo } => {
            format!("https://github.com/{repo}")
        }
    };
    Item {
        what: text.to_string(),
        why: "Asked for from the CLI or MCP; only the app can do it, after I confirm (CORE-3)."
            .to_string(),
        source: Source::GuardedAction {
            text: text.to_string(),
        },
        link,
        action: "Confirm in the app".to_string(),
        since: at,
    }
}

/// WAIT-1, BAK-2: a refused backup; `reason` is `backup::refuse_reason`'s text, `dir` the
/// backup folder that kept the old copy.
pub fn refused_backup_item(repo: &str, reason: &str, dir: &Path, at: UnixSeconds) -> Item {
    Item {
        what: format!("Check the {repo} backup"),
        why: format!("The new backup was refused and the old one kept: {reason}"),
        source: Source::RefusedBackup {
            repo: repo.to_string(),
        },
        link: dir.display().to_string(),
        action: "Open the backup folder".to_string(),
        since: at,
    }
}

/// WAIT-1, SKY-9: the item for a stale priority copy, `None` while it is fresh.
pub fn stale_priority_item(copy: &PriorityCopy, path: &Path, now: UnixSeconds) -> Option<Item> {
    if !priority::is_stale(copy, now) {
        return None;
    }
    Some(Item {
        what: "Refresh the priority copy".to_string(),
        why: format!(
            "The priority copy is {} days old; the sky does not trust it after {STALE_DAYS} days.",
            priority::age_days(copy, now)
        ),
        source: Source::StalePriority,
        link: path.display().to_string(),
        action: "Refresh in Claude".to_string(),
        since: copy.updated_at + STALE_DAYS * 86_400,
    })
}

/// WAIT-1, SKY-9 (#344): the priority copy's item from `<data>/priority.json`: none without a
/// copy or while it is fresh; one when it is stale, or when it does not read (a copy the sky
/// cannot use is no more trusted than a stale one).
pub fn priority_items(data_dir: &Path, now: UnixSeconds) -> Vec<Item> {
    let path = data_dir.join(PRIORITY_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => return vec![unreadable_priority_item(&path, &e.to_string(), now)],
    };
    match priority::parse(&text) {
        Ok(copy) => stale_priority_item(&copy, &path, now).into_iter().collect(),
        Err(e) => vec![unreadable_priority_item(&path, &e.to_string(), now)],
    }
}

fn unreadable_priority_item(path: &Path, why: &str, now: UnixSeconds) -> Item {
    Item {
        what: "Refresh the priority copy".to_string(),
        why: format!("The priority copy does not read ({why}); the sky cannot use it."),
        source: Source::StalePriority,
        link: path.display().to_string(),
        action: "Refresh in Claude".to_string(),
        since: now,
    }
}

/// WAIT-1: the items no snapshot holds, kept in `<data>/waiting/local.json`.
pub struct LocalItems {
    path: PathBuf,
}

impl LocalItems {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("waiting").join("local.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// No file is no items; a file that does not parse is a `CoreError::Parse` naming it.
    pub fn load(&self) -> Result<Vec<Item>, CoreError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(CoreError::Io(format!("{}: {e}", self.path.display()))),
        };
        serde_json::from_str(&text)
            .map_err(|e| CoreError::Parse(format!("{}: {e}", self.path.display())))
    }

    /// Replace the item with the same `source` in place, or push it at the end.
    pub fn add(&self, item: Item) -> Result<(), CoreError> {
        let mut items = self.load()?;
        match items.iter().position(|old| old.source == item.source) {
            Some(i) => items[i] = item,
            None => items.push(item),
        }
        self.write(&items)
    }

    /// True when an item was removed; nothing removed writes nothing.
    pub fn remove(&self, source: &Source) -> Result<bool, CoreError> {
        let mut items = self.load()?;
        let before = items.len();
        items.retain(|item| item.source != *source);
        if items.len() == before {
            return Ok(false);
        }
        self.write(&items)?;
        Ok(true)
    }

    /// Pretty JSON to `local.json.tmp`, then renamed over `local.json`.
    fn write(&self, items: &[Item]) -> Result<(), CoreError> {
        let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", self.path.display()));
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let mut text = serde_json::to_string_pretty(items)
            .map_err(|e| CoreError::Parse(format!("{}: {e}", self.path.display())))?;
        text.push('\n');
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(io)?;
        std::fs::rename(&tmp, &self.path).map_err(io)
    }
}

/// WAIT-1: merge every source's items into one list, newest first; ties by `what`.
pub fn merge(lists: Vec<Vec<Item>>) -> Vec<Item> {
    let mut items: Vec<Item> = lists.into_iter().flatten().collect();
    items.sort_by(|a, b| b.since.cmp(&a.since).then_with(|| a.what.cmp(&b.what)));
    items
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

/// WAIT-W: the one list the Waiting view shows: the snapshot's items and the local ones,
/// merged newest first.
pub fn assemble(snaps: &[RepoSnapshot], local: &[Item]) -> Vec<Item> {
    merge(vec![from_snapshots(snaps), local.to_vec()])
}

/// WAIT-W: `load_at` now.
pub fn load(data_dir: &Path) -> Result<Vec<Item>, CoreError> {
    load_at(data_dir, SystemClock.now())
}

/// WAIT-W: `assemble` over the snapshot store and the local items in the app data folder, with
/// the priority copy's item at `now` (`priority_items`, #344).
pub fn load_at(data_dir: &Path, now: UnixSeconds) -> Result<Vec<Item>, CoreError> {
    let snaps = SnapshotStore::new(data_dir).load_all()?;
    let local = LocalItems::new(data_dir).load()?;
    Ok(merge(vec![
        assemble(&snaps, &local),
        priority_items(data_dir, now),
    ]))
}
