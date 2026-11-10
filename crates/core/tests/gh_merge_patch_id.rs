//! GH-5, R25 (#381): a PASS on an older sha carries over to the head when the PR's own diff
//! from main has the same patch-id at both (an update from main), and not when a code commit
//! came after it. Checked on a temp git repo; the merge pass then merges the carried-over PR
//! through a stub `merge_pr.py`. No GitHub, no real merge.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::autopilot::passes::merge_pass;
use switchboard_core::clock::FixedClock;
use switchboard_core::github::merge::carried_over_by_patch_id;
use switchboard_core::github::poll::Poller;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::{PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::CoreError;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-patchid-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Path, file: &str, text: &str, msg: &str) -> String {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"])
}

/// A repo: main with one commit; branch `pr` adds `feature.txt` (the reviewed sha); main then
/// moves on in another file. Answers (repo, reviewed sha).
fn repo(name: &str) -> (PathBuf, String) {
    let dir = temp_dir(name);
    git(&dir, &["init", "-q"]);
    git(&dir, &["checkout", "-q", "-b", "main"]);
    commit(&dir, "base.txt", "base\n", "base");
    git(&dir, &["checkout", "-q", "-b", "pr"]);
    let reviewed = commit(&dir, "feature.txt", "feature\n", "feature");
    git(&dir, &["checkout", "-q", "main"]);
    commit(&dir, "other.txt", "other\n", "main moves on");
    git(&dir, &["checkout", "-q", "pr"]);
    (dir, reviewed)
}

fn pull(pass: &str, head: &str) -> PullRequest {
    PullRequest {
        number: 21,
        state: "open".to_string(),
        head_sha: head.to_string(),
        head_ref: "pr".to_string(),
        verdicts: vec![ReviewVerdict {
            verdict: "PASS".to_string(),
            sha: pass.to_string(),
            needs_me: false,
        }],
        ci_on_head: "success".to_string(),
        ..Default::default()
    }
}

#[test]
fn an_update_from_main_carries_the_pass() {
    let (dir, reviewed) = repo("carried");
    git(&dir, &["merge", "-q", "--no-edit", "main"]);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    assert_ne!(head, reviewed);
    assert!(carried_over_by_patch_id(&dir, &pull(&reviewed, &head)));
}

#[test]
fn a_code_commit_after_the_pass_does_not() {
    let (dir, reviewed) = repo("code");
    git(&dir, &["merge", "-q", "--no-edit", "main"]);
    let head = commit(&dir, "feature.txt", "feature, changed\n", "more code");
    assert!(!carried_over_by_patch_id(&dir, &pull(&reviewed, &head)));
}

#[test]
fn a_whitespace_only_change_does_not() {
    let (dir, reviewed) = repo("space");
    let head = commit(&dir, "feature.txt", "feature \n", "indent");
    assert!(!carried_over_by_patch_id(&dir, &pull(&reviewed, &head)));
}

#[test]
fn no_pass_a_pass_on_head_an_unknown_sha_or_no_repo_is_no() {
    let (dir, reviewed) = repo("edges");
    let head = git(&dir, &["rev-parse", "HEAD"]);
    // The PASS is on the head already: nothing to carry.
    assert!(!carried_over_by_patch_id(&dir, &pull(&head, &head)));
    // A FAIL last.
    let mut failed = pull(&reviewed, &head);
    failed.verdicts[0].verdict = "FAIL".to_string();
    assert!(!carried_over_by_patch_id(&dir, &failed));
    // A sha the clone does not have.
    let unknown = "c".repeat(40);
    assert!(!carried_over_by_patch_id(&dir, &pull(&unknown, &head)));
    // A folder that is not a git repo.
    let plain = temp_dir("plain");
    assert!(!carried_over_by_patch_id(&plain, &pull(&reviewed, &head)));
}

// --- review of #381: the cases merge_pr.py's `carried_pass` decides, one by one ---

#[test]
fn a_fail_after_the_pass_wins() {
    let (dir, reviewed) = repo("fail-after");
    git(&dir, &["merge", "-q", "--no-edit", "main"]);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    let mut pr = pull(&reviewed, &head);
    pr.verdicts.push(ReviewVerdict {
        verdict: "FAIL".to_string(),
        sha: head.clone(),
        needs_me: true,
    });
    assert!(!carried_over_by_patch_id(&dir, &pr));
    // A FAIL on the same reviewed sha after the PASS: still the FAIL.
    pr.verdicts[1].sha = reviewed.clone();
    assert!(!carried_over_by_patch_id(&dir, &pr));
}

#[test]
fn a_rebase_onto_main_with_the_same_change_carries_the_pass() {
    // merge_pr.py compares each tip with its own merge-base, so a rebase that leaves the
    // PR's own diff byte-identical keeps the PASS.
    let (dir, reviewed) = repo("rebase");
    git(&dir, &["rebase", "-q", "main"]);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    assert_ne!(head, reviewed);
    assert!(carried_over_by_patch_id(&dir, &pull(&reviewed, &head)));
}

#[test]
fn a_rebase_that_changes_the_change_does_not() {
    let (dir, reviewed) = repo("rebase-edit");
    git(&dir, &["rebase", "-q", "main"]);
    git(&dir, &["reset", "-q", "--soft", "HEAD~1"]);
    std::fs::write(dir.join("feature.txt"), "feature, edited in the rebase\n").unwrap();
    git(&dir, &["add", "feature.txt"]);
    git(&dir, &["commit", "-q", "-m", "feature"]);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    assert!(!carried_over_by_patch_id(&dir, &pull(&reviewed, &head)));
}

#[test]
fn a_new_file_after_the_pass_does_not() {
    let (dir, reviewed) = repo("new-file");
    let head = commit(&dir, "extra.txt", "more\n", "one more file");
    assert!(!carried_over_by_patch_id(&dir, &pull(&reviewed, &head)));
}

#[test]
fn a_merge_from_main_that_also_changes_the_pr_does_not() {
    // The merge commit itself edits a file: not only an update from main.
    let (dir, reviewed) = repo("evil-merge");
    git(&dir, &["merge", "-q", "--no-commit", "--no-ff", "main"]);
    std::fs::write(dir.join("feature.txt"), "feature, slipped into the merge\n").unwrap();
    git(&dir, &["add", "feature.txt"]);
    git(&dir, &["commit", "-q", "--no-edit"]);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    assert!(!carried_over_by_patch_id(&dir, &pull(&reviewed, &head)));
}

#[test]
fn a_head_whose_own_change_is_empty_does_not() {
    // merge_pr.py: an empty change has no patch-id, so nothing carries.
    let (dir, reviewed) = repo("emptied");
    git(&dir, &["rm", "-q", "feature.txt"]);
    git(&dir, &["commit", "-q", "-m", "drop it"]);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    assert!(!carried_over_by_patch_id(&dir, &pull(&reviewed, &head)));
    // And a head sha the clone does not have.
    assert!(!carried_over_by_patch_id(
        &dir,
        &pull(&reviewed, &"d".repeat(40))
    ));
}

const STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(sys.argv[1:]) + "\n")
"#;

struct Quiet;

impl GitHubTransport for Quiet {
    fn graphql(&self, _query: &str, _variables: &Value) -> Result<Value, CoreError> {
        Ok(json!({ "repository": {} }))
    }
    fn rest_get(&self, _path: &str, _etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        Err(CoreError::GitHub("no REST here".to_string()))
    }
}

#[test]
fn the_merge_pass_merges_a_carried_over_pr() {
    let base = temp_dir("pass");
    let pipeline = base.join("pipeline");
    let stub_dir = pipeline.join("tools").join("merge-pr");
    std::fs::create_dir_all(&stub_dir).unwrap();
    std::fs::write(stub_dir.join("merge_pr.py"), STUB).unwrap();
    let dev = base.join("dev");
    std::fs::create_dir_all(&dev).unwrap();
    let (src, reviewed) = repo("pass-src");
    git(&src, &["merge", "-q", "--no-edit", "main"]);
    let head = git(&src, &["rev-parse", "HEAD"]);
    std::fs::rename(&src, dev.join("demo")).unwrap();

    let store = SnapshotStore::new(&base.join("data"));
    let clock = FixedClock::at(1_800_000_000);
    let poller = Poller {
        transport: &Quiet,
        store: &store,
        clock: &clock,
        repos: vec![],
    };
    let snaps = vec![RepoSnapshot {
        repo: "owner/demo".to_string(),
        pulls: vec![pull(&reviewed, &head)],
        ..Default::default()
    }];
    let merged = merge_pass(&snaps, &poller, &pipeline, &dev).merged;
    assert_eq!(merged, vec![("owner/demo".to_string(), vec![21], vec![])]);
    let log = std::fs::read_to_string(stub_dir.join("calls.log")).unwrap();
    assert_eq!(log.lines().count(), 2, "a dry run and a push: {log}");
}
