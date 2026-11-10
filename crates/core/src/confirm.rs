//! CORE-3, BAK-3 (#347): the confirm step in the app. A repo delete or a restore over a live
//! repo asked for from the CLI or MCP waits in Waiting on you; here the app confirms it. The
//! request is read back from its item, `rules::guard` decides (only the app, only with my
//! confirm, never inside a job worktree), the action runs, and its item leaves the list
//! (WAIT-3). A refusal runs nothing and leaves the list as it was.
//!
//! A restore over a live repo runs `backup::restore_over_live` from the repo's backup folder.
//! A repo delete is still refused after the confirm: the core has no repo delete yet (#347).

use std::path::Path;

use serde::Serialize;

use crate::backup::{self, RestoreReport};
use crate::backup_run;
use crate::fleet::FleetBridge;
use crate::rules::{self, Caller, GuardedAction};
use crate::settings::Settings;
use crate::waiting::{Item, LocalItems, Source};
use crate::CoreError;

/// What a confirmed action did, as the face shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Done {
    /// The action as the guard words it, e.g. `restore over owner/name`.
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

/// The guard's wording for a delete or a restore.
fn worded(action: &GuardedAction) -> String {
    match action {
        GuardedAction::Merge { repo, pr } => format!("merge {repo}#{pr}"),
        GuardedAction::DeleteRepo { repo } => format!("delete {repo}"),
        GuardedAction::RestoreOverLive { repo } => format!("restore over {repo}"),
    }
}

/// The restore's report as plain lines.
fn lines_of(report: &RestoreReport) -> Vec<String> {
    let issues = &report.issues;
    let mut lines = vec![
        format!("{} refs pushed", report.refs),
        format!(
            "{} labels and {} milestones queued",
            report.labels, report.milestones
        ),
        format!(
            "{} issues and {} comments queued",
            issues.issues, issues.comments
        ),
    ];
    if issues.issues_there > 0 || issues.comments_there > 0 {
        lines.push(format!(
            "{} issues and {} comments were there already",
            issues.issues_there, issues.comments_there
        ));
    }
    if issues.comments_waiting > 0 {
        lines.push(format!(
            "{} comments wait for their issues: confirm again once the create queue has made them",
            issues.comments_waiting
        ));
    }
    lines.extend(report.left_out.iter().cloned());
    lines.extend(issues.left_out.iter().cloned());
    lines
}

/// Confirm the guarded request whose Waiting on you text is `text`, for `caller`; `restore`
/// runs a restore over the live repo it is given.
///
/// In order: the request must be waiting in `<data>/waiting/local.json` (`Refused` when it is
/// not) and name one repo (`guarded_request`); `rules::guard` must allow it for `caller`
/// (`Refused` with the guard's text); then a restore runs through `restore` and a delete is
/// `Refused`, because no repo delete is built. Only a restore that put everything back removes
/// its item; one that held comments back keeps it, so a second confirm can queue them. Every
/// refusal and every failed restore leaves the file untouched.
pub fn confirm_with(
    data_dir: &Path,
    text: &str,
    caller: &Caller,
    restore: &dyn Fn(&Caller, &str) -> Result<RestoreReport, CoreError>,
) -> Result<Done, CoreError> {
    let local = LocalItems::new(data_dir);
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
            let report = restore(caller, repo)?;
            let cleared = report.issues.comments_waiting == 0;
            if cleared {
                local.remove(&source)?;
            }
            Ok(Done {
                what,
                lines: lines_of(&report),
                cleared,
            })
        }
        GuardedAction::DeleteRepo { .. } => Err(CoreError::Refused(format!(
            "{what}: Switchboard has no repo delete yet; nothing was deleted"
        ))),
        // `guarded_request` never answers a merge.
        GuardedAction::Merge { .. } => Err(CoreError::Refused(what)),
    }
}

/// `confirm_with` as the app runs it: a restore reads the repo's backup in
/// `<nextcloud_backups>/github/<owner>/<name>/` and goes to GitHub through
/// `backup::restore_over_live`, with the create queue of the pipeline repo in `settings`.
pub fn confirm(
    settings: &Settings,
    data_dir: &Path,
    text: &str,
    caller: &Caller,
) -> Result<Done, CoreError> {
    confirm_with(data_dir, text, caller, &|caller, repo| {
        let backups = match settings.nextcloud_backups.as_deref() {
            Some(path) if !path.as_os_str().is_empty() => path,
            _ => return Err(CoreError::MissingPath("nextcloud_backups")),
        };
        let fleet = FleetBridge::new(settings.pipeline_repo()?);
        backup::restore_over_live(
            caller,
            &fleet,
            &backup_run::folder(backups, repo),
            repo,
            caller.confirmed_in_app,
        )
    })
}
