//! AUTO-4: after each poll, every passed PR (PASS on its head, green CI on that head) merges
//! through merge_pr.py as the Project Manager, its issues close by the merge, and the repo is
//! read once more (GH-3). The stub `merge_pr.py` (as in auto_merge_passed.rs) logs its
//! arguments; a fake transport logs the re-reads.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use switchboard_core::autopilot::passes::{autopilot_caller, merge_pass, Merged};
use switchboard_core::clock::FixedClock;
use switchboard_core::github::poll::Poller;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::{PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::rules::{Face, Role};
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

impl Fake {
    /// The repos read, one entry per GraphQL read.
    fn reads(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter_map(|l| l.strip_prefix("graphql ").map(str::to_string))
            .collect()
    }
}

/// A pipeline root with the stub merge_pr.py, and a dev root holding a clone of `demo`.
struct World {
    pipeline: PathBuf,
    stub_dir: PathBuf,
    dev: PathBuf,
}

fn world(tag: &str) -> World {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("sb-auto4p-{tag}-{}-{nanos}", std::process::id()));
    let pipeline = base.join("pipeline");
    let stub_dir = pipeline.join("tools").join("merge-pr");
    std::fs::create_dir_all(&stub_dir).unwrap();
    std::fs::write(stub_dir.join("merge_pr.py"), STUB).unwrap();
    let dev = base.join("dev");
    std::fs::create_dir_all(dev.join("demo")).unwrap();
    World {
        pipeline,
        stub_dir,
        dev,
    }
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

fn pr(number: u64, verdict: Option<&str>, ci: &str, closes: &[u64]) -> PullRequest {
    PullRequest {
        number,
        state: "open".to_string(),
        head_sha: HEAD.to_string(),
        head_ref: format!("sat/{number}"),
        closes: closes.to_vec(),
        verdicts: verdict
            .map(|sha| {
                vec![ReviewVerdict {
                    verdict: "PASS".to_string(),
                    sha: sha.to_string(),
                    needs_me: false,
                }]
            })
            .unwrap_or_default(),
        ci_on_head: ci.to_string(),
        ..Default::default()
    }
}

fn snap(repo: &str, pulls: Vec<PullRequest>) -> RepoSnapshot {
    RepoSnapshot {
        repo: repo.to_string(),
        pulls,
        ..Default::default()
    }
}

/// Run `merge_pass` over `snaps` with a fresh fake transport; answer the result and the reads.
fn run(w: &World, snaps: &[RepoSnapshot]) -> (Result<Vec<Merged>, CoreError>, Vec<String>) {
    let t = Fake::default();
    let store = SnapshotStore::new(&w.pipeline.join("data"));
    let clock = FixedClock::at(1_800_000_000);
    let poller = Poller {
        transport: &t,
        store: &store,
        clock: &clock,
        repos: vec![],
    };
    let result = merge_pass(snaps, &poller, &w.pipeline, &w.dev);
    (result, t.reads())
}

#[test]
fn the_autopilot_merges_as_the_app_in_the_project_manager_role() {
    let caller = autopilot_caller();
    assert_eq!(caller.face, Face::Gui);
    assert_eq!(caller.role, Some(Role::ProjectManager));
    assert!(!caller.in_job_worktree);
    assert!(!caller.confirmed_in_app);
}

#[test]
fn a_pass_on_head_with_green_ci_merges_closes_its_issue_and_reads_the_repo_once() {
    let w = world("merge");
    let snaps = vec![snap(
        "iannitram/demo",
        vec![pr(7, Some(HEAD), "success", &[12])],
    )];
    let (result, reads) = run(&w, &snaps);
    assert_eq!(
        result,
        Ok(vec![("iannitram/demo".to_string(), vec![7], vec![12])])
    );
    let clone = w.dev.join("demo").to_string_lossy().into_owned();
    let call = |flag: &str| -> Vec<String> {
        [
            "merge",
            clone.as_str(),
            "main",
            "origin/sat/7",
            "--pr",
            "7",
            "--repo",
            "iannitram/demo",
            flag,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    assert_eq!(calls(&w.stub_dir), vec![call("--dry-run"), call("--push")]);
    assert_eq!(reads, vec!["iannitram/demo".to_string()]);
}

#[test]
fn a_pass_on_an_older_head_red_ci_or_no_pass_is_left_open() {
    let w = world("left");
    let snaps = vec![snap(
        "iannitram/demo",
        vec![
            pr(3, Some(OLD), "success", &[30]),
            pr(4, Some(HEAD), "failure", &[40]),
            pr(5, Some(HEAD), "pending", &[50]),
            pr(6, None, "success", &[60]),
        ],
    )];
    let (result, reads) = run(&w, &snaps);
    assert_eq!(result, Ok(vec![]));
    assert!(calls(&w.stub_dir).is_empty());
    assert!(reads.is_empty());
}

#[test]
fn only_the_passed_pr_merges_beside_ones_that_are_not_ready() {
    let w = world("mixed");
    let snaps = vec![snap(
        "iannitram/demo",
        vec![
            pr(9, Some(HEAD), "success", &[90, 91]),
            pr(4, Some(OLD), "success", &[40]),
        ],
    )];
    let (result, reads) = run(&w, &snaps);
    assert_eq!(
        result,
        Ok(vec![("iannitram/demo".to_string(), vec![9], vec![90, 91])])
    );
    assert_eq!(calls(&w.stub_dir).len(), 2);
    assert_eq!(reads.len(), 1);
}

#[test]
fn an_archived_repo_or_one_without_a_clone_is_skipped() {
    let w = world("skip");
    let mut archived = snap("iannitram/demo", vec![pr(7, Some(HEAD), "success", &[12])]);
    archived.archived = true;
    let no_clone = snap(
        "iannitram/elsewhere",
        vec![pr(8, Some(HEAD), "success", &[13])],
    );
    let (result, reads) = run(&w, &[archived, no_clone]);
    assert_eq!(result, Ok(vec![]));
    assert!(calls(&w.stub_dir).is_empty());
    assert!(reads.is_empty());
}

#[test]
fn a_refused_merge_stops_the_pass_with_merge_prs_reason() {
    let w = world("refused");
    std::fs::write(w.stub_dir.join("exit-dry-run"), "1").unwrap();
    let snaps = vec![snap(
        "iannitram/demo",
        vec![
            pr(7, Some(HEAD), "success", &[12]),
            pr(8, Some(HEAD), "success", &[13]),
        ],
    )];
    let (result, reads) = run(&w, &snaps);
    assert_eq!(
        result,
        Err(CoreError::Refused("branch is behind main".to_string()))
    );
    assert_eq!(calls(&w.stub_dir).len(), 1, "only PR 7's dry run");
    assert!(reads.is_empty());
}

#[test]
fn the_code_map_names_the_merge_pass_present() {
    let map = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md"),
    )
    .unwrap();
    assert!(map.contains(
        "- AUTO-4 merge pass (`merge_pass` in `passes.rs`; the app's poll thread runs it after `after_poll`): present."
    ));
}
