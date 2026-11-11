//! CORE-3, BAK-3 (#347): the confirm step in the app. A repo archive (the `delete` request) or
//! a restore over a live repo asked for from the CLI or MCP waits in Waiting on you; here the app confirms it. The
//! request is read back from its item, `rules::guard` decides (only the app, only with my
//! confirm, never inside a job worktree), the repo's `owner/name` I typed must be the request's,
//! the action runs, and its item leaves the list (WAIT-3). A refusal runs nothing and leaves
//! the list as it was.
//!
//! A delete request archives the repo on GitHub (`github::archive`); Switchboard never deletes
//! one. A restore over a live repo runs `backup::restore_over_live` from the repo's backup
//! folder. When that restore held something back (comments, closes) until its issues are
//! there, its item stays with the action `HELD_ACTION`, and the confirm of such an item only
//! does what was held back: it pushes no ref again. What the first restore made (the issues and
//! milestones the create queue made for it) is kept beside the item, in
//! `<data>/waiting/held-restores.json`, and the second confirm closes and renumbers only those.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::backup::{self, Made, Pass, RestoreReport};
use crate::backup_run;
use crate::fleet::FleetBridge;
use crate::github::archive::{self, Archived};
use crate::github::transport::HttpTransport;
use crate::rules::{self, Caller, Face, GuardedAction};
use crate::settings::Settings;
use crate::waiting::{Item, LocalItems, Source};
use crate::CoreError;

/// The action of a restore request whose first confirm held something back. An item with this
/// action is confirmed with `Pass::Held`. While it waits, no face files a second restore
/// request for that repo (`LocalItems::add_new` keeps this item, whichever face asks). A whole
/// restore again needs the request dropped first, then asked for anew.
pub const HELD_ACTION: &str = "Queue what the restore held back";

/// What a confirmed action did, as the face shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Done {
    /// What was done, e.g. `restore over owner/name` or `archive owner/name`.
    pub what: String,
    /// What was done, one plain line each.
    pub lines: Vec<String>,
    /// True when the request left Waiting on you; false when it stays for a second confirm.
    pub cleared: bool,
}

/// The delete or restore request a Waiting on you item holds, `None` when it holds none.
///
/// The item's source must be a guarded action whose text ends in ` to archive <repo>` or
/// ` to restore over <repo>` (what the CLI and MCP write; ` to delete <repo>`, the wording
/// before #347's SPEC change, is read as the same archive request), `<repo>` must be one `owner/name`, and the
/// item's link must be that repo's GitHub address. Anything else is no request: the app never
/// guesses which repo was meant.
pub fn guarded_request(item: &Item) -> Option<GuardedAction> {
    let Source::GuardedAction { text } = &item.source else {
        return None;
    };
    let (_, request) = text.rsplit_once(" to ")?;
    let (repo, delete) = match request.strip_prefix("restore over ") {
        Some(repo) => (repo, false),
        None => (
            request
                .strip_prefix("archive ")
                .or_else(|| request.strip_prefix("delete "))?,
            true,
        ),
    };
    let mut parts = repo.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    let plain = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    if parts.next().is_some() || !plain(owner) || !plain(name) {
        return None;
    }
    if item.link != format!("https://github.com/{repo}") {
        return None;
    }
    let repo = repo.to_string();
    Some(if delete {
        GuardedAction::DeleteRepo { repo }
    } else {
        GuardedAction::RestoreOverLive { repo }
    })
}

/// #426: the text of the waiting request for `action` (an archive of, or a restore over, one
/// repo; the repo's `owner/name` compared without case), `None` when none waits. The CLI names
/// a request by what it asks, as `drop_requests_for` does; the confirm step finds it by this text.
pub fn request_text_for(
    data_dir: &Path,
    action: &GuardedAction,
) -> Result<Option<String>, CoreError> {
    let same = |a: &GuardedAction, b: &GuardedAction| match (a, b) {
        (GuardedAction::DeleteRepo { repo: a }, GuardedAction::DeleteRepo { repo: b })
        | (
            GuardedAction::RestoreOverLive { repo: a },
            GuardedAction::RestoreOverLive { repo: b },
        ) => a.eq_ignore_ascii_case(b),
        _ => false,
    };
    Ok(LocalItems::new(data_dir)
        .load()?
        .iter()
        .find(|item| guarded_request(item).is_some_and(|found| same(&found, action)))
        .and_then(|item| match &item.source {
            Source::GuardedAction { text } => Some(text.clone()),
            _ => None,
        }))
}

/// What the confirm does, in the words the face shows: a delete request archives.
fn worded(action: &GuardedAction) -> String {
    match action {
        GuardedAction::Merge { repo, pr } => format!("merge {repo}#{pr}"),
        GuardedAction::DeleteRepo { repo } => format!("archive {repo}"),
        GuardedAction::RestoreOverLive { repo } => format!("restore over {repo}"),
    }
}

/// The restore's report as plain lines; a `Pass::Held` report has no refs, labels or issues to
/// speak of.
fn lines_of(report: &RestoreReport, pass: &Pass) -> Vec<String> {
    let issues = &report.issues;
    let mut lines = match pass {
        Pass::Whole => vec![
            format!("{} refs pushed", report.refs),
            format!(
                "{} labels and {} milestones queued",
                report.labels, report.milestones
            ),
            format!(
                "{} issues and {} comments queued",
                issues.issues, issues.comments
            ),
        ],
        Pass::Held(_) => vec![
            "no ref was pushed again".to_string(),
            format!("{} comments queued", issues.comments),
        ],
    };
    if *pass == Pass::Whole && (issues.issues_there > 0 || issues.comments_there > 0) {
        lines.push(format!(
            "{} issues and {} comments were there already",
            issues.issues_there, issues.comments_there
        ));
    }
    if issues.closed > 0 || issues.milestones_closed > 0 {
        lines.push(format!(
            "{} issues and {} milestones closed, as in the backup",
            issues.closed, issues.milestones_closed
        ));
    }
    if issues.renumbered > 0 {
        lines.push(format!(
            "{} issue bodies given the new issue numbers",
            issues.renumbered
        ));
    }
    if issues.held() {
        lines.push(format!(
            "{}: confirm again once the create queue has made the issues",
            held_back(report)
        ));
    }
    lines.extend(report.left_out.iter().cloned());
    lines.extend(issues.left_out.iter().cloned());
    lines
}

/// What a restore held back, e.g. `2 comments and 1 closes or body changes wait`.
fn held_back(report: &RestoreReport) -> String {
    format!(
        "{} comments and {} closes or body changes wait",
        report.issues.comments_waiting, report.issues.state_waiting
    )
}

/// #347: a restore of a repo's backup (first name) into a new private repo (second name).
pub type RestoreNew<'a> =
    dyn Fn(&Caller, &str, &str, &Pass) -> Result<RestoreReport, CoreError> + 'a;

/// What a confirm runs, so tests can stand in for GitHub.
pub struct Actions<'a> {
    /// A restore over the live repo it is given: the whole restore, or only what was held back.
    pub restore: &'a dyn Fn(&Caller, &str, &Pass) -> Result<RestoreReport, CoreError>,
    /// Archives the repo it is given on GitHub.
    pub archive: &'a dyn Fn(&Caller, &str) -> Result<Archived, CoreError>,
    /// #347: a restore of the first repo's backup into the second, a new private repo: the
    /// whole restore, or only what was held back. The live repo is left alone.
    pub restore_new: &'a RestoreNew<'a>,
}

/// Confirm the guarded request whose Waiting on you text is `text`, for `caller`, who typed
/// the repo's name as `typed`.
///
/// In order: the request must be waiting in `<data>/waiting/local.json` (`Refused` when it is
/// not) and name one repo (`guarded_request`); `rules::guard` must allow it for `caller`
/// (`Refused` with the guard's text); `typed` must be that repo's `owner/name`, letter for
/// letter (`Refused` when it is not, or is empty). Only then does anything run:
///
/// - a delete request archives the repo through `actions.archive` and its item leaves the list;
/// - a restore request runs `actions.restore` with `Pass::Whole`. When it put everything back
///   its item leaves the list; when it held something back the item stays, with `HELD_ACTION`
///   and what waits in its `why`;
/// - a restore request whose item has `HELD_ACTION` runs `actions.restore` with `Pass::Held`
///   and the record of what the restore made, kept beside the item (`HeldRecords`; an item
///   with no record gets an empty one, so nothing is closed), and leaves the list once nothing
///   is held back. Each time the item stays, the report's record is kept beside it.
///
/// The record of held restores is read before a restore runs: one that cannot be read stops
/// the confirm there. Every refusal and every failed action leaves the file untouched.
pub fn confirm_with(
    data_dir: &Path,
    text: &str,
    typed: &str,
    caller: &Caller,
    actions: &Actions<'_>,
) -> Result<Done, CoreError> {
    confirm_into_with(data_dir, text, typed, None, caller, actions)
}

/// `confirm_with`, and with `into` (#347) a restore request goes to that new private repo
/// (`owner/name`, another name than the request's repo) through `actions.restore_new` instead
/// of over the live one. The guard and the typed name are the same: the name typed is still
/// the request's repo. A name that is not an `owner/name`, or is the repo itself, is `Refused`
/// before anything runs. A restore that held something back keeps the new repo's name in its
/// record, and its next confirm goes there whatever `into` says then. `into` on an archive
/// request is `Refused`.
pub fn confirm_into_with(
    data_dir: &Path,
    text: &str,
    typed: &str,
    into: Option<&str>,
    caller: &Caller,
    actions: &Actions<'_>,
) -> Result<Done, CoreError> {
    let local = LocalItems::new(data_dir);
    let records = HeldRecords::new(data_dir);
    let source = Source::GuardedAction {
        text: text.to_string(),
    };
    let Some(item) = local.load()?.into_iter().find(|i| i.source == source) else {
        return Err(CoreError::Refused(format!(
            "{text}: no such request is waiting on you"
        )));
    };
    let Some(action) = guarded_request(&item) else {
        return Err(CoreError::Refused(format!(
            "{text}: does not name one repo to archive or restore over"
        )));
    };
    // A restore into a new private repo (#426) has its own guard: the CLI may confirm it with
    // the name typed, as it may finish what such a restore held back. Over the live repo, and
    // an archive, stay the app's.
    let to_new = matches!(action, GuardedAction::RestoreOverLive { .. })
        && (into.is_some() || (caller.face == Face::Cli && item.action == HELD_ACTION));
    match &action {
        GuardedAction::RestoreOverLive { repo } if to_new => {
            rules::guard_restore_new(caller, repo).into_result()?
        }
        _ => rules::guard(caller, &action).into_result()?,
    }
    let what = worded(&action);
    match &action {
        GuardedAction::RestoreOverLive { repo } => {
            typed_is(&what, repo, typed)?;
            // Read before anything runs: a record that cannot be read refuses the confirm
            // here, not after the restore pushed and its record could not be kept.
            let kept = records.get(text)?;
            let held = item.action == HELD_ACTION;
            let new = if held {
                kept.as_ref().and_then(|made| made.into.clone())
            } else {
                into.map(str::to_string)
            };
            if let Some(new) = &new {
                new_repo_ok(&what, repo, new)?;
            } else if to_new {
                return Err(CoreError::Refused(format!(
                    "{what}: only the app restores over a live repo; nothing was changed"
                )));
            }
            let pass = if held {
                Pass::Held(kept.unwrap_or_default())
            } else {
                Pass::Whole
            };
            let mut report = match &new {
                Some(new) => (actions.restore_new)(caller, repo, new, &pass)?,
                None => (actions.restore)(caller, repo, &pass)?,
            };
            report.made.into = new.clone();
            let (what, target) = match &new {
                Some(new) => (
                    format!("restore {repo} into the new private repo {new}"),
                    format!("restore of {repo} into {new}"),
                ),
                None => (what, format!("restore over {repo}")),
            };
            let cleared = !report.issues.held();
            if cleared {
                local.remove(&source)?;
                records.set(text, None)?;
            } else {
                records.set(text, Some(&report.made))?;
                local.add(Item {
                    why: format!(
                        "The {target} pushed the backup's refs; {}. A confirm now \
                         only queues and sends those: it pushes nothing again (CORE-3).",
                        held_back(&report)
                    ),
                    action: HELD_ACTION.to_string(),
                    ..item
                })?;
            }
            let mut lines = lines_of(&report, &pass);
            if let (Some(new), Pass::Whole) = (&new, &pass) {
                lines.insert(
                    0,
                    format!("{new} was created as a private repo; {repo} was left as it is"),
                );
            }
            Ok(Done {
                what: match pass {
                    Pass::Whole => what,
                    Pass::Held(_) => format!("{what} (what was held back)"),
                },
                lines,
                cleared,
            })
        }
        GuardedAction::DeleteRepo { repo } => {
            if into.is_some() {
                return Err(CoreError::Refused(format!(
                    "{what}: an archive has no new repo to go to; nothing was changed"
                )));
            }
            typed_is(&what, repo, typed)?;
            let line = match (actions.archive)(caller, repo)? {
                Archived::Now => {
                    format!("{repo} is archived on GitHub: read-only, nothing was deleted")
                }
                Archived::Already => format!("{repo} was archived already; nothing was changed"),
            };
            local.remove(&source)?;
            Ok(Done {
                what,
                lines: vec![line],
                cleared: true,
            })
        }
        // `guarded_request` never answers a merge.
        GuardedAction::Merge { .. } => Err(CoreError::Refused(what)),
    }
}

/// What turning a request down did, as the face shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dropped {
    /// What the dropped request asked for, e.g. `archive owner/name`.
    pub what: String,
    /// One plain line for each request that left Waiting on you.
    pub lines: Vec<String>,
}

/// #347, turn a request down: the archive or restore request whose Waiting on you text is
/// `text` leaves the list, and the record a held restore kept beside it goes with it. Nothing
/// else happens: no archive, no push, nothing queued.
///
/// `Refused` when no such request is waiting, when the item is not an archive or restore
/// request (`guarded_request`), or inside a job worktree. A refusal changes nothing.
pub fn drop_request(data_dir: &Path, text: &str, caller: &Caller) -> Result<Dropped, CoreError> {
    let source = Source::GuardedAction {
        text: text.to_string(),
    };
    let items = LocalItems::new(data_dir).load()?;
    let Some(item) = items.iter().find(|i| i.source == source) else {
        return Err(CoreError::Refused(format!(
            "{text}: no such request is waiting on you; nothing was dropped"
        )));
    };
    let Some(action) = guarded_request(item) else {
        return Err(CoreError::Refused(format!(
            "{text}: is not an archive or restore request; nothing was dropped"
        )));
    };
    drop_items(data_dir, &action, &[item], caller)
}

/// `drop_request` for the CLI and MCP, which name the request by what it asks: every waiting
/// request for `action` (an archive of, or a restore over, one repo) leaves the list, whoever
/// filed it. `Refused` when none is waiting.
pub fn drop_requests_for(
    data_dir: &Path,
    action: &GuardedAction,
    caller: &Caller,
) -> Result<Dropped, CoreError> {
    let what = worded(action);
    if matches!(action, GuardedAction::Merge { .. }) {
        return Err(CoreError::Refused(format!(
            "{what}: is not an archive or restore request; nothing was dropped"
        )));
    }
    if caller.in_job_worktree {
        return Err(in_worktree(&what));
    }
    let items = LocalItems::new(data_dir).load()?;
    let asked: Vec<&Item> = items
        .iter()
        .filter(|item| guarded_request(item).as_ref() == Some(action))
        .collect();
    if asked.is_empty() {
        return Err(CoreError::Refused(format!(
            "{what}: no such request is waiting on you; nothing was dropped"
        )));
    }
    drop_items(data_dir, action, &asked, caller)
}

fn in_worktree(what: &str) -> CoreError {
    CoreError::Refused(format!(
        "drop the request to {what}: refused inside a job worktree"
    ))
}

/// Removes `items`, each a waiting request for `action`, and their held-restore records. The
/// records are read first, so a file that cannot be read stops the drop before anything goes.
fn drop_items(
    data_dir: &Path,
    action: &GuardedAction,
    items: &[&Item],
    caller: &Caller,
) -> Result<Dropped, CoreError> {
    let what = worded(action);
    if caller.in_job_worktree {
        return Err(in_worktree(&what));
    }
    let local = LocalItems::new(data_dir);
    let records = HeldRecords::new(data_dir);
    records.load()?;
    let mut lines = Vec::new();
    for item in items {
        local.remove(&item.source)?;
        if let Source::GuardedAction { text } = &item.source {
            records.set(text, None)?;
        }
        lines.push(format!(
            "the request to {what} was dropped; nothing was archived, pushed or queued"
        ));
        if item.action == HELD_ACTION {
            lines.push("what the restore held back stays undone".to_string());
        }
    }
    Ok(Dropped { what, lines })
}

/// What each held restore made (`RestoreReport::made`), by its request's text, in
/// `<data>/waiting/held-restores.json`, beside the Waiting item it belongs to.
pub struct HeldRecords {
    path: PathBuf,
}

impl HeldRecords {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("waiting").join("held-restores.json"),
        }
    }

    fn load(&self) -> Result<BTreeMap<String, Made>, CoreError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(CoreError::Io(format!("{}: {e}", self.path.display()))),
        };
        serde_json::from_str(&text)
            .map_err(|e| CoreError::Parse(format!("{}: {e}", self.path.display())))
    }

    /// The record kept for the request `text`, `None` when there is none.
    pub fn get(&self, text: &str) -> Result<Option<Made>, CoreError> {
        Ok(self.load()?.remove(text))
    }

    /// Keeps `made` for the request `text`, or drops its record (`None`). A read, change and
    /// write under the file's lock (#431), pretty JSON to a temp file of this run's own
    /// (`lock::temp_path`), then renamed over the file; no change writes nothing.
    fn set(&self, text: &str, made: Option<&Made>) -> Result<(), CoreError> {
        let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", self.path.display()));
        if made.is_none() && !self.load()?.contains_key(text) {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let _held = crate::lock::FileLock::take_for(
            &self.path,
            crate::waiting::LOCAL_LOCK_WAIT,
            "a change to the held restores",
        )?;
        let mut all = self.load()?;
        let changed = match made {
            Some(made) => all.insert(text.to_string(), made.clone()).as_ref() != Some(made),
            None => all.remove(text).is_some(),
        };
        if !changed {
            return Ok(());
        }
        let mut body = serde_json::to_string_pretty(&all)
            .map_err(|e| CoreError::Parse(format!("{}: {e}", self.path.display())))?;
        body.push('\n');
        let tmp = crate::lock::temp_path(&self.path, ".tmp");
        std::fs::write(&tmp, body).map_err(io)?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            io(e)
        })
    }
}

/// `Refused` unless `typed` is `repo`, letter for letter.
fn typed_is(what: &str, repo: &str, typed: &str) -> Result<(), CoreError> {
    if typed == repo {
        return Ok(());
    }
    Err(CoreError::Refused(format!(
        "{what}: type {repo} to confirm; the name typed does not match, nothing was changed"
    )))
}

/// #347: `new` can be the new private repo of a restore of `repo`: an `owner/name` of GitHub's
/// letters, and another name than `repo`.
fn new_repo_ok(what: &str, repo: &str, new: &str) -> Result<(), CoreError> {
    let part = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    let named = matches!(new.split_once('/'), Some((owner, name)) if part(owner) && part(name));
    if !named {
        return Err(CoreError::Refused(format!(
            "{what}: {new} is not an owner/name for the new private repo; nothing was changed"
        )));
    }
    if new.eq_ignore_ascii_case(repo) {
        return Err(CoreError::Refused(format!(
            "{what}: {new} is the repo itself; a restore to a new private repo needs another \
             name; nothing was changed"
        )));
    }
    Ok(())
}

/// `confirm_with` as the app runs it: a restore reads the repo's backup in
/// `<nextcloud_backups>/github/<owner>/<name>/` and goes to GitHub through
/// `backup::restore_over_live` (or `backup::restore_held_over_live` for what was held back),
/// with the create queue of the pipeline repo in `settings`; an archive goes to GitHub through
/// `archive::archive_repo` with the stored token.
pub fn confirm(
    settings: &Settings,
    data_dir: &Path,
    text: &str,
    typed: &str,
    caller: &Caller,
) -> Result<Done, CoreError> {
    confirm_into(settings, data_dir, text, typed, None, caller)
}

/// `confirm`, and with `into` (#347) a restore goes to that new private repo through
/// `backup::restore_to_new` (`confirm_into_with`).
pub fn confirm_into(
    settings: &Settings,
    data_dir: &Path,
    text: &str,
    typed: &str,
    into: Option<&str>,
    caller: &Caller,
) -> Result<Done, CoreError> {
    let source = |repo: &str| {
        let backups = match settings.nextcloud_backups.as_deref() {
            Some(path) if !path.as_os_str().is_empty() => path,
            _ => return Err(CoreError::MissingPath("nextcloud_backups")),
        };
        let fleet = FleetBridge::new(settings.pipeline_repo()?);
        Ok((fleet, backup_run::folder(backups, repo)))
    };
    // #426: what a restore into a new repo made is kept here, so a retry can finish it.
    let marks = backup::Marks::new(data_dir);
    let restore_new = |caller: &Caller, repo: &str, new: &str, pass: &Pass| {
        let (fleet, folder) = source(repo)?;
        let confirmed = caller.confirmed_in_app;
        backup::restore_to_new(
            caller,
            &fleet,
            &folder,
            (repo, new),
            confirmed,
            pass,
            Some(&marks),
        )
    };
    let restore = |caller: &Caller, repo: &str, pass: &Pass| {
        let (fleet, folder) = source(repo)?;
        let confirmed = caller.confirmed_in_app;
        match pass {
            Pass::Whole => backup::restore_over_live(caller, &fleet, &folder, repo, confirmed),
            Pass::Held(made) => {
                backup::restore_held_over_live(caller, &fleet, &folder, repo, confirmed, made)
            }
        }
    };
    let archive = |caller: &Caller, repo: &str| {
        let tokens = crate::platform::GhAuthToken::default();
        archive::archive_repo(caller, &HttpTransport::new(&tokens)?, repo)
    };
    confirm_into_with(
        data_dir,
        text,
        typed,
        into,
        caller,
        &Actions {
            restore: &restore,
            archive: &archive,
            restore_new: &restore_new,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #431: held restores recorded at once, each by a store of its own as two processes
    /// would, all keep their record. Before, every save went through one fixed
    /// `held-restores.json.tmp` with no lock: records were lost or a save failed.
    #[test]
    fn held_restores_recorded_at_once_all_keep_their_record() {
        let data = std::env::temp_dir().join(format!("sb-431-held-{}", crate::lock::run_tag()));
        const WRITERS: usize = 6;
        const EACH: usize = 15;
        let start = std::sync::Arc::new(std::sync::Barrier::new(WRITERS));
        let running: Vec<_> = (0..WRITERS)
            .map(|w| {
                let (data, go) = (data.clone(), start.clone());
                std::thread::spawn(move || {
                    let records = HeldRecords::new(&data);
                    go.wait();
                    for n in 0..EACH {
                        let made = Made {
                            into: Some(format!("owner/new-{w}-{n}")),
                            ..Made::default()
                        };
                        records
                            .set(&format!("Restore {w}-{n}"), Some(&made))
                            .unwrap();
                    }
                })
            })
            .collect();
        for thread in running {
            thread.join().unwrap();
        }
        let all = HeldRecords::new(&data).load().unwrap();
        assert_eq!(all.len(), WRITERS * EACH, "records lost");
        let left: Vec<_> = std::fs::read_dir(data.join("waiting"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != "held-restores.json")
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = std::fs::remove_dir_all(&data);
    }
}
