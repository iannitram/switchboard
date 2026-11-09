//! Tests for #100 (BAK-3: restore history, labels and milestones to a new private repo).
//!
//! The fixture GitHub is a stub `fleet.py` in a temp pipeline root: it logs its arguments,
//! copies any `ghq add-batch` file, answers `<cmd>-<subcommand>.json` and exits with the code in
//! `exit-<cmd>-<subcommand>` (0 when there is none). Its `repo create` answer names a local bare
//! repo as the push URL. Every git repo is made by local `git` in a temp dir; nothing touches
//! the network.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::backup::{
    bundle, restore_history, write_json, RecordedAnswers, RestoreReport,
};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::CoreError;

const NEW_REPO: &str = "example/restored";

const STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(args) + "\n")
name = "-".join(args[:2])
if name == "ghq-add-batch":
    with open(args[2]) as b, open(os.path.join(here, "batch.json"), "w") as o:
        o.write(b.read())
answer = os.path.join(here, name + ".json")
print(open(answer).read() if os.path.exists(answer) else "{}")
code = os.path.join(here, "exit-" + name)
sys.exit(int(open(code).read()) if os.path.exists(code) else 0)
"#;

/// A fresh, empty temp dir for one test.
fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-bak3-{tag}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs git in `dir` with a fixed identity and no signing; panics when git fails. Returns stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Switchboard Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The sorted `refname objectname` lines of `repo`.
fn refs_of(repo: &Path) -> Vec<String> {
    let mut lines: Vec<String> = git(repo, &["for-each-ref", "--format=%(refname) %(objectname)"])
        .lines()
        .map(str::to_string)
        .collect();
    lines.sort();
    lines
}

/// A mirror with branches `main` and `feature`, tag `v1`, and GitHub's own `refs/pull/1/head`.
fn mirror(root: &Path) -> PathBuf {
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    std::fs::write(work.join("README.md"), "one\n").unwrap();
    git(&work, &["add", "README.md"]);
    git(&work, &["commit", "-q", "-m", "first"]);
    git(&work, &["tag", "-a", "v1", "-m", "v1"]);
    git(&work, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(work.join("README.md"), "two\n").unwrap();
    git(&work, &["commit", "-q", "-am", "second"]);
    git(&work, &["checkout", "-q", "main"]);
    let mirror = root.join("mirror.git");
    git(
        root,
        &[
            "clone",
            "-q",
            "--mirror",
            work.to_str().unwrap(),
            mirror.to_str().unwrap(),
        ],
    );
    git(
        &mirror,
        &["update-ref", "refs/pull/1/head", "refs/heads/feature"],
    );
    mirror
}

fn array(text: &str) -> Vec<Value> {
    serde_json::from_str(text).unwrap()
}

/// A backup folder: the mirror's bundle plus the BAK-1 JSON of the fixture (4 labels, 2
/// milestones). Returns (backup dir, mirror).
fn backup(root: &Path) -> (PathBuf, PathBuf) {
    let mirror = mirror(root);
    let dir = root.join("backup");
    bundle(&mirror, &dir).unwrap();
    let answers = RecordedAnswers {
        repo: "example/fixture".to_string(),
        issues: array(include_str!("fixtures/backup-json/issues.json")),
        pulls: array(include_str!("fixtures/backup-json/pulls.json")),
        comments: array(include_str!("fixtures/backup-json/comments.json")),
        labels: array(include_str!("fixtures/backup-json/labels.json")),
        milestones: array(include_str!("fixtures/backup-json/milestones.json")),
    };
    write_json(&answers, &dir, "2026-11-09T07:00:00Z").unwrap();
    (dir, mirror)
}

/// A stub pipeline root whose `repo view` says the name is free and whose `repo create` names a
/// new local bare repo. Returns (bridge, stub dir, bare repo).
fn fixture_github(root: &Path, private: bool) -> (FleetBridge, PathBuf, PathBuf) {
    let pipeline = root.join("pipeline");
    let dir = pipeline.join("tools").join("fleet");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("fleet.py"), STUB).unwrap();
    let bare = root.join("github").join("restored.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare"]);
    std::fs::write(dir.join("repo-view.json"), r#"{"exists": false}"#).unwrap();
    std::fs::write(
        dir.join("repo-create.json"),
        json!({"url": bare.to_str().unwrap(), "private": private}).to_string(),
    )
    .unwrap();
    (FleetBridge::with_root(&pipeline), dir, bare)
}

/// Each logged call's arguments.
fn calls(dir: &Path) -> Vec<Vec<String>> {
    match std::fs::read_to_string(dir.join("calls.log")) {
        Ok(t) => t
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// The first two arguments of each call (`repo view`, `ghq add-batch`).
fn commands(dir: &Path) -> Vec<String> {
    calls(dir).iter().map(|c| c[..2].join(" ")).collect()
}

#[test]
fn a_restore_pushes_every_ref_and_queues_the_labels_and_milestones() {
    let root = scratch("full");
    let (backup_dir, mirror) = backup(&root);
    let (fleet, dir, bare) = fixture_github(&root, true);

    let report = restore_history(&fleet, &backup_dir, NEW_REPO).unwrap();

    assert_eq!(
        report,
        RestoreReport {
            refs: 3,
            labels: 4,
            milestones: 2
        }
    );
    let wanted: Vec<String> = refs_of(&mirror)
        .into_iter()
        .filter(|line| !line.starts_with("refs/pull/"))
        .collect();
    assert_eq!(refs_of(&bare), wanted);
    assert_eq!(
        commands(&dir),
        vec![
            "repo view",
            "repo create",
            "ghq add-batch",
            "ghq run",
            "ghq status"
        ]
    );
}

#[test]
fn the_new_repo_is_created_private() {
    let root = scratch("private");
    let (backup_dir, _) = backup(&root);
    let (fleet, dir, _) = fixture_github(&root, true);
    restore_history(&fleet, &backup_dir, NEW_REPO).unwrap();
    let create = &calls(&dir)[1];
    assert_eq!(&create[..3], ["repo", "create", NEW_REPO]);
    assert!(create.contains(&"--private".to_string()));
    assert!(create.contains(&"--coordinator".to_string()));
}

#[test]
fn labels_and_milestones_are_sent_through_the_queue_and_match_the_manifest() {
    let root = scratch("queue");
    let (backup_dir, _) = backup(&root);
    let (fleet, dir, _) = fixture_github(&root, true);
    restore_history(&fleet, &backup_dir, NEW_REPO).unwrap();

    let add = &calls(&dir)[2];
    assert!(add.contains(&"--coordinator".to_string()));
    let batch: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(dir.join("batch.json")).unwrap()).unwrap();
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(backup_dir.join("manifest.json")).unwrap())
            .unwrap();
    let of_kind = |kind: &str| batch.iter().filter(|c| c["kind"] == kind).count() as u64;
    assert_eq!(of_kind("label"), manifest["labels"].as_u64().unwrap());
    assert_eq!(
        of_kind("milestone"),
        manifest["milestones"].as_u64().unwrap()
    );
    assert_eq!(batch.len(), 6);
    assert!(batch.iter().all(|c| c["repo"] == NEW_REPO));
    assert!(batch.contains(&json!({"kind": "label", "repo": NEW_REPO, "name": "size:easy"})));
    assert!(batch.contains(&json!({"kind": "milestone", "repo": NEW_REPO, "title": "0.9"})));
}

#[test]
fn a_name_that_already_exists_is_refused_with_no_write() {
    let root = scratch("exists");
    let (backup_dir, _) = backup(&root);
    let (fleet, dir, bare) = fixture_github(&root, true);
    std::fs::write(dir.join("repo-view.json"), r#"{"exists": true}"#).unwrap();

    match restore_history(&fleet, &backup_dir, NEW_REPO) {
        Err(CoreError::Refused(why)) => assert!(why.contains(NEW_REPO), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(commands(&dir), vec!["repo view"]);
    assert!(refs_of(&bare).is_empty());
}

#[test]
fn a_view_answer_that_does_not_say_free_is_refused_with_no_write() {
    let root = scratch("unsure");
    let (backup_dir, _) = backup(&root);
    let (fleet, dir, bare) = fixture_github(&root, true);
    std::fs::write(dir.join("repo-view.json"), "{}").unwrap();

    assert!(matches!(
        restore_history(&fleet, &backup_dir, NEW_REPO),
        Err(CoreError::Refused(_))
    ));
    assert_eq!(commands(&dir), vec!["repo view"]);
    assert!(refs_of(&bare).is_empty());
}

#[test]
fn a_failed_view_is_a_fleet_error_with_no_write() {
    let root = scratch("viewfail");
    let (backup_dir, _) = backup(&root);
    let (fleet, dir, bare) = fixture_github(&root, true);
    std::fs::write(dir.join("exit-repo-view"), "2").unwrap();

    assert!(matches!(
        restore_history(&fleet, &backup_dir, NEW_REPO),
        Err(CoreError::Fleet(_))
    ));
    assert_eq!(commands(&dir), vec!["repo view"]);
    assert!(refs_of(&bare).is_empty());
}

#[test]
fn a_repo_not_created_private_gets_no_push_and_no_labels() {
    let root = scratch("public");
    let (backup_dir, _) = backup(&root);
    let (fleet, dir, bare) = fixture_github(&root, false);

    assert!(restore_history(&fleet, &backup_dir, NEW_REPO).is_err());
    assert_eq!(commands(&dir), vec!["repo view", "repo create"]);
    assert!(refs_of(&bare).is_empty());
}

#[test]
fn counts_that_differ_from_the_manifest_are_an_error_with_no_fleet_call() {
    let root = scratch("count");
    let (backup_dir, _) = backup(&root);
    let (fleet, dir, bare) = fixture_github(&root, true);
    std::fs::write(backup_dir.join("labels.json"), r#"[{"name": "bug"}]"#).unwrap();

    assert!(restore_history(&fleet, &backup_dir, NEW_REPO).is_err());
    assert!(calls(&dir).is_empty());
    assert!(refs_of(&bare).is_empty());
}

#[test]
fn a_backup_without_a_bundle_is_an_error_with_no_fleet_call() {
    let root = scratch("nobundle");
    let (backup_dir, _) = backup(&root);
    let (fleet, dir, _) = fixture_github(&root, true);
    std::fs::remove_file(backup_dir.join("repo.bundle")).unwrap();

    assert!(matches!(
        restore_history(&fleet, &backup_dir, NEW_REPO),
        Err(CoreError::Io(_))
    ));
    assert!(calls(&dir).is_empty());
}

#[test]
fn a_name_that_is_not_owner_slash_name_is_refused_with_no_fleet_call() {
    let root = scratch("badname");
    let (backup_dir, _) = backup(&root);
    let (fleet, dir, _) = fixture_github(&root, true);
    for name in [
        "restored",
        "a/b/c",
        "/restored",
        "example/",
        "../x",
        "o/r --public",
    ] {
        assert!(
            matches!(
                restore_history(&fleet, &backup_dir, name),
                Err(CoreError::Refused(_))
            ),
            "{name}"
        );
    }
    assert!(calls(&dir).is_empty());
}
