//! CORE-3 (#347): a repo delete asked of Switchboard archives the repo on GitHub. Nothing is
//! ever deleted: the repo becomes read-only and can be unarchived on GitHub. Only after
//! `rules::guard` allows the delete request for the caller (the app, with my confirm).

use serde_json::json;

use crate::github::transport::GitHubTransport;
use crate::rules::{self, Caller, GuardedAction};
use crate::CoreError;

/// What `archive_repo` found or did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archived {
    /// The repo was archived by this call.
    Now,
    /// The repo was archived already; nothing was sent.
    Already,
}

/// Archive `repo` (`owner/name`) on GitHub for `caller`.
///
/// `rules::guard` must allow `DeleteRepo` for `caller`, else `Refused` with the guard's text
/// and no request at all; a `repo` that is not `owner/name` is an error, also with no request.
/// Then the repo is read: one that is not there is an error, one that
/// is archived already is `Already`. Otherwise one `PATCH /repos/<repo>` with
/// `{"archived": true}`, which must answer 200 with `archived: true`. No other field is sent,
/// and no DELETE is ever made.
pub fn archive_repo(
    caller: &Caller,
    api: &dyn GitHubTransport,
    repo: &str,
) -> Result<Archived, CoreError> {
    rules::guard(
        caller,
        &GuardedAction::DeleteRepo {
            repo: repo.to_string(),
        },
    )
    .into_result()?;
    let parts: Vec<&str> = repo.split('/').collect();
    let named = |part: &&str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    if parts.len() != 2 || !parts.iter().all(named) {
        return Err(CoreError::GitHub(format!(
            "{repo:?} is not an owner/name: nothing was archived"
        )));
    }
    let path = format!("/repos/{repo}");
    let found = api.rest_get(&path, None)?;
    match found.status {
        200 => {}
        404 => {
            return Err(CoreError::GitHub(format!(
                "{repo}: no such repo to archive"
            )))
        }
        other => return Err(CoreError::GitHub(format!("GET {path}: status {other}"))),
    }
    if found.body["archived"] == json!(true) {
        return Ok(Archived::Already);
    }
    let answer = api.rest_patch(&path, &json!({"archived": true}))?;
    if answer.status != 200 || answer.body["archived"] != json!(true) {
        return Err(CoreError::GitHub(format!(
            "PATCH {path}: status {}; {repo} was not archived (the token may not have the \
             right to change this repo)",
            answer.status
        )));
    }
    Ok(Archived::Now)
}
