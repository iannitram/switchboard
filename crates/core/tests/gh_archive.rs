//! CORE-3 (#347): a repo delete asked of Switchboard archives the repo on GitHub and never
//! deletes it. `archive_repo` against recorded answers: the guard refuses everyone but the app
//! with my confirm before any request, and the one write is `PATCH {"archived": true}`.

use serde_json::{json, Value};
use switchboard_core::github::archive::{archive_repo, Archived};
use switchboard_core::github::transport::{Recorded, RestAnswer};
use switchboard_core::rules::{Caller, Face, Role};
use switchboard_core::CoreError;

const REPO: &str = "owner/alpha";

fn answer(status: u16, body: Value) -> RestAnswer {
    RestAnswer {
        status,
        etag: None,
        body,
    }
}

fn app() -> Caller {
    Caller {
        face: Face::Gui,
        role: None,
        in_job_worktree: false,
        confirmed_in_app: true,
    }
}

fn log(api: &Recorded) -> Vec<String> {
    api.log.lock().unwrap().clone()
}

#[test]
fn the_app_with_my_confirm_archives_the_repo_with_one_patch_and_nothing_else() {
    let api = Recorded::new(
        Vec::new(),
        vec![answer(200, json!({"full_name": REPO, "archived": false}))],
    );
    assert_eq!(archive_repo(&app(), &api, REPO), Ok(Archived::Now));
    assert_eq!(
        log(&api),
        vec![
            format!("GET /repos/{REPO}"),
            format!(r#"PATCH /repos/{REPO} {{"archived":true}}"#),
        ],
        "only `archived` is sent; nothing is deleted"
    );
}

#[test]
fn a_repo_that_is_archived_already_gets_no_request_to_change_it() {
    let api = Recorded::new(
        Vec::new(),
        vec![answer(200, json!({"full_name": REPO, "archived": true}))],
    );
    assert_eq!(archive_repo(&app(), &api, REPO), Ok(Archived::Already));
    assert_eq!(log(&api), vec![format!("GET /repos/{REPO}")]);
}

#[test]
fn everyone_but_the_app_with_my_confirm_is_refused_before_any_request() {
    let callers = [
        Caller {
            confirmed_in_app: false,
            ..app()
        },
        Caller {
            in_job_worktree: true,
            ..app()
        },
        Caller {
            face: Face::Cli,
            role: Some(Role::ProjectManager),
            ..app()
        },
        Caller {
            face: Face::Mcp,
            role: Some(Role::ProjectManager),
            ..app()
        },
        Caller {
            face: Face::Cli,
            role: None,
            ..app()
        },
    ];
    let api = Recorded::new(Vec::new(), Vec::new());
    for who in callers {
        match archive_repo(&who, &api, REPO) {
            Err(CoreError::Refused(why)) => assert!(why.contains(REPO), "{why}"),
            other => panic!("{who:?}: expected a refusal, got {other:?}"),
        }
    }
    assert!(log(&api).is_empty(), "no request at all");
}

#[test]
fn a_repo_that_is_not_there_or_cannot_be_read_is_an_error_with_no_write() {
    for (status, needle) in [(404, "no such repo to archive"), (500, "status 500")] {
        let api = Recorded::new(Vec::new(), vec![answer(status, json!({}))]);
        match archive_repo(&app(), &api, REPO) {
            Err(CoreError::GitHub(why)) => assert!(why.contains(needle), "{why}"),
            other => panic!("expected an error, got {other:?}"),
        }
        assert_eq!(log(&api), vec![format!("GET /repos/{REPO}")]);
    }
}

#[test]
fn a_name_that_is_not_owner_name_is_refused_before_any_request() {
    let api = Recorded::new(Vec::new(), Vec::new());
    for bad in ["alpha", "owner/alpha/..", "../alpha", "owner/", "o w/a", ""] {
        assert!(archive_repo(&app(), &api, bad).is_err(), "{bad:?}");
    }
    assert!(log(&api).is_empty(), "no request at all");
}

#[test]
fn an_answer_that_does_not_say_archived_is_an_error_not_a_success() {
    for patch in [
        answer(403, json!({"message": "Must have admin rights"})),
        answer(200, json!({"full_name": REPO, "archived": false})),
    ] {
        let api = Recorded::new(
            Vec::new(),
            vec![answer(200, json!({"full_name": REPO, "archived": false}))],
        );
        api.patch.lock().unwrap().push(patch);
        match archive_repo(&app(), &api, REPO) {
            Err(CoreError::GitHub(why)) => assert!(why.contains("was not archived"), "{why}"),
            other => panic!("expected an error, got {other:?}"),
        }
    }
}
