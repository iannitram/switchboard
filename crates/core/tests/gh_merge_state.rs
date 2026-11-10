//! #341 (GH-5): `merge` checks the PR's state. A closed or already merged PR is refused with
//! no `merge_pr.py` call, whatever its verdict and CI.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use switchboard_core::clock::FixedClock;
use switchboard_core::github::merge::{eligibility, merge, MergeEligibility, MergeJob};
use switchboard_core::github::poll::Poller;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::{PullRequest, ReviewVerdict};
use switchboard_core::rules::{Caller, Face, Role};
use switchboard_core::CoreError;

const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

const STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(sys.argv[1:]) + "\n")
"#;

struct NoGitHub;

impl GitHubTransport for NoGitHub {
    fn graphql(&self, _query: &str, _variables: &Value) -> Result<Value, CoreError> {
        Ok(json!({ "repository": {} }))
    }
    fn rest_get(&self, _path: &str, _etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        Err(CoreError::GitHub("no REST here".to_string()))
    }
}

fn setup(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("sb-merge-state-{tag}-{}-{nanos}", std::process::id()));
    let dir = root.join("tools").join("merge-pr");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("merge_pr.py"), STUB).unwrap();
    let clone = root.join("clone");
    std::fs::create_dir_all(&clone).unwrap();
    (root, dir, clone)
}

/// PR 11 in `state`, a PASS on its head and CI green.
fn pr(state: &str) -> PullRequest {
    PullRequest {
        number: 11,
        state: state.to_string(),
        head_sha: HEAD.to_string(),
        head_ref: "b11".to_string(),
        verdicts: vec![ReviewVerdict {
            verdict: "PASS".to_string(),
            sha: HEAD.to_string(),
            needs_me: false,
        }],
        ci_on_head: "success".to_string(),
        ..Default::default()
    }
}

fn run(root: &Path, clone: &Path, pr: &PullRequest) -> Result<(), CoreError> {
    let store = SnapshotStore::new(&root.join("data"));
    let clock = FixedClock::at(1_800_000_000);
    let poller = Poller {
        transport: &NoGitHub,
        store: &store,
        clock: &clock,
        repos: vec!["owner/alpha".to_string()],
    };
    let caller = Caller {
        face: Face::Cli,
        role: Some(Role::ProjectManager),
        in_job_worktree: false,
        confirmed_in_app: false,
    };
    let job = MergeJob {
        pipeline_root: root,
        clone,
        repo: "owner/alpha",
        pr,
        carried_over: false,
    };
    merge(&caller, &job, &poller)
}

#[test]
fn a_closed_or_merged_pr_is_not_eligible() {
    assert_eq!(
        eligibility(&pr("closed"), false),
        MergeEligibility::NotOpen("closed".to_string())
    );
    assert_eq!(
        eligibility(&pr("merged"), true),
        MergeEligibility::NotOpen("merged".to_string())
    );
    assert_eq!(eligibility(&pr("open"), false), MergeEligibility::Eligible);
}

#[test]
fn merging_a_closed_pr_is_refused_without_calling_merge_pr() {
    let (root, dir, clone) = setup("closed");
    match run(&root, &clone, &pr("closed")) {
        Err(CoreError::Refused(why)) => assert_eq!(why, "the PR is closed"),
        other => panic!("got {other:?}"),
    }
    assert!(!dir.join("calls.log").exists(), "merge_pr.py was called");
}

#[test]
fn merging_an_already_merged_pr_is_refused_without_calling_merge_pr() {
    let (root, dir, clone) = setup("merged");
    match run(&root, &clone, &pr("merged")) {
        Err(CoreError::Refused(why)) => assert_eq!(why, "the PR is already merged"),
        other => panic!("got {other:?}"),
    }
    assert!(!dir.join("calls.log").exists(), "merge_pr.py was called");
}
