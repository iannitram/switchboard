//! Locked tests for #22 (GH-5: merge through merge_pr.py). Qwen never edits this file (R28).
//! Each test writes a stub `merge_pr.py` into a temp pipeline root. The stub logs its
//! arguments and exits with the code in `exit-dry-run` or `exit-push` (0 when there is none),
//! printing `REFUSED: branch is behind main` when that code is not 0.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use switchboard_core::clock::FixedClock;
use switchboard_core::github::merge::{merge, MergeJob};
use switchboard_core::github::poll::Poller;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::{PullRequest, ReviewVerdict};
use switchboard_core::rules::{Caller, Face, Role};
use switchboard_core::CoreError;

const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

const STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(args) + "\n")
code = os.path.join(here, "exit-" + args[-1].lstrip("-"))
code = int(open(code).read()) if os.path.exists(code) else 0
if code:
    print("REFUSED: branch is behind main")
sys.exit(code)
"#;

/// GraphQL logs `graphql <owner>/<name>` and answers an empty repository; REST is never used.
#[derive(Default)]
struct Fake {
    log: Mutex<Vec<String>>,
}

impl GitHubTransport for Fake {
    fn graphql(&self, _query: &str, variables: &Value) -> Result<Value, CoreError> {
        self.log.lock().unwrap().push(format!(
            "graphql {}/{}",
            variables["owner"].as_str().unwrap_or("?"),
            variables["name"].as_str().unwrap_or("?")
        ));
        Ok(json!({ "repository": {} }))
    }

    fn rest_get(&self, path: &str, _etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        self.log.lock().unwrap().push(format!("GET {path}"));
        Err(CoreError::GitHub("no REST here".to_string()))
    }
}

/// A temp pipeline root with the stub at `tools/merge-pr/merge_pr.py`, and a clone folder.
/// Returns (root, stub dir, clone).
fn setup(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("sb-merge-{tag}-{}-{nanos}", std::process::id()));
    let dir = root.join("tools").join("merge-pr");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("merge_pr.py"), STUB).unwrap();
    let clone = root.join("clone");
    std::fs::create_dir_all(&clone).unwrap();
    (root, dir, clone)
}

fn calls(dir: &Path) -> Vec<Vec<String>> {
    match std::fs::read_to_string(dir.join("calls.log")) {
        Ok(t) => t
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// PR 71 on branch `feat-x`, a PASS on its head and CI green.
fn passing_pr() -> PullRequest {
    PullRequest {
        number: 71,
        head_sha: HEAD.to_string(),
        head_ref: "feat-x".to_string(),
        verdicts: vec![ReviewVerdict {
            verdict: "PASS".to_string(),
            sha: HEAD.to_string(),
            needs_me: false,
        }],
        ci_on_head: "success".to_string(),
        ..Default::default()
    }
}

fn pm() -> Caller {
    Caller {
        face: Face::Cli,
        role: Some(Role::ProjectManager),
        in_job_worktree: false,
        confirmed_in_app: false,
    }
}

/// Run `merge` for `pr` on `o/r` as `caller`. Returns the result and the transport's log.
fn run(
    root: &Path,
    clone: &Path,
    caller: &Caller,
    pr: &PullRequest,
) -> (Result<(), CoreError>, Vec<String>) {
    let t = Fake::default();
    let store = SnapshotStore::new(&root.join("snapshot"));
    let clock = FixedClock::at(1_800_000_000);
    let poller = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec!["o/r".to_string()],
    };
    let job = MergeJob {
        pipeline_root: root,
        clone,
        repo: "o/r",
        pr,
        carried_over: false,
    };
    let result = merge(caller, &job, &poller);
    let log = t.log.lock().unwrap().clone();
    (result, log)
}

fn args(clone: &Path, flag: &str) -> Vec<String> {
    [
        "merge",
        &clone.display().to_string(),
        "main",
        "origin/feat-x",
        "--pr",
        "71",
        "--repo",
        "o/r",
        flag,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[test]
fn an_eligible_pr_runs_a_dry_run_then_a_push_then_rereads_the_repo() {
    let (root, dir, clone) = setup("ok");
    let (result, log) = run(&root, &clone, &pm(), &passing_pr());
    result.unwrap();
    assert_eq!(
        calls(&dir),
        vec![args(&clone, "--dry-run"), args(&clone, "--push")]
    );
    assert_eq!(log, vec!["graphql o/r"]);
}

#[test]
fn a_caller_the_guard_refuses_runs_nothing() {
    let (root, dir, clone) = setup("guard");
    let no_role = Caller {
        face: Face::Mcp,
        role: None,
        in_job_worktree: false,
        confirmed_in_app: false,
    };
    let in_worktree = Caller {
        in_job_worktree: true,
        ..pm()
    };
    for caller in [no_role, in_worktree] {
        let (result, log) = run(&root, &clone, &caller, &passing_pr());
        assert!(matches!(result, Err(CoreError::Refused(_))));
        assert!(log.is_empty());
    }
    assert!(calls(&dir).is_empty());
}

#[test]
fn a_pr_that_is_not_eligible_is_refused_and_runs_nothing() {
    let (root, dir, clone) = setup("inelig");
    let no_pass = PullRequest {
        verdicts: vec![],
        ..passing_pr()
    };
    let old_pass = PullRequest {
        head_sha: "c".repeat(40),
        ..passing_pr()
    };
    let ci_pending = PullRequest {
        ci_on_head: "pending".to_string(),
        ..passing_pr()
    };
    for pr in [no_pass, old_pass, ci_pending] {
        let (result, log) = run(&root, &clone, &pm(), &pr);
        assert!(matches!(result, Err(CoreError::Refused(_))));
        assert!(log.is_empty());
    }
    assert!(calls(&dir).is_empty());
}

#[test]
fn a_refused_dry_run_stops_before_the_push_and_says_why() {
    let (root, dir, clone) = setup("dry");
    std::fs::write(dir.join("exit-dry-run"), "1").unwrap();
    let (result, log) = run(&root, &clone, &pm(), &passing_pr());
    match result {
        Err(CoreError::Refused(why)) => assert!(why.contains("branch is behind main"), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(calls(&dir), vec![args(&clone, "--dry-run")]);
    assert!(log.is_empty());
}

#[test]
fn a_refused_push_is_an_error_and_rereads_nothing() {
    let (root, dir, clone) = setup("push");
    std::fs::write(dir.join("exit-push"), "1").unwrap();
    let (result, log) = run(&root, &clone, &pm(), &passing_pr());
    assert!(matches!(result, Err(CoreError::Refused(_))));
    assert_eq!(calls(&dir).len(), 2);
    assert!(log.is_empty());
}

#[test]
fn a_pass_carried_over_by_patch_id_merges() {
    let (root, dir, clone) = setup("carried");
    let pr = PullRequest {
        head_sha: "c".repeat(40),
        ..passing_pr()
    };
    let t = Fake::default();
    let store = SnapshotStore::new(&root.join("snapshot"));
    let clock = FixedClock::at(1_800_000_000);
    let poller = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec!["o/r".to_string()],
    };
    let job = MergeJob {
        pipeline_root: &root,
        clone: &clone,
        repo: "o/r",
        pr: &pr,
        carried_over: true,
    };
    merge(&pm(), &job, &poller).unwrap();
    assert_eq!(calls(&dir).len(), 2);
}
