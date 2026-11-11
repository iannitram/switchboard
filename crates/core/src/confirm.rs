//! CORE-3, BAK-3 (#347): the confirm step in the app. A repo delete or a restore over a live
//! repo asked for from the CLI or MCP waits in Waiting on you; here the app confirms it. The
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
use crate::rules::{self, Caller, GuardedAction};
use crate::settings::Settings;
use crate::waiting::{Item, LocalItems, Source};
use crate::CoreError;

/// The action of a restore request whose first confirm held something back. An item with this
/// action is confirmed with `Pass::Held`. A new request from the CLI for the same repo writes
/// the item afresh, and its confirm is a whole restore again.
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
/// The item's source must be a guarded action whose text ends in ` to delete <repo>` or
/// ` to restore over <repo>` (what the CLI writes), `<repo>` must be one `owner/name`, and the
/// item's link must be that repo's GitHub address. Anything else is no request: the app never
/// guesses which repo was meant.
pub fn guarded_request(item: &Item) -> Option<GuardedAction> {
    let Source::GuardedAction { text } = &item.source else {
        return None;
    };
    let (_, request) = text.rsplit_once(" to ")?;
    let (repo, delete) = match request.strip_prefix("restore over ") {
        Some(repo) => (repo, false),
        None => (request.strip_prefix("delete ")?, true),
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

/// What a confirm runs, so tests can stand in for GitHub.
pub struct Actions<'a> {
    /// A restore over the live repo it is given: the whole restore, or only what was held back.
    pub restore: &'a dyn Fn(&Caller, &str, &Pass) -> Result<RestoreReport, CoreError>,
    /// Archives the repo it is given on GitHub.
    pub archive: &'a dyn Fn(&Caller, &str) -> Result<Archived, CoreError>,
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
/// Every refusal and every failed action leaves the file untouched.
pub fn confirm_with(
    data_dir: &Path,
    text: &str,
    typed: &str,
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
            "{text}: does not name one repo to delete or restore over"
        )));
    };
    rules::guard(caller, &action).into_result()?;
    let what = worded(&action);
    match &action {
        GuardedAction::RestoreOverLive { repo } => {
            typed_is(&what, repo, typed)?;
            let pass = if item.action == HELD_ACTION {
                Pass::Held(records.get(text)?.unwrap_or_default())
            } else {
                Pass::Whole
            };
            let report = (actions.restore)(caller, repo, &pass)?;
            let cleared = !report.issues.held();
            if cleared {
                local.remove(&source)?;
                records.set(text, None)?;
            } else {
                records.set(text, Some(&report.made))?;
                local.add(Item {
                    why: format!(
                        "The restore over {repo} pushed the backup's refs; {}. A confirm now \
                         only queues and sends those: it pushes nothing again (CORE-3).",
                        held_back(&report)
                    ),
                    action: HELD_ACTION.to_string(),
                    ..item
                })?;
            }
            Ok(Done {
                what: match pass {
                    Pass::Whole => what,
                    Pass::Held(_) => format!("{what} (what was held back)"),
                },
                lines: lines_of(&report, &pass),
                cleared,
            })
        }
        GuardedAction::DeleteRepo { repo } => {
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

    /// Keeps `made` for the request `text`, or drops its record (`None`). Pretty JSON to
    /// `held-restores.json.tmp`, then renamed over the file; no change writes nothing.
    fn set(&self, text: &str, made: Option<&Made>) -> Result<(), CoreError> {
        let mut all = self.load()?;
        let changed = match made {
            Some(made) => all.insert(text.to_string(), made.clone()).as_ref() != Some(made),
            None => all.remove(text).is_some(),
        };
        if !changed {
            return Ok(());
        }
        let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", self.path.display()));
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let mut body = serde_json::to_string_pretty(&all)
            .map_err(|e| CoreError::Parse(format!("{}: {e}", self.path.display())))?;
        body.push('\n');
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, body).map_err(io)?;
        std::fs::rename(&tmp, &self.path).map_err(io)
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
    let restore = |caller: &Caller, repo: &str, pass: &Pass| {
        let backups = match settings.nextcloud_backups.as_deref() {
            Some(path) if !path.as_os_str().is_empty() => path,
            _ => return Err(CoreError::MissingPath("nextcloud_backups")),
        };
        let fleet = FleetBridge::new(settings.pipeline_repo()?);
        let folder = backup_run::folder(backups, repo);
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
    confirm_with(
        data_dir,
        text,
        typed,
        caller,
        &Actions {
            restore: &restore,
            archive: &archive,
        },
    )
}
