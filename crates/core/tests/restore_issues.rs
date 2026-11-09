//! BAK-3: `restore_issues` queues a backup's issues and comments through the create queue, and
//! `restore_over_live` restores over an existing repo only when `rules::guard` allows it and the
//! caller confirmed. Git pushes go to a local bare repo (standing in for `https://github.com`),
//! the API is recorded answers, and the create queue is a stub `fleet.py` that keeps its own
//! counts. A `ratelimit` file in the stub makes its run stop with exit 75, as a rate limit does.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::backup::{
    restore_issues, restore_over_live_from, write_json, RecordedAnswers, Remote, RestoreReport,
};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::transport::{Recorded, RestAnswer};
use switchboard_core::rules::{Caller, Face, Role};
use switchboard_core::CoreError;

const NEW: &str = "owner/restored";

/// A fixture `fleet ghq`: logs each call, keeps every batch item in `items.json`, and answers
/// `status` with counts: a repo named in `taken` is `exists`, an item whose kind is in `fail` is
/// `failed`, everything is `pending` while `ratelimit` exists (and `run` stops with exit 75),
/// anything else is `done`.
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
def flag(name):
    path = os.path.join(here, name)
    return open(path).read().split() if os.path.exists(path) else []
taken = flag("taken")
failing = flag("fail")
limited = os.path.exists(os.path.join(here, "ratelimit"))
counts = {"pending": 0, "done": 0, "exists": 0, "failed": 0}
for item in items:
    taken_repo = item["kind"] == "repo" and item["repo"] in taken
    state = "exists" if taken_repo else "failed" if item["kind"] in failing else "done"
    if limited:
        state = "pending"
    counts[state] += 1
if sub == "run" and limited:
    print(json.dumps({"stopped": "rate limit"}))
    sys.exit(75)
print(json.dumps({"counts": counts, "total": len(items)}))
"#;

fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!(
        "sb-bak3-issues-{tag}-{}-{nanos}",
        std::process::id()
    ));
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

/// A backup folder (bundle of `master`, `feature` and `v1`; two issues, one of them closed, one
/// comment on the first; two labels; one milestone; the manifest), an empty bare repo at
/// `<server>/owner/restored.git`, and the stub queue.
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
    std::fs::write(source.join("b.txt"), "b\n").unwrap();
    git(&source, &["add", "b.txt"]);
    git(&source, &["commit", "-q", "-m", "second"]);

    let backup = root.join("backup");
    let answers = RecordedAnswers {
        repo: "owner/demo".to_string(),
        issues: vec![
            json!({"number": 1, "title": "Bug", "body": "broken", "state": "open",
                   "labels": [{"name": "bug"}], "milestone": {"title": "1.0"}}),
            json!({"number": 2, "title": "Old", "body": null, "state": "closed",
                   "labels": [], "milestone": null}),
        ],
        comments: vec![json!({
            "id": 10,
            "body": "me too",
            "issue_url": "https://api.github.com/repos/owner/demo/issues/1",
        })],
        labels: vec![json!({"name": "bug"}), json!({"name": "size:Easy"})],
        milestones: vec![json!({"title": "1.0"})],
        ..RecordedAnswers::default()
    };
    write_json(&answers, &backup, "2026-11-09T14:00:00Z").unwrap();
    git(
        &source,
        &[
            "bundle",
            "create",
            "-q",
            &backup.join("repo.bundle").to_string_lossy(),
            "--all",
        ],
    );

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

/// A caller of `face`; the app's caller has confirmed in the app, others have not.
fn caller(face: Face, in_job_worktree: bool) -> Caller {
    Caller {
        face,
        role: Some(Role::ProjectManager),
        in_job_worktree,
        confirmed_in_app: face == Face::Gui,
    }
}

fn expected_issue_queue() -> Vec<Value> {
    vec![
        json!({"kind": "issue", "repo": NEW, "title": "Bug", "body": "broken",
               "labels": ["bug"], "milestone": "1.0"}),
        json!({"kind": "issue", "repo": NEW, "title": "Old", "body": "",
               "labels": [], "milestone": null}),
        json!({"kind": "comment", "repo": NEW, "number": 1, "body": "me too", "marker": null}),
    ]
}

#[test]
fn every_issue_and_comment_is_queued_once_and_the_count_is_the_manifests() {
    let f = fixture("ok");
    let queued_count = restore_issues(&f.fleet, &f.backup, NEW).unwrap();
    assert_eq!(queued_count, 3);
    assert_eq!(queued(&f), expected_issue_queue());
}

#[test]
fn a_comments_file_that_is_not_the_manifests_count_stops_before_the_queue() {
    let f = fixture("mismatch");
    std::fs::write(f.backup.join("comments.json"), "[]").unwrap();
    assert!(matches!(
        restore_issues(&f.fleet, &f.backup, NEW),
        Err(CoreError::Parse(_))
    ));
    assert!(!f.stub.join("calls.log").exists(), "no fleet call at all");
}

#[test]
fn a_rate_limit_stop_leaves_the_items_queued_not_failed() {
    let f = fixture("ratelimit");
    std::fs::write(f.stub.join("ratelimit"), "").unwrap();
    assert_eq!(restore_issues(&f.fleet, &f.backup, NEW).unwrap(), 3);
    assert_eq!(queued(&f), expected_issue_queue());
}

#[test]
fn an_unconfirmed_restore_over_a_live_repo_writes_nothing() {
    let f = fixture("unconfirmed");
    let api = Recorded::new(Vec::new(), vec![answer(200, json!({"private": true}))]);
    match restore_over_live_from(
        &caller(Face::Gui, false),
        &f.fleet,
        &f.backup,
        NEW,
        false,
        &remote(&f, &api),
    ) {
        Err(CoreError::Refused(why)) => assert!(why.contains(NEW), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(!f.stub.join("calls.log").exists(), "no fleet call at all");
    assert!(refs_of(&f.target, "refs").is_empty());
}

#[test]
fn a_guard_refusal_over_a_live_repo_writes_nothing() {
    let f = fixture("guard");
    let api = Recorded::new(Vec::new(), vec![answer(200, json!({"private": true}))]);
    for who in [caller(Face::Cli, false), caller(Face::Gui, true)] {
        match restore_over_live_from(&who, &f.fleet, &f.backup, NEW, true, &remote(&f, &api)) {
            Err(CoreError::Refused(why)) => assert!(why.contains(NEW), "{why}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert!(api.log.lock().unwrap().is_empty(), "no API call at all");
    assert!(!f.stub.join("calls.log").exists(), "no fleet call at all");
    assert!(refs_of(&f.target, "refs").is_empty());
}

#[test]
fn a_confirmed_restore_over_a_live_repo_pushes_every_ref_and_queues_the_rest() {
    let f = fixture("live");
    let api = Recorded::new(Vec::new(), vec![answer(200, json!({"private": true}))]);
    let report = restore_over_live_from(
        &caller(Face::Gui, false),
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &remote(&f, &api),
    )
    .unwrap();
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
    let mut expected = vec![
        json!({"kind": "label", "repo": NEW, "name": "bug"}),
        json!({"kind": "label", "repo": NEW, "name": "size:Easy"}),
        json!({"kind": "milestone", "repo": NEW, "title": "1.0"}),
    ];
    expected.extend(expected_issue_queue());
    assert_eq!(queued(&f), expected);
    assert_eq!(*api.log.lock().unwrap(), vec![format!("GET /repos/{NEW}")]);
}

#[test]
fn a_restore_over_a_repo_that_is_not_there_writes_nothing() {
    let f = fixture("absent");
    let api = Recorded::new(
        Vec::new(),
        vec![answer(404, json!({"message": "Not Found"}))],
    );
    assert!(matches!(
        restore_over_live_from(
            &caller(Face::Gui, false),
            &f.fleet,
            &f.backup,
            NEW,
            true,
            &remote(&f, &api),
        ),
        Err(CoreError::GitHub(_))
    ));
    assert!(!f.stub.join("calls.log").exists(), "no fleet call at all");
    assert!(refs_of(&f.target, "refs").is_empty());
}
