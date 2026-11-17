//! unit-land (#406): the pull request carries each verify command and its result; no pull request
//! is opened when the code map check fails on the branch head; resolve leaves one record, closes
//! the issue and releases the claim file.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use serde_json::{json, Value};
use switchboard_core::clock::FixedClock;
use switchboard_core::error::CoreError;
use switchboard_core::sky::codemap::Touched;
use switchboard_core::unit::claim::{self, Claim, Mirror, Outcome, Store};
use switchboard_core::unit::job::{self, NodeSpec};
use switchboard_core::unit::land::{self, Forge, Git, Pr, RECORD_MARK};

const BRANCH: &str = "sat/406-unit-land";
const BASE: &str = "main";
const REPO: &str = "example/switchboard";
const CLAIM_FILE: &str = "claims/example/switchboard/406.json";

const MAP_BASE: &str = "# Code map\n\nUpdated: 2026-11-01\n\nmodules:\n- unit-land: crates/core/src/unit/land.rs (planned)\n";
const MAP_PRESENT: &str = "# Code map\n\nUpdated: 2026-11-16\n\nmodules:\n- unit-land: crates/core/src/unit/land.rs (present)\n";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn node() -> NodeSpec {
    job::read_node(&repo_root(), "unit-land").expect("the node file parses")
}

fn card(checks: &[(&str, &str, &str)]) -> Value {
    let checks: Vec<Value> = checks
        .iter()
        .map(|(name, status, note)| json!({"name": name, "status": status, "note": note}))
        .collect();
    json!({ "id": "unit-406-unit-land", "checks": checks })
}

fn passing_card() -> Value {
    card(&[
        ("tests", "passed", "1 test failed before the change"),
        ("lint", "passed", ""),
    ])
}

fn touched(added_land: bool, map_updated: bool) -> Vec<Touched> {
    let mut files = vec![Touched {
        path: "crates/core/src/unit/land.rs".into(),
        added: added_land,
    }];
    if map_updated {
        files.push(Touched {
            path: "docs/code-map.md".into(),
            added: false,
        });
    }
    files
}

#[derive(Default)]
struct FakeGit {
    touched: Vec<Touched>,
    head_map: String,
    pushes: Mutex<Vec<String>>,
}

impl Git for FakeGit {
    fn touched(&self, _: &str, _: &str) -> Result<Vec<Touched>, CoreError> {
        Ok(self.touched.clone())
    }
    fn show(&self, rev: &str, path: &str) -> Result<String, CoreError> {
        assert_eq!(path, "docs/code-map.md");
        Ok(if rev == BASE {
            MAP_BASE.to_string()
        } else {
            self.head_map.clone()
        })
    }
    fn push(&self, branch: &str) -> Result<(), CoreError> {
        self.pushes.lock().unwrap().push(branch.to_string());
        Ok(())
    }
}

#[derive(Default)]
struct FakeForge {
    comments: Mutex<Vec<String>>,
    closed: Mutex<Vec<u64>>,
    prs: Mutex<Vec<(String, String, String)>>,
}

impl Forge for FakeForge {
    fn open_pr(
        &self,
        _: &str,
        head: &str,
        base: &str,
        _: &str,
        body: &str,
    ) -> Result<u64, CoreError> {
        self.prs
            .lock()
            .unwrap()
            .push((head.into(), base.into(), body.into()));
        Ok(7)
    }
    fn comments(&self, _: &str, _: u64) -> Result<Vec<String>, CoreError> {
        Ok(self.comments.lock().unwrap().clone())
    }
    fn comment(&self, _: &str, _: u64, body: &str) -> Result<(), CoreError> {
        self.comments.lock().unwrap().push(body.to_string());
        Ok(())
    }
    fn close(&self, _: &str, issue: u64) -> Result<(), CoreError> {
        self.closed.lock().unwrap().push(issue);
        Ok(())
    }
}

#[derive(Default)]
struct FakeMirror {
    calls: Mutex<Vec<String>>,
}

impl Mirror for FakeMirror {
    fn open_blockers(&self, _: &str, _: u64) -> Result<Vec<u64>, CoreError> {
        Ok(Vec::new())
    }
    fn assign(&self, _: &str, _: u64, _: &str) -> Result<(), CoreError> {
        Ok(())
    }
    fn unassign(&self, _: &str, i: u64, w: &str) -> Result<(), CoreError> {
        self.calls.lock().unwrap().push(format!("unassign {i} {w}"));
        Ok(())
    }
    fn label(&self, _: &str, _: u64, _: &str) -> Result<(), CoreError> {
        Ok(())
    }
    fn unlabel(&self, _: &str, i: u64, l: &str) -> Result<(), CoreError> {
        self.calls.lock().unwrap().push(format!("unlabel {i} {l}"));
        Ok(())
    }
}

fn pr<'a>(node: &'a NodeSpec, card: &'a Value) -> Pr<'a> {
    Pr {
        repo: REPO,
        issue: 406,
        node,
        base: BASE,
        head: BRANCH,
        done_card: card,
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A bare remote whose fleet-state branch holds one file, and a store over it.
fn store(name: &str) -> Store {
    let root = std::env::temp_dir().join(format!("sb-land-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let seed = root.join("seed");
    let remote = root.join("remote.git");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "--quiet", "-b", "fleet-state"]);
    std::fs::write(seed.join("README"), "state\n").unwrap();
    git(&seed, &["add", "README"]);
    git(&seed, &["commit", "--quiet", "-m", "seed"]);
    git(
        &root,
        &["init", "--bare", "--quiet", remote.to_str().unwrap()],
    );
    git(
        &seed,
        &["push", "--quiet", remote.to_str().unwrap(), "fleet-state"],
    );
    Store {
        remote: remote.to_str().unwrap().to_string(),
        scratch: root.join("scratch"),
    }
}

fn files(store: &Store) -> String {
    git(
        Path::new(&store.remote),
        &["ls-tree", "-r", "--name-only", "fleet-state"],
    )
}

/// Acquire the claim on #406 for `worker`; the claim the caller holds.
fn claim_406(store: &Store, mirror: &FakeMirror, worker: &str) -> Claim {
    match claim::claim(
        store,
        mirror,
        &FixedClock::at(1_000),
        REPO,
        406,
        worker,
        "base-sha",
    )
    .expect("claims")
    {
        Outcome::Claimed(claim) => claim,
        other => panic!("expected a claim, got {other:?}"),
    }
}

#[test]
fn the_node_file_names_the_verify_commands_of_the_unit() {
    let n = node();
    assert_eq!(n.id, "unit-land");
    assert_eq!(n.verify.len(), 2);
    assert!(n
        .files_allowed
        .contains(&"crates/core/src/unit/land.rs".to_string()));
}

#[test]
fn the_pr_body_carries_each_verify_command_and_its_result() {
    let body = land::pr_body(406, &node(), &passing_card()).expect("the body builds");
    assert!(body.starts_with("Closes #406"), "{body}");
    assert!(
        body.contains("`cargo test -p switchboard-core --test unit_land` (tests): passed (1 test failed before the change)"),
        "{body}"
    );
    assert!(
        body.contains("`cargo clippy --all-targets -- -D warnings` (lint): passed"),
        "{body}"
    );
}

#[test]
fn a_verify_with_no_result_on_the_done_card_refuses_the_body() {
    let partial = card(&[("tests", "passed", "")]);
    let err = land::pr_body(406, &node(), &partial).unwrap_err();
    assert!(matches!(err, CoreError::Refused(_)), "{err:?}");
}

#[test]
fn open_pr_pushes_and_opens_the_pr_when_the_code_map_check_passes() {
    let node = node();
    let card = passing_card();
    let git = FakeGit {
        touched: touched(true, true),
        head_map: MAP_PRESENT.into(),
        ..Default::default()
    };
    let forge = FakeForge::default();
    let number = land::open_pr(&pr(&node, &card), &git, &forge).expect("the PR opens");
    assert_eq!(number, 7);
    assert_eq!(*git.pushes.lock().unwrap(), [BRANCH]);
    let prs = forge.prs.lock().unwrap();
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].0, BRANCH);
    assert_eq!(prs[0].1, BASE);
    assert!(prs[0]
        .2
        .contains("`cargo test -p switchboard-core --test unit_land` (tests)"));
}

#[test]
fn no_pr_is_opened_when_the_code_map_check_fails_on_the_head() {
    let node = node();
    let card = passing_card();
    // The head still says `planned`: the check fails.
    let git = FakeGit {
        touched: touched(true, true),
        head_map: MAP_BASE.into(),
        ..Default::default()
    };
    let forge = FakeForge::default();
    let err = land::open_pr(&pr(&node, &card), &git, &forge).unwrap_err();
    assert!(matches!(err, CoreError::Refused(_)), "{err:?}");
    assert!(git.pushes.lock().unwrap().is_empty());
    assert!(forge.prs.lock().unwrap().is_empty());
}

#[test]
fn no_pr_is_opened_when_the_code_map_is_not_updated_by_the_change() {
    let node = node();
    let card = passing_card();
    let git = FakeGit {
        touched: touched(true, false),
        head_map: MAP_PRESENT.into(),
        ..Default::default()
    };
    let forge = FakeForge::default();
    let err = land::open_pr(&pr(&node, &card), &git, &forge).unwrap_err();
    assert!(matches!(err, CoreError::Refused(_)), "{err:?}");
    assert!(git.pushes.lock().unwrap().is_empty());
    assert!(forge.prs.lock().unwrap().is_empty());
}

#[test]
fn resolve_writes_one_record_closes_the_issue_and_releases_the_claim_file() {
    let store = store("resolve");
    let mirror = FakeMirror::default();
    let claim = claim_406(&store, &mirror, "builder");
    assert!(files(&store).contains(CLAIM_FILE));

    let forge = FakeForge::default();
    let done = land::resolve(
        REPO,
        "unit-land",
        406,
        7,
        &claim.token,
        &store,
        &mirror,
        &forge,
    )
    .expect("resolves");
    assert_eq!(
        done,
        land::Resolved {
            record_written: true,
            claim_released: true
        }
    );
    let comments = forge.comments.lock().unwrap();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].starts_with(RECORD_MARK), "{}", comments[0]);
    let block = comments[0]
        .split("```json\n")
        .nth(1)
        .and_then(|b| b.split("\n```").next());
    let record: Value = serde_json::from_str(block.expect("one JSON block")).expect("JSON");
    assert_eq!(record["node_id"], "unit-land");
    assert_eq!(record["pr"], 7);
    assert!(record["answer"].as_str().is_some_and(|a| !a.is_empty()));
    assert_eq!(*forge.closed.lock().unwrap(), [406]);
    assert!(!files(&store).contains(CLAIM_FILE), "{}", files(&store));
    assert_eq!(
        *mirror.calls.lock().unwrap(),
        ["unassign 406 builder", "unlabel 406 in-progress"]
    );
}

#[test]
fn resolving_twice_leaves_one_record_and_no_claim_to_release() {
    let store = store("twice");
    let mirror = FakeMirror::default();
    let claim = claim_406(&store, &mirror, "builder");
    let forge = FakeForge::default();
    land::resolve(
        REPO,
        "unit-land",
        406,
        7,
        &claim.token,
        &store,
        &mirror,
        &forge,
    )
    .expect("first");
    let again = land::resolve(
        REPO,
        "unit-land",
        406,
        7,
        &claim.token,
        &store,
        &mirror,
        &forge,
    )
    .expect("second");
    assert_eq!(
        again,
        land::Resolved {
            record_written: false,
            claim_released: false
        }
    );
    let records = forge
        .comments
        .lock()
        .unwrap()
        .iter()
        .filter(|body| body.starts_with(RECORD_MARK))
        .count();
    assert_eq!(records, 1);
}

#[test]
fn resolve_with_a_stale_token_leaves_the_new_holders_claim_alone() {
    let store = store("stale");
    let mirror = FakeMirror::default();
    // The first holder's claim is released, and another worker claims the unit.
    let stale = claim_406(&store, &mirror, "builder");
    assert!(claim::release(&store, &mirror, REPO, 406, &stale.token).expect("releases"));
    let replacement = claim_406(&store, &mirror, "other");
    assert_ne!(stale.token, replacement.token);

    // A fresh mirror, so any call resolve makes on the replaced claim shows up alone.
    let mirror = FakeMirror::default();
    let forge = FakeForge::default();
    let done = land::resolve(
        REPO,
        "unit-land",
        406,
        7,
        &stale.token,
        &store,
        &mirror,
        &forge,
    )
    .expect("resolves");
    assert_eq!(
        done,
        land::Resolved {
            record_written: true,
            claim_released: false
        }
    );
    // The replacement's claim file is still there, and its mirror was not touched.
    assert!(files(&store).contains(CLAIM_FILE), "{}", files(&store));
    assert!(mirror.calls.lock().unwrap().is_empty());
    // The record and the close still happen: the unit is resolved by its pull request.
    assert_eq!(*forge.closed.lock().unwrap(), [406]);
}
