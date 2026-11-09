//! AUTO-4: the passed PRs are merged through `merge` in number order, and the first error stops
//! the run. A PASS on a head whose CI is pending is not ready; once CI is green it is.
//! Each merge test writes the stub `merge_pr.py` from gh_merge.rs into a temp pipeline root.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use switchboard_core::clock::FixedClock;
use switchboard_core::github::merge::{merge_passed, ready_to_merge};
use switchboard_core::github::poll::Poller;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::{PullRequest, ReviewVerdict};
use switchboard_core::rules::{Caller, Face, Role};
use switchboard_core::CoreError;

const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

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

fn setup(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("sb-auto4-{tag}-{}-{nanos}", std::process::id()));
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

fn pr(number: u64, verdicts: Vec<(&str, &str)>, ci: &str) -> PullRequest {
    PullRequest {
        number,
        state: "open".to_string(),
        head_sha: HEAD.to_string(),
        head_ref: format!("feat-{number}"),
        verdicts: verdicts
            .into_iter()
            .map(|(v, sha)| ReviewVerdict {
                verdict: v.to_string(),
                sha: sha.to_string(),
                needs_me: false,
            })
            .collect(),
        ci_on_head: ci.to_string(),
        ..Default::default()
    }
}

fn no_carry(_: &PullRequest) -> bool {
    false
}

fn pm() -> Caller {
    Caller {
        face: Face::Cli,
        role: Some(Role::ProjectManager),
        in_job_worktree: false,
        confirmed_in_app: false,
    }
}

#[test]
fn pass_on_head_is_ready_only_once_ci_is_green() {
    let pending = vec![pr(71, vec![("PASS", HEAD)], "pending")];
    assert!(ready_to_merge(&pending, &no_carry).is_empty());

    let green = vec![pr(71, vec![("PASS", HEAD)], "success")];
    assert_eq!(ready_to_merge(&green, &no_carry), vec![71]);
}

#[test]
fn older_pass_is_ready_only_when_carried_over_and_green() {
    let green = vec![pr(8, vec![("PASS", OLD)], "success")];
    assert!(ready_to_merge(&green, &no_carry).is_empty());
    assert_eq!(ready_to_merge(&green, &|_| true), vec![8]);

    let red = vec![pr(8, vec![("PASS", OLD)], "failure")];
    assert!(ready_to_merge(&red, &|_| true).is_empty());
}

#[test]
fn closed_fail_no_verdict_and_red_ci_are_not_ready() {
    let mut closed = pr(1, vec![("PASS", HEAD)], "success");
    closed.state = "merged".to_string();
    let prs = vec![
        closed,
        pr(2, vec![("FAIL", HEAD)], "success"),
        pr(3, vec![], "success"),
        pr(4, vec![("PASS", HEAD)], "failure"),
    ];
    assert!(ready_to_merge(&prs, &no_carry).is_empty());
}

#[test]
fn ready_prs_come_smallest_number_first() {
    let prs = vec![
        pr(9, vec![("PASS", HEAD)], "success"),
        pr(3, vec![("PASS", HEAD)], "success"),
        pr(5, vec![("PASS", HEAD)], "success"),
    ];
    assert_eq!(ready_to_merge(&prs, &no_carry), vec![3, 5, 9]);
}

#[test]
fn merge_passed_merges_in_order_and_returns_closed_issues() {
    let (root, dir, clone) = setup("order");
    let t = Fake::default();
    let store = SnapshotStore::new(&root.join("snapshot"));
    let clock = FixedClock::at(1_800_000_000);
    let poller = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec!["o/r".to_string()],
    };
    let mut five = pr(5, vec![("PASS", HEAD)], "success");
    five.closes = vec![10];
    let mut two = pr(2, vec![("PASS", HEAD)], "success");
    two.closes = vec![11, 12];
    let prs = vec![five, two];

    let (merged, closed) =
        merge_passed(&pm(), &clone, &root, "o/r", &prs, &no_carry, &poller).unwrap();
    assert_eq!(merged, vec![2, 5]);
    assert_eq!(closed, vec![11, 12, 10]);

    let numbers: Vec<String> = calls(&dir)
        .iter()
        .map(|c| c[c.iter().position(|a| a == "--pr").unwrap() + 1].clone())
        .collect();
    assert_eq!(numbers, vec!["2", "2", "5", "5"]);
}

#[test]
fn merge_passed_stops_at_the_first_error() {
    let (root, dir, clone) = setup("stop");
    std::fs::write(dir.join("exit-dry-run"), "1").unwrap();
    let t = Fake::default();
    let store = SnapshotStore::new(&root.join("snapshot"));
    let clock = FixedClock::at(1_800_000_000);
    let poller = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec!["o/r".to_string()],
    };
    let prs = vec![
        pr(2, vec![("PASS", HEAD)], "success"),
        pr(5, vec![("PASS", HEAD)], "success"),
    ];
    let result = merge_passed(&pm(), &clone, &root, "o/r", &prs, &no_carry, &poller);
    assert!(matches!(result, Err(CoreError::Refused(_))));
    assert_eq!(
        calls(&dir).len(),
        1,
        "only the first PR reaches merge_pr.py"
    );
}

#[test]
fn guard_refusal_stops_merge_passed_before_any_merge() {
    let (root, dir, clone) = setup("guard");
    let t = Fake::default();
    let store = SnapshotStore::new(&root.join("snapshot"));
    let clock = FixedClock::at(1_800_000_000);
    let poller = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec!["o/r".to_string()],
    };
    let prs = vec![pr(2, vec![("PASS", HEAD)], "success")];
    let caller = Caller {
        face: Face::Mcp,
        role: None,
        in_job_worktree: false,
        confirmed_in_app: false,
    };
    let result = merge_passed(&caller, &clone, &root, "o/r", &prs, &no_carry, &poller);
    assert!(matches!(result, Err(CoreError::Refused(_))));
    assert!(
        calls(&dir).is_empty(),
        "no merge_pr.py call on a guard refusal"
    );
}
