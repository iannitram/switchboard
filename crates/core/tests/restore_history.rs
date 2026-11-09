//! BAK-3: `restore_history` puts a backup's history, labels and milestones into a new private
//! repo. Git pushes go to a local bare repo (standing in for `https://github.com`), the API is
//! recorded answers, and the create queue is a stub `fleet.py` that keeps its own counts.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::backup::{
    restore_history_from, write_json, RecordedAnswers, Remote, RestoreReport,
};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::transport::{Recorded, RestAnswer};
use switchboard_core::CoreError;

const NEW: &str = "owner/restored";

/// A fixture `fleet ghq`: logs each call, keeps every batch item in `items.json`, and answers
/// `status` with counts: a repo named in `taken` is `exists`, anything else is `done`.
const STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(args) + "\n")
items_file = os.path.join(here, "items.json")
items = json.load(open(items_file)) if os.path.exists(items_file) else []
sub = args[1] if len(args) > 1 else ""
if sub == "add-batch":
    items += json.load(open(args[2]))
    json.dump(items, open(items_file, "w"))
taken_file = os.path.join(here, "taken")
taken = open(taken_file).read().split() if os.path.exists(taken_file) else []
counts = {"pending": 0, "done": 0, "exists": 0, "failed": 0}
for item in items:
    taken_repo = item["kind"] == "repo" and item["repo"] in taken
    counts["exists" if taken_repo else "done"] += 1
print(json.dumps({"counts": counts, "total": len(items)}))
"#;

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
            "init.defaultBranch=master",
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
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The refs of `repo` as `<sha> <name>` lines, sorted.
fn refs_of(repo: &Path, pattern: &str) -> Vec<String> {
    let text = git(
        repo,
        &["for-each-ref", "--format=%(objectname) %(refname)", pattern],
    );
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    lines.sort();
    lines
}

struct Fixture {
    backup: PathBuf,
    source: PathBuf,
    server: PathBuf,
    target: PathBuf,
    stub: PathBuf,
    fleet: FleetBridge,
}

/// A backup folder (bundle of `master`, `feature`, `v1` and `refs/pull/1/head`; two labels; one
/// milestone; the manifest), an empty bare repo at `<server>/owner/restored.git` for the queue's
/// new repo, and the stub queue.
fn fixture(tag: &str) -> Fixture {
    let root = scratch(tag);
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    git(&source, &["init", "-q", "-b", "master"]);
    std::fs::write(source.join("a.txt"), "a\n").unwrap();
    git(&source, &["add", "a.txt"]);
    git(&source, &["commit", "-q", "-m", "first"]);
    git(&source, &["tag", "v1"]);
    git(&source, &["branch", "feature"]);
    git(&source, &["update-ref", "refs/pull/1/head", "HEAD"]);
    std::fs::write(source.join("b.txt"), "b\n").unwrap();
    git(&source, &["add", "b.txt"]);
    git(&source, &["commit", "-q", "-m", "second"]);

    let backup = root.join("backup");
    let answers = RecordedAnswers {
        repo: "owner/demo".to_string(),
        labels: vec![json!({"name": "bug"}), json!({"name": "size:Easy"})],
        milestones: vec![json!({"title": "1.0"})],
        ..RecordedAnswers::default()
    };
    let mut manifest = write_json(&answers, &backup, "2026-11-09T14:00:00Z").unwrap();
    let bundle = backup.join("repo.bundle");
    git(
        &source,
        &["bundle", "create", "-q", &bundle.to_string_lossy(), "--all"],
    );
    manifest.refs = 4;
    std::fs::write(
        backup.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let server = root.join("server");
    let target = server.join(format!("{NEW}.git"));
    std::fs::create_dir_all(&target).unwrap();
    git(&target, &["init", "-q", "--bare"]);

    let pipeline = root.join("pipeline");
    let stub = pipeline.join("tools").join("fleet");
    std::fs::create_dir_all(&stub).unwrap();
    std::fs::write(stub.join("fleet.py"), STUB).unwrap();
    let fleet = FleetBridge::with_root(&pipeline);
    Fixture {
        backup,
        source,
        server,
        target,
        stub,
        fleet,
    }
}

fn answer(status: u16, body: Value) -> RestAnswer {
    RestAnswer {
        status,
        etag: None,
        body,
    }
}

fn remote<'a>(f: &Fixture, api: &'a Recorded) -> Remote<'a> {
    Remote {
        git_base: f.server.to_string_lossy().into_owned(),
        git_config: Vec::new(),
        api,
    }
}

/// Every batch item the stub queue was given, in order.
fn queued(f: &Fixture) -> Vec<Value> {
    match std::fs::read_to_string(f.stub.join("items.json")) {
        Ok(text) => serde_json::from_str(&text).unwrap(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn a_restore_makes_a_private_repo_with_every_ref_and_the_manifest_counts() {
    let f = fixture("ok");
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(404, json!({"message": "Not Found"})),
            answer(200, json!({"full_name": NEW, "private": true})),
        ],
    );
    let report = restore_history_from(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(
        report,
        RestoreReport {
            refs: 3,
            labels: 2,
            milestones: 1
        }
    );

    for pattern in ["refs/heads", "refs/tags"] {
        assert_eq!(refs_of(&f.target, pattern), refs_of(&f.source, pattern));
    }
    assert!(refs_of(&f.target, "refs/pull").is_empty());

    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(f.backup.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(report.labels, manifest["labels"].as_u64().unwrap());
    assert_eq!(report.milestones, manifest["milestones"].as_u64().unwrap());

    assert_eq!(
        queued(&f),
        vec![
            json!({"kind": "repo", "repo": NEW, "private": true}),
            json!({"kind": "label", "repo": NEW, "name": "bug"}),
            json!({"kind": "label", "repo": NEW, "name": "size:Easy"}),
            json!({"kind": "milestone", "repo": NEW, "title": "1.0"}),
        ]
    );
    assert_eq!(
        *api.log.lock().unwrap(),
        vec![format!("GET /repos/{NEW}"), format!("GET /repos/{NEW}")]
    );
}

#[test]
fn a_name_github_already_has_is_refused_with_no_write() {
    let f = fixture("taken-api");
    let api = Recorded::new(
        Vec::new(),
        vec![answer(200, json!({"full_name": NEW, "private": false}))],
    );
    match restore_history_from(&f.fleet, &f.backup, NEW, &remote(&f, &api)) {
        Err(CoreError::Refused(why)) => assert!(why.contains(NEW), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(queued(&f).is_empty());
    assert!(!f.stub.join("calls.log").exists(), "no fleet call at all");
    assert!(refs_of(&f.target, "refs").is_empty());
}

#[test]
fn a_repo_the_queue_answers_exists_for_is_refused_and_nothing_is_pushed_or_labelled() {
    let f = fixture("taken-queue");
    std::fs::write(f.stub.join("taken"), NEW).unwrap();
    let api = Recorded::new(Vec::new(), vec![answer(404, json!({}))]);
    match restore_history_from(&f.fleet, &f.backup, NEW, &remote(&f, &api)) {
        Err(CoreError::Refused(why)) => assert!(why.contains(NEW), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(
        queued(&f),
        vec![json!({"kind": "repo", "repo": NEW, "private": true})]
    );
    assert!(refs_of(&f.target, "refs").is_empty());
}

#[test]
fn labels_that_do_not_match_the_manifest_stop_before_any_call() {
    let f = fixture("mismatch");
    std::fs::write(f.backup.join("labels.json"), r#"[{"name": "bug"}]"#).unwrap();
    let api = Recorded::new(Vec::new(), Vec::new());
    assert!(matches!(
        restore_history_from(&f.fleet, &f.backup, NEW, &remote(&f, &api)),
        Err(CoreError::Parse(_))
    ));
    assert!(api.log.lock().unwrap().is_empty());
    assert!(!f.stub.join("calls.log").exists());
}
