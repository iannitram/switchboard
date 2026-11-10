//! BAK-3: `restore_issues` queues a backup's issues and comments through the create queue, each
//! comment on the number its issue has in the repo now and nothing the repo already has, and
//! `restore_over_live` restores over an existing repo only when `rules::guard` allows it and the
//! caller confirmed. Git pushes go to a local bare repo (standing in for `https://github.com`),
//! the API is recorded answers, and the create queue is a stub `fleet.py` that keeps its own
//! counts. A `ratelimit` file in the stub makes its run stop with exit 75, as a rate limit does.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::backup::{
    restore_issues, restore_over_live_from, write_json, FileSource, IssuesReport, RecordedAnswers,
    Remote, RestoreReport,
};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::transport::{Recorded, RestAnswer};
use switchboard_core::rules::{Caller, Face, Role};
use switchboard_core::CoreError;

const NEW: &str = "owner/restored";

/// No test here has a release file to fetch.
struct NoFiles;

impl FileSource for NoFiles {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, CoreError> {
        Err(CoreError::GitHub(format!("GET {url}: not expected")))
    }
}

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
    fixture_of(tag, &answers)
}

/// `fixture`, the backup's JSON written from `answers`.
fn fixture_of(tag: &str, answers: &RecordedAnswers) -> Fixture {
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
    let mut manifest = write_json(answers, &backup, "2026-11-09T14:00:00Z").unwrap();
    manifest.refs = 3;
    std::fs::write(
        backup.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
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
        files: &NoFiles,
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

/// The repo's issues as the API lists them: `(number, title)`.
fn listed(issues: &[(u64, &str)]) -> RestAnswer {
    answer(
        200,
        Value::Array(
            issues
                .iter()
                .map(|(number, title)| json!({"number": number, "title": title}))
                .collect(),
        ),
    )
}

const CLOSED_LEFT_OPEN: &str =
    "1 closed issues come back open: the create queue cannot close an issue";

#[test]
fn every_issue_and_comment_is_queued_once_and_the_count_is_the_manifests() {
    let f = fixture("ok");
    // The new repo: no issues, then the two the queue made, then no comments.
    let api = Recorded::new(
        Vec::new(),
        vec![
            listed(&[]),
            listed(&[(1, "Bug"), (2, "Old")]),
            answer(200, json!([])),
        ],
    );
    let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(
        report,
        IssuesReport {
            issues: 2,
            comments: 1,
            left_out: vec![CLOSED_LEFT_OPEN.to_string()],
            ..IssuesReport::default()
        }
    );
    assert_eq!(queued(&f), expected_issue_queue());
}

/// #365: a new repo numbers the issues 1, 2, 3, so the comment on the old #5 goes to #3, and a
/// comment on a PR (not restored) goes nowhere.
#[test]
fn a_comment_goes_to_the_number_its_issue_has_in_the_new_repo() {
    let issue = |n: u64, state: &str| {
        json!({"number": n, "title": format!("issue {n}"), "body": "", "state": state,
               "labels": [], "milestone": null})
    };
    let comment = |on: u64, body: &str| {
        json!({"body": body,
               "issue_url": format!("https://api.github.com/repos/owner/demo/issues/{on}")})
    };
    // Newest first, as the API lists them.
    let answers = RecordedAnswers {
        repo: "owner/demo".to_string(),
        issues: vec![issue(5, "open"), issue(2, "closed"), issue(1, "open")],
        pulls: vec![json!({"number": 4, "state": "closed", "merged_at": null, "body": ""})],
        comments: vec![
            comment(5, "c on five"),
            comment(4, "c on the PR"),
            comment(1, "c on one"),
        ],
        ..RecordedAnswers::default()
    };
    let f = fixture_of("renumber", &answers);
    let api = Recorded::new(
        Vec::new(),
        vec![
            listed(&[]),
            listed(&[(3, "issue 5"), (2, "issue 2"), (1, "issue 1")]),
            answer(200, json!([])),
        ],
    );

    let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();

    let queue = queued(&f);
    let titles: Vec<&str> = queue
        .iter()
        .filter(|item| item["kind"] == "issue")
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["issue 1", "issue 2", "issue 5"], "oldest first");
    let comments: Vec<(u64, &str)> = queue
        .iter()
        .filter(|item| item["kind"] == "comment")
        .map(|item| {
            (
                item["number"].as_u64().unwrap(),
                item["body"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(comments, [(3, "c on five"), (1, "c on one")]);
    assert_eq!(
        report,
        IssuesReport {
            issues: 3,
            comments: 2,
            left_out: vec![
                CLOSED_LEFT_OPEN.to_string(),
                "1 comments on PRs are not restored: PRs are not".to_string(),
            ],
            ..IssuesReport::default()
        }
    );
}

#[test]
fn a_comments_file_that_is_not_the_manifests_count_stops_before_the_queue() {
    let f = fixture("mismatch");
    std::fs::write(f.backup.join("comments.json"), "[]").unwrap();
    let api = Recorded::new(Vec::new(), Vec::new());
    assert!(matches!(
        restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)),
        Err(CoreError::Parse(_))
    ));
    assert!(api.log.lock().unwrap().is_empty(), "no API call at all");
    assert!(!f.stub.join("calls.log").exists(), "no fleet call at all");
}

/// A rate limit leaves the issues queued, so the repo does not have them yet: their comments
/// wait (they are never queued on the backup's numbers), and a second call queues them.
#[test]
fn a_rate_limit_stop_leaves_the_issues_queued_and_holds_their_comments_back() {
    let f = fixture("ratelimit");
    std::fs::write(f.stub.join("ratelimit"), "").unwrap();
    let api = Recorded::new(
        Vec::new(),
        vec![listed(&[]), listed(&[]), answer(200, json!([]))],
    );
    let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(
        report,
        IssuesReport {
            issues: 2,
            comments_waiting: 1,
            left_out: vec![CLOSED_LEFT_OPEN.to_string()],
            ..IssuesReport::default()
        }
    );
    assert_eq!(queued(&f), expected_issue_queue()[..2]);

    // Later the queue has made them: the second call queues only the comment.
    std::fs::remove_file(f.stub.join("ratelimit")).unwrap();
    let api = Recorded::new(
        Vec::new(),
        vec![listed(&[(1, "Bug"), (2, "Old")]), answer(200, json!([]))],
    );
    let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(
        report,
        IssuesReport {
            issues_there: 2,
            comments: 1,
            left_out: vec![CLOSED_LEFT_OPEN.to_string()],
            ..IssuesReport::default()
        }
    );
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
    // The live repo has no labels, milestones, issues or comments yet.
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(200, json!([])),
            answer(200, json!([])),
            listed(&[]),
            listed(&[(1, "Bug"), (2, "Old")]),
            answer(200, json!([])),
        ],
    );
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
            milestones: 1,
            issues: IssuesReport {
                issues: 2,
                comments: 1,
                left_out: vec![CLOSED_LEFT_OPEN.to_string()],
                ..IssuesReport::default()
            },
            left_out: Vec::new(),
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
    assert_eq!(
        *api.log.lock().unwrap(),
        vec![
            format!("GET /repos/{NEW}"),
            format!("GET /repos/{NEW}/labels?per_page=100&page=1"),
            format!("GET /repos/{NEW}/milestones?state=all&per_page=100&page=1"),
            format!("GET /repos/{NEW}/issues?state=all&per_page=100&page=1"),
            format!("GET /repos/{NEW}/issues?state=all&per_page=100&page=1"),
            format!("GET /repos/{NEW}/issues/comments?per_page=100&page=1"),
        ]
    );
}

/// #366: the live repo already holds everything the backup does (as after one restore), so a
/// restore over it creates nothing again.
#[test]
fn a_restore_over_a_repo_that_has_it_all_queues_nothing() {
    let f = fixture("live-again");
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(200, json!([{"name": "Bug"}, {"name": "size:Easy"}])),
            answer(200, json!([{"title": "1.0"}])),
            listed(&[(2, "Old"), (1, "Bug")]),
            // As the queue posts a comment: the body, then its own marker line.
            answer(
                200,
                json!([{"body": "me too\n\n<!-- ghq:0123456789ab -->\n",
                        "issue_url": format!("https://api.github.com/repos/{NEW}/issues/1")}]),
            ),
        ],
    );
    let report = restore_over_live_from(
        &caller(Face::Gui, false),
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &remote(&f, &api),
    )
    .unwrap();
    assert_eq!(queued(&f), Vec::<Value>::new(), "nothing is created again");
    assert_eq!(
        report,
        RestoreReport {
            refs: 3,
            issues: IssuesReport {
                issues_there: 2,
                comments_there: 1,
                left_out: vec![CLOSED_LEFT_OPEN.to_string()],
                ..IssuesReport::default()
            },
            ..RestoreReport::default()
        }
    );
}

/// #366: only what the live repo lacks is created: here one label, one issue and its comment.
#[test]
fn a_restore_over_a_repo_that_has_some_of_it_queues_only_the_rest() {
    let f = fixture("live-some");
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(200, json!([{"name": "size:Easy"}])),
            answer(200, json!([{"title": "1.0"}])),
            listed(&[(7, "Old")]),
            listed(&[(8, "Bug"), (7, "Old")]),
            answer(200, json!([])),
        ],
    );
    restore_over_live_from(
        &caller(Face::Gui, false),
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &remote(&f, &api),
    )
    .unwrap();
    assert_eq!(
        queued(&f),
        vec![
            json!({"kind": "label", "repo": NEW, "name": "bug"}),
            json!({"kind": "issue", "repo": NEW, "title": "Bug", "body": "broken",
                   "labels": ["bug"], "milestone": "1.0"}),
            json!({"kind": "comment", "repo": NEW, "number": 8, "body": "me too", "marker": null}),
        ]
    );
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
