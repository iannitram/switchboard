//! BAK-3: `restore_issues` queues a backup's issues and comments through the create queue, each
//! comment on the number its issue has in the repo now and nothing the repo already has, and
//! `restore_over_live` restores over an existing repo only when `rules::guard` allows it and the
//! caller confirmed. Git pushes go to a local bare repo (standing in for `https://github.com`),
//! the API is recorded answers, and the create queue is a stub `fleet.py` that keeps its own
//! counts. A `ratelimit` file in the stub makes its run stop with exit 75, as a rate limit does.
//!
//! #365: issues and milestones that were closed in the backup are closed once they are there,
//! and a `#N` in a restored body or comment gets the number its issue has now. #347: the second
//! confirm of a restore that held something back does only that, and pushes no ref again.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::backup::{
    restore_held_over_live_from, restore_issues, restore_over_live_from, write_json, Created,
    FileSource, IssuesReport, Made, RecordedAnswers, Remote, RestoreReport,
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
/// `status` with counts: a repo named in `taken`, or an issue titled in `found`, is `exists`, an item whose kind is in `fail` is
/// `failed`, everything is `pending` while `ratelimit` exists (and `run` stops with exit 75),
/// anything else is `done`. `add-batch` answers each item's key (kind, repo and title or name, as
/// the queue's are) and whether it was new to the queue. Its own record, `queue.json` in the
/// folder `status` names, holds each key once, with its status and, when done, the URL of what
/// it made: `made.json` maps an issue's title, or `milestone:<title>`, to the number GitHub gave
/// it; anything else done says only "created".
const STUB: &str = r#"import hashlib, json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(args) + "\n")
items_file = os.path.join(here, "items.json")
items = json.load(open(items_file)) if os.path.exists(items_file) else []
sub = args[1] if len(args) > 1 else ""
def key_of(item):
    what = item.get("title") or item.get("name") or json.dumps([item.get("number"), item.get("body")])
    return hashlib.sha1(("%s|%s|%s" % (item["kind"], item["repo"].lower(), what)).encode()).hexdigest()[:16]
added = []
if sub == "add-batch":
    seen = set(key_of(i) for i in items)
    for item in json.load(open(args[2])):
        added.append({"key": key_of(item), "added": key_of(item) not in seen, "status": "pending"})
        seen.add(key_of(item))
        items.append(item)
    json.dump(items, open(items_file, "w"))
def flag(name):
    path = os.path.join(here, name)
    return open(path).read().split() if os.path.exists(path) else []
taken = flag("taken")
failing = flag("fail")
limited = os.path.exists(os.path.join(here, "ratelimit"))
made_file = os.path.join(here, "made.json")
made = json.load(open(made_file)) if os.path.exists(made_file) else {}
counts = {"pending": 0, "done": 0, "exists": 0, "failed": 0}
record = {}
for item in items:
    taken_repo = item["kind"] == "repo" and item["repo"] in taken
    found = item["kind"] == "issue" and item["title"] in flag("found")
    state = "exists" if taken_repo or found else "failed" if item["kind"] in failing else "done"
    if limited:
        state = "pending"
    counts[state] += 1
    result = None
    if state == "done":
        result = "created"
        if item["kind"] == "issue" and item["title"] in made:
            result = "https://github.com/%s/issues/%d" % (item["repo"], made[item["title"]])
        if item["kind"] == "milestone" and "milestone:" + item["title"] in made:
            result = "https://github.com/%s/milestone/%d" % (item["repo"], made["milestone:" + item["title"]])
    record.setdefault(key_of(item), {"key": key_of(item), "status": state, "result": result})
json.dump({"version": 1, "items": list(record.values())}, open(os.path.join(here, "queue.json"), "w"))
if sub == "add-batch":
    print(json.dumps(added))
    sys.exit(0)
if sub == "run" and limited:
    print(json.dumps({"stopped": "rate limit"}))
    sys.exit(75)
print(json.dumps({"counts": counts, "total": len(items), "dir": here}))
"#;

/// Says in the stub queue's own record which number GitHub gave each create it made: an
/// issue's title, or `milestone:<title>`, to the number.
fn made_as(f: &Fixture, numbers: &[(&str, u64)]) {
    let map: serde_json::Map<String, Value> = numbers
        .iter()
        .map(|(what, n)| (what.to_string(), json!(n)))
        .collect();
    std::fs::write(f.stub.join("made.json"), Value::Object(map).to_string()).unwrap();
}

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

/// A number the queue gave a create, by what it is in the backup; `None` while it waits.
type Numbered<T> = Vec<(T, Option<u64>)>;

/// The numbers a record gives its issues (by the backup's number) and milestones.
fn numbers(made: &Made) -> (Numbered<u64>, Numbered<String>) {
    (
        made.issues
            .iter()
            .map(|(old, c)| (*old, c.number))
            .collect(),
        made.milestones
            .iter()
            .map(|(title, c)| (title.clone(), c.number))
            .collect(),
    )
}

/// A record of what a restore made: these issues (backup number, number now) and no milestone.
fn record(issues: &[(u64, u64)]) -> Made {
    Made {
        issues: issues
            .iter()
            .map(|(old, now)| {
                (
                    *old,
                    Created {
                        key: format!("key-{old}"),
                        number: Some(*now),
                    },
                )
            })
            .collect(),
        ..Made::default()
    }
}

/// The PATCH requests in `api`'s log, in order.
fn patches(api: &Recorded) -> Vec<String> {
    api.log
        .lock()
        .unwrap()
        .iter()
        .filter(|line| line.starts_with("PATCH "))
        .cloned()
        .collect()
}

/// The one close the fixture's backup calls for: its issue 2 ("Old") was closed.
fn closes_old(number: u64) -> Vec<String> {
    vec![format!(
        r#"PATCH /repos/{NEW}/issues/{number} {{"state":"closed"}}"#
    )]
}

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
            closed: 1,
            ..IssuesReport::default()
        }
    );
    assert_eq!(queued(&f), expected_issue_queue());
    // #365: the issue that was closed in the backup is closed once it is there.
    assert_eq!(patches(&api), closes_old(2));
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
            closed: 1,
            left_out: vec!["1 comments on PRs are not restored: PRs are not".to_string()],
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
            state_waiting: 1,
            ..IssuesReport::default()
        }
    );
    assert!(report.held());
    assert_eq!(queued(&f), expected_issue_queue()[..2]);
    assert_eq!(patches(&api), Vec::<String>::new(), "no issue to close yet");

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
            closed: 1,
            ..IssuesReport::default()
        }
    );
    assert!(!report.held());
    assert_eq!(queued(&f), expected_issue_queue());
    assert_eq!(patches(&api), closes_old(2));
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
    made_as(&f, &[("Bug", 1), ("Old", 2), ("milestone:1.0", 1)]);
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
                closed: 1,
                ..IssuesReport::default()
            },
            left_out: Vec::new(),
            made: report.made.clone(),
        }
    );
    // What the restore made, with the numbers the queue says it gave them.
    assert_eq!(
        numbers(&report.made),
        (
            vec![(1, Some(1)), (2, Some(2))],
            vec![("1.0".to_string(), Some(1))]
        )
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
            closes_old(2).remove(0),
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
            // "Old" is closed there, as in the backup.
            answer(
                200,
                json!([{"number": 2, "title": "Old", "state": "closed", "body": ""},
                       {"number": 1, "title": "Bug", "state": "open", "body": "broken"}]),
            ),
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
                ..IssuesReport::default()
            },
            ..RestoreReport::default()
        }
    );
    assert_eq!(
        patches(&api),
        Vec::<String>::new(),
        "nothing is closed again"
    );
}

/// #366: only what the live repo lacks is created: here one label, one issue and its comment.
#[test]
fn a_restore_over_a_repo_that_has_some_of_it_queues_only_the_rest() {
    let f = fixture("live-some");
    made_as(&f, &[("Bug", 8)]);
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
    // #365: "Old" was in the repo before the restore, open; it is not the restore's to close.
    assert_eq!(patches(&api), Vec::<String>::new());
}

/// #365: over a live repo only what the restore made is closed or renumbered. An issue and a
/// milestone the repo had already, open, stay open though the backup has them closed, and the
/// report says so; the issue the restore queued is closed.
#[test]
fn a_restore_over_a_live_repo_closes_only_what_it_made() {
    let f = fixture("live-made");
    made_as(&f, &[("Bug", 8)]);
    edit_json(&f.backup, "issues.json", |issues| {
        for issue in issues.as_array_mut().unwrap() {
            issue["state"] = json!("closed");
            issue["body"] = json!("see #1 and #2");
        }
    });
    std::fs::write(
        f.backup.join("milestones.json"),
        r#"[{"title": "1.0", "state": "closed"}]"#,
    )
    .unwrap();
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(200, json!([{"name": "Bug"}, {"name": "size:Easy"}])),
            answer(200, json!([{"title": "1.0", "state": "open"}])),
            // The repo had "Old" (#7), open, with the backup's body.
            answer(
                200,
                json!([{"number": 7, "title": "Old", "state": "open", "body": "see #1 and #2"}]),
            ),
            answer(
                200,
                json!([{"number": 7, "title": "Old", "state": "open", "body": "see #1 and #2"},
                       {"number": 8, "title": "Bug", "state": "open", "body": "see #1 and #2"}]),
            ),
            answer(200, json!([])),
            answer(200, json!([{"number": 3, "title": "1.0", "state": "open"}])),
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
        patch_bodies(&api),
        vec![(
            format!("/repos/{NEW}/issues/8"),
            json!({"body": "see #8 and #7", "state": "closed"})
        )],
        "only the issue the restore queued"
    );
    assert_eq!(
        report.issues,
        IssuesReport {
            issues: 1,
            issues_there: 1,
            comments: 1,
            closed: 1,
            renumbered: 1,
            left_out: vec![
                "1 issues closed in the backup are open in the repo and were left open, as this \
                 restore did not make them: #7"
                    .to_string(),
                "1 milestones closed in the backup are open in the repo and were left open, as \
                 this restore did not make them: 1.0"
                    .to_string(),
                "1 issue bodies keep the backup's #N, not rewritten: #7 (not made by this \
                 restore)"
                    .to_string(),
            ],
            ..IssuesReport::default()
        }
    );
}

/// #365: the held pass closes only what the record of the whole pass says it made; an issue
/// the repo had, open, stays open, is named, and is not counted as waiting.
#[test]
fn the_held_pass_closes_nothing_the_restore_did_not_make() {
    let f = fixture("held-before");
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            listed(&[(1, "Bug"), (2, "Old")]),
            answer(200, json!([])),
        ],
    );
    let report = restore_held_over_live_from(
        &caller(Face::Gui, false),
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &record(&[(1, 1)]),
        &remote(&f, &api),
    )
    .unwrap();
    assert_eq!(patches(&api), Vec::<String>::new());
    assert_eq!(
        report.issues,
        IssuesReport {
            issues_there: 2,
            comments: 1,
            left_out: vec![
                "1 issues closed in the backup are open in the repo and were left open, as this \
                 restore did not make them: #2"
                    .to_string()
            ],
            ..IssuesReport::default()
        }
    );
    assert!(
        !report.issues.held(),
        "nothing waits: the request can leave"
    );
    assert_eq!(report.made, record(&[(1, 1)]));
}

/// #365 (second check): the whole pass matches the repo's issues again after the queue has
/// run, and with two issues of one title an exact number match would win. The restore closes
/// only the number the queue says it made: the repo's own "Dup" (#7) is never touched.
#[test]
fn a_whole_pass_closes_the_issue_the_queue_made_not_one_with_the_same_title() {
    let f = fixture("dup-swap");
    edit_json(&f.backup, "issues.json", |issues| {
        let a = issues.as_array_mut().unwrap();
        a[0]["number"] = json!(8);
        a[0]["title"] = json!("Dup");
        a[0]["state"] = json!("open");
        a[1]["number"] = json!(9);
        a[1]["title"] = json!("Dup");
        a[1]["state"] = json!("closed");
    });
    made_as(&f, &[("Dup", 8)]);
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(200, json!([{"name": "bug"}, {"name": "size:Easy"}])),
            answer(200, json!([{"title": "1.0"}])),
            // The repo had one "Dup" (#7), open, before the restore.
            answer(
                200,
                json!([{"number": 7, "title": "Dup", "state": "open", "body": "mine"}]),
            ),
            // The queue made the missing one; GitHub gave it the next number, 8.
            answer(
                200,
                json!([{"number": 7, "title": "Dup", "state": "open", "body": "mine"},
                       {"number": 8, "title": "Dup", "state": "open", "body": ""}]),
            ),
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
        patch_bodies(&api),
        vec![(format!("/repos/{NEW}/issues/8"), json!({"state": "closed"}))],
        "the backup's closed #9 is the #8 the queue made; #7 was the repo's"
    );
    assert_eq!(numbers(&report.made).0, vec![(9, Some(8))]);
    assert!(!report.issues.held());
}

/// #365: duplicate titles in the backup and in the repo. The queue holds one item per title, so
/// of the two missing "Dup" only the first is the restore's; the other is not attributed, and
/// stays as it is and named. The repo's own "Dup" (#5), open, is left open and named.
#[test]
fn duplicate_titles_on_both_sides_close_only_what_the_queue_made() {
    let issue = |n: u64, state: &str| {
        json!({"number": n, "title": "Dup", "body": "", "state": state,
               "labels": [], "milestone": null})
    };
    let answers = RecordedAnswers {
        repo: "owner/demo".to_string(),
        issues: vec![issue(3, "open"), issue(2, "closed"), issue(1, "closed")],
        ..RecordedAnswers::default()
    };
    let f = fixture_of("dup-both", &answers);
    made_as(&f, &[("Dup", 6)]);
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(200, json!([])),
            answer(200, json!([])),
            answer(
                200,
                json!([{"number": 5, "title": "Dup", "state": "open", "body": ""}]),
            ),
            answer(
                200,
                json!([{"number": 5, "title": "Dup", "state": "open", "body": ""},
                       {"number": 6, "title": "Dup", "state": "open", "body": ""}]),
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
    let issues_queued = queued(&f)
        .iter()
        .filter(|item| item["kind"] == "issue")
        .count();
    assert_eq!(issues_queued, 2, "the backup's #2 and #3 were sent");
    assert_eq!(
        patch_bodies(&api),
        vec![(format!("/repos/{NEW}/issues/6"), json!({"state": "closed"}))]
    );
    assert_eq!(numbers(&report.made).0, vec![(2, Some(6))]);
    assert_eq!(
        report.issues.left_out,
        vec![
            "1 issues closed in the backup are open in the repo and were left open, as this \
             restore did not make them: #5"
                .to_string(),
            "1 issues or milestones of the backup are not in the repo and the create queue did \
             not say this restore made them, so nothing was changed for them: backup #3"
                .to_string(),
        ]
    );
    assert!(!report.issues.held());
}

/// #365 (second check): an issue someone opened after the request, before the first confirm,
/// is not the restore's: neither pass closes it, whatever its time.
#[test]
fn an_issue_someone_made_before_the_first_confirm_is_never_closed() {
    let f = fixture("window");
    made_as(&f, &[("Bug", 8)]);
    let app = caller(Face::Gui, false);
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(200, json!([{"name": "bug"}, {"name": "size:Easy"}])),
            answer(200, json!([{"title": "1.0"}])),
            listed(&[(7, "Old")]),
            listed(&[(7, "Old"), (8, "Bug")]),
            answer(200, json!([])),
        ],
    );
    let first =
        restore_over_live_from(&app, &f.fleet, &f.backup, NEW, true, &remote(&f, &api)).unwrap();
    assert_eq!(
        patches(&api),
        Vec::<String>::new(),
        "the whole pass leaves #7"
    );
    assert!(
        first.issues.left_out[0].ends_with("did not make them: #7"),
        "{:?}",
        first.issues.left_out
    );
    assert_eq!(numbers(&first.made).0, vec![(1, Some(8))]);
    // A second confirm, with the record of the first.
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            listed(&[(7, "Old"), (8, "Bug")]),
            answer(200, json!([])),
        ],
    );
    restore_held_over_live_from(
        &app,
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &first.made,
        &remote(&f, &api),
    )
    .unwrap();
    assert_eq!(
        patches(&api),
        Vec::<String>::new(),
        "the held pass leaves #7 too"
    );
}

/// #365: someone opens an issue with a backed-up title while the restore's create waits in the
/// queue; the queue finds it there (`exists`) and makes nothing. It is not the restore's.
#[test]
fn an_issue_the_queue_found_there_is_not_the_restores() {
    let f = fixture("found");
    made_as(&f, &[("Bug", 9)]);
    std::fs::write(f.stub.join("found"), "Old").unwrap();
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(200, json!([{"name": "bug"}, {"name": "size:Easy"}])),
            answer(200, json!([{"title": "1.0"}])),
            listed(&[]),
            listed(&[(8, "Old"), (9, "Bug")]),
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
    assert_eq!(patches(&api), Vec::<String>::new(), "#8 is someone else's");
    assert_eq!(numbers(&report.made).0, vec![(1, Some(9))]);
    assert!(
        report.issues.left_out[0].ends_with("did not make them: #8"),
        "{:?}",
        report.issues.left_out
    );
}

/// Second check of #417: what is at a recorded number is closed or rewritten only while it
/// still has the backup's title. An issue or milestone with another title there now is left
/// as it is and named; it does not hold the restore.
#[test]
fn what_is_at_a_recorded_number_with_another_title_is_left_and_named() {
    let f = fixture("retitled");
    std::fs::write(
        f.backup.join("milestones.json"),
        r#"[{"title": "1.0", "state": "closed"}]"#,
    )
    .unwrap();
    edit_json(&f.backup, "issues.json", |issues| {
        issues[0]["body"] = json!("see #2");
    });
    let mut made = record(&[(1, 1), (2, 2)]);
    made.milestones.insert(
        "1.0".to_string(),
        Created {
            key: "key-milestone".to_string(),
            number: Some(3),
        },
    );
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(
                200,
                json!([{"number": 1, "title": "Not the bug", "state": "open", "body": "see #2"},
                       {"number": 2, "title": "Someone else's", "state": "open", "body": ""}]),
            ),
            answer(200, json!([])),
            answer(
                200,
                json!([{"number": 3, "title": "Another plan", "state": "open"}]),
            ),
        ],
    );
    let report = restore_held_over_live_from(
        &caller(Face::Gui, false),
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &made,
        &remote(&f, &api),
    )
    .unwrap();
    assert_eq!(patches(&api), Vec::<String>::new(), "nothing was changed");
    assert_eq!(report.issues.closed, 0);
    assert_eq!(report.issues.milestones_closed, 0);
    assert_eq!(report.issues.state_waiting, 0, "and nothing waits for it");
    assert!(
        report.issues.left_out.contains(
            &"2 issues or milestones this restore made have another title in the repo now and \
              were left as they are: #2, milestone Another plan"
                .to_string()
        ),
        "{:?}",
        report.issues.left_out
    );
}

/// #365 (second check): the held pass closes a closed milestone only by the number the queue
/// says it made for the restore; a milestone the repo had, open, stays open and is named.
#[test]
fn the_held_pass_closes_only_the_milestone_the_restore_made() {
    let f = fixture("held-milestone");
    std::fs::write(
        f.backup.join("milestones.json"),
        r#"[{"title": "1.0", "state": "closed"}, {"title": "2.0", "state": "closed"}]"#,
    )
    .unwrap();
    edit_json(&f.backup, "manifest.json", |m| m["milestones"] = json!(2));
    std::fs::write(f.stub.join("ratelimit"), "").unwrap();
    let app = caller(Face::Gui, false);
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(200, json!([])),
            // The repo had "2.0", open.
            answer(200, json!([{"number": 1, "title": "2.0", "state": "open"}])),
            listed(&[]),
            listed(&[]),
            answer(200, json!([])),
            answer(200, json!([{"number": 1, "title": "2.0", "state": "open"}])),
        ],
    );
    let first =
        restore_over_live_from(&app, &f.fleet, &f.backup, NEW, true, &remote(&f, &api)).unwrap();
    assert_eq!(patches(&api), Vec::<String>::new());
    assert_eq!(
        numbers(&first.made),
        (vec![(1, None), (2, None)], vec![("1.0".to_string(), None)]),
        "all still in the queue"
    );
    assert!(first.issues.held());

    // The queue has made them; the second confirm.
    std::fs::remove_file(f.stub.join("ratelimit")).unwrap();
    made_as(&f, &[("Bug", 1), ("Old", 2), ("milestone:1.0", 3)]);
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            listed(&[(1, "Bug"), (2, "Old")]),
            answer(200, json!([])),
            answer(
                200,
                json!([{"number": 1, "title": "2.0", "state": "open"},
                       {"number": 3, "title": "1.0", "state": "open"}]),
            ),
        ],
    );
    let second = restore_held_over_live_from(
        &app,
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &first.made,
        &remote(&f, &api),
    )
    .unwrap();
    assert_eq!(
        patch_bodies(&api),
        vec![
            (format!("/repos/{NEW}/issues/2"), json!({"state": "closed"})),
            (
                format!("/repos/{NEW}/milestones/3"),
                json!({"state": "closed"})
            ),
        ],
        "never the repo's own milestone 1"
    );
    assert_eq!(
        second.issues.left_out,
        vec![
            "1 milestones closed in the backup are open in the repo and were left open, as \
             this restore did not make them: 2.0"
                .to_string()
        ]
    );
    assert!(!second.issues.held());
    assert_eq!(numbers(&second.made).1, vec![("1.0".to_string(), Some(3))]);
}

/// A closed milestone is closed by the number the queue made, never by its title: when that
/// one is gone and the repo has another of the same title (#4), nothing is closed.
#[test]
fn a_milestone_is_closed_by_the_number_the_queue_made_not_by_title() {
    let f = fixture("milestone-number");
    std::fs::write(
        f.backup.join("milestones.json"),
        r#"[{"title": "1.0", "state": "closed"}]"#,
    )
    .unwrap();
    let mut made = record(&[(1, 1), (2, 2)]);
    made.milestones.insert(
        "1.0".to_string(),
        Created {
            key: "key-1.0".to_string(),
            number: Some(3),
        },
    );
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(
                200,
                json!([{"number": 1, "title": "Bug", "state": "open", "body": "broken"},
                       {"number": 2, "title": "Old", "state": "closed", "body": ""}]),
            ),
            answer(200, json!([])),
            answer(200, json!([{"number": 4, "title": "1.0", "state": "open"}])),
        ],
    );
    restore_held_over_live_from(
        &caller(Face::Gui, false),
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &made,
        &remote(&f, &api),
    )
    .unwrap();
    assert_eq!(patches(&api), Vec::<String>::new());
}

/// A rewrite of a `#N` the restore made but someone changed since is skipped, and named.
#[test]
fn a_skipped_number_rewrite_is_named() {
    let f = fixture("skipped");
    edit_json(&f.backup, "issues.json", |issues| {
        issues.as_array_mut().unwrap()[0]["body"] = json!("see #2");
    });
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(
                200,
                json!([{"number": 4, "title": "Bug", "state": "open", "body": "see #2, edited"},
                       {"number": 5, "title": "Old", "state": "closed", "body": ""}]),
            ),
            answer(200, json!([])),
        ],
    );
    let report = restore_held_over_live_from(
        &caller(Face::Gui, false),
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &record(&[(1, 4), (2, 5)]),
        &remote(&f, &api),
    )
    .unwrap();
    assert_eq!(patches(&api), Vec::<String>::new());
    assert_eq!(
        report.issues.left_out,
        vec![
            "1 issue bodies keep the backup's #N, not rewritten: #4 (changed in the repo since)"
                .to_string()
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

/// Each PATCH in `api`'s log as `(path, body)`.
fn patch_bodies(api: &Recorded) -> Vec<(String, Value)> {
    patches(api)
        .iter()
        .map(|line| {
            let rest = line.strip_prefix("PATCH ").unwrap();
            let (path, body) = rest.split_once(' ').unwrap();
            (path.to_string(), serde_json::from_str(body).unwrap())
        })
        .collect()
}

/// Rewrites one of the backup's JSON files in place.
fn edit_json(backup: &Path, file: &str, change: impl Fn(&mut Value)) {
    let path = backup.join(file);
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    change(&mut value);
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
}

/// #365, owner decision 4: an issue closed in the backup is closed after it is created, with
/// the reason it had, and a closed milestone is closed once its issues are there.
#[test]
fn closed_issues_and_milestones_are_closed_after_they_are_created() {
    let f = fixture("closed");
    edit_json(&f.backup, "issues.json", |issues| {
        for issue in issues.as_array_mut().unwrap() {
            if issue["number"] == 2 {
                issue["state_reason"] = json!("not_planned");
            }
        }
    });
    std::fs::write(
        f.backup.join("milestones.json"),
        r#"[{"title": "1.0", "state": "closed"}]"#,
    )
    .unwrap();
    let api = Recorded::new(
        Vec::new(),
        vec![
            listed(&[]),
            listed(&[(1, "Bug"), (2, "Old")]),
            answer(200, json!([])),
            answer(200, json!([{"number": 4, "title": "1.0", "state": "open"}])),
        ],
    );
    let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(
        report,
        IssuesReport {
            issues: 2,
            comments: 1,
            closed: 1,
            milestones_closed: 1,
            ..IssuesReport::default()
        }
    );
    assert_eq!(
        patch_bodies(&api),
        vec![
            (
                format!("/repos/{NEW}/issues/2"),
                json!({"state": "closed", "state_reason": "not_planned"})
            ),
            (
                format!("/repos/{NEW}/milestones/4"),
                json!({"state": "closed"})
            ),
        ],
        "the open issue is not touched; the milestone is closed last"
    );

    // Again, with the repo as that left it: nothing is closed twice.
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(
                200,
                json!([{"number": 1, "title": "Bug", "state": "open", "body": "broken"},
                       {"number": 2, "title": "Old", "state": "closed", "body": ""}]),
            ),
            answer(
                200,
                json!([{"body": "me too",
                        "issue_url": format!("https://api.github.com/repos/{NEW}/issues/1")}]),
            ),
            answer(
                200,
                json!([{"number": 4, "title": "1.0", "state": "closed"}]),
            ),
        ],
    );
    let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(
        report,
        IssuesReport {
            issues_there: 2,
            comments_there: 1,
            ..IssuesReport::default()
        }
    );
    assert_eq!(patches(&api), Vec::<String>::new());
}

/// A closed milestone waits while one of the backup's issues is not there yet: the queue could
/// still put an issue on it.
#[test]
fn a_closed_milestone_waits_until_every_issue_is_there() {
    let f = fixture("milestone-waits");
    std::fs::write(
        f.backup.join("milestones.json"),
        r#"[{"title": "1.0", "state": "closed"}]"#,
    )
    .unwrap();
    std::fs::write(f.stub.join("ratelimit"), "").unwrap();
    let api = Recorded::new(
        Vec::new(),
        vec![
            listed(&[]),
            listed(&[]),
            answer(200, json!([])),
            answer(200, json!([{"number": 4, "title": "1.0", "state": "open"}])),
        ],
    );
    let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(
        report,
        IssuesReport {
            issues: 2,
            comments_waiting: 1,
            // The closed issue and the closed milestone.
            state_waiting: 2,
            ..IssuesReport::default()
        }
    );
    assert_eq!(patches(&api), Vec::<String>::new());
}

/// #365, owner decision 5: a `#N` in a restored comment or body names the issue's new number.
/// A number nothing is known about (a PR, another repo's issue) and one in a code span stay.
#[test]
fn old_issue_numbers_in_comments_and_bodies_get_the_new_numbers() {
    let issue = |n: u64, title: &str, body: &str, state: &str| {
        json!({"number": n, "title": title, "body": body, "state": state,
               "labels": [], "milestone": null})
    };
    let comment = |on: u64, body: &str| {
        json!({"body": body,
               "issue_url": format!("https://api.github.com/repos/owner/demo/issues/{on}")})
    };
    let first = "see #5 and `#5`, PR #4, other/repo#5";
    let answers = RecordedAnswers {
        repo: "owner/demo".to_string(),
        issues: vec![
            issue(5, "B", "after #1", "closed"),
            issue(1, "A", first, "open"),
        ],
        pulls: vec![json!({"number": 4, "state": "closed", "merged_at": null, "body": ""})],
        comments: vec![comment(1, "dup of #5"), comment(5, "see #1, #5 and #7")],
        ..RecordedAnswers::default()
    };
    let f = fixture_of("numbers", &answers);
    // The queue made the issues with the backup's bodies; it put its marker after the first.
    let made = format!("{first}\n\n<!-- ghq:0123456789ab -->\n");
    let api = Recorded::new(
        Vec::new(),
        vec![
            listed(&[]),
            answer(
                200,
                json!([{"number": 2, "title": "B", "state": "open", "body": "after #1"},
                       {"number": 1, "title": "A", "state": "open", "body": made}]),
            ),
            answer(200, json!([])),
        ],
    );
    let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    let comments: Vec<Value> = queued(&f)
        .into_iter()
        .filter(|item| item["kind"] == "comment")
        .collect();
    assert_eq!(
        comments,
        vec![
            json!({"kind": "comment", "repo": NEW, "number": 1, "body": "dup of #2",
                   "marker": null}),
            json!({"kind": "comment", "repo": NEW, "number": 2, "body": "see #1, #2 and #7",
                   "marker": null}),
        ]
    );
    // Issues are created with the backup's text (their numbers are not known yet); the body
    // is rewritten once they are there, the queue's marker kept.
    assert_eq!(
        patch_bodies(&api),
        vec![
            (
                format!("/repos/{NEW}/issues/1"),
                json!({"body": "see #2 and `#5`, PR #4, other/repo#5\n\n<!-- ghq:0123456789ab -->\n"})
            ),
            (format!("/repos/{NEW}/issues/2"), json!({"state": "closed"})),
        ]
    );
    assert_eq!(
        report,
        IssuesReport {
            issues: 2,
            comments: 2,
            closed: 1,
            renumbered: 1,
            ..IssuesReport::default()
        }
    );

    // Again, with the repo as that left it: the rewritten body no longer reads as the
    // backup's, so it is not rewritten twice, and the comments are found as they were queued.
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(
                200,
                json!([{"number": 2, "title": "B", "state": "closed", "body": "after #1"},
                       {"number": 1, "title": "A", "state": "open",
                        "body": "see #2 and `#5`, PR #4, other/repo#5\n\n<!-- ghq:0123456789ab -->\n"}]),
            ),
            answer(
                200,
                json!([{"body": "dup of #2",
                        "issue_url": format!("https://api.github.com/repos/{NEW}/issues/1")},
                       {"body": "see #1, #2 and #7",
                        "issue_url": format!("https://api.github.com/repos/{NEW}/issues/2")}]),
            ),
        ],
    );
    let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(
        report,
        IssuesReport {
            issues_there: 2,
            comments_there: 2,
            ..IssuesReport::default()
        }
    );
    assert_eq!(patches(&api), Vec::<String>::new());
    assert_eq!(queued(&f).len(), 4, "nothing more was queued");
}

/// #365, owner ruling of 2026-11-11: a full link and `owner/name#N` are rewritten too, only
/// when they name the repo being restored (without case), and then name the repo the restore
/// goes to. Another repo, another host, a code span and a number nothing maps stay.
#[test]
fn full_links_and_named_numbers_of_the_restored_repo_get_the_new_numbers() {
    let issue = |n: u64, title: &str, body: &str| {
        json!({"number": n, "title": title, "body": body, "state": "open",
               "labels": [], "milestone": null})
    };
    let first = "[#5](https://github.com/Owner/Demo/issues/5), owner/demo#5, \
                 https://github.com/other/repo/issues/5, other/repo#5, \
                 `owner/demo#5`, owner/demo#7, https://example.invalid/owner/demo/issues/5";
    let answers = RecordedAnswers {
        repo: "owner/demo".to_string(),
        issues: vec![issue(5, "B", "plain"), issue(1, "A", first)],
        comments: vec![
            json!({"body": "see https://github.com/owner/demo/pull/5 and OWNER/DEMO#5",
            "issue_url": "https://api.github.com/repos/owner/demo/issues/1"}),
        ],
        ..RecordedAnswers::default()
    };
    let f = fixture_of("links", &answers);
    let api = Recorded::new(
        Vec::new(),
        vec![
            listed(&[]),
            answer(
                200,
                json!([{"number": 2, "title": "B", "state": "open", "body": "plain"},
                       {"number": 1, "title": "A", "state": "open", "body": first}]),
            ),
            answer(200, json!([])),
        ],
    );
    let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    let comments: Vec<Value> = queued(&f)
        .into_iter()
        .filter(|item| item["kind"] == "comment")
        .collect();
    assert_eq!(
        comments,
        vec![
            json!({"kind": "comment", "repo": NEW, "number": 1, "marker": null,
            "body": format!("see https://github.com/{NEW}/pull/2 and {NEW}#2")})
        ]
    );
    let rewritten = format!(
        "[#2](https://github.com/{NEW}/issues/2), {NEW}#2, \
         https://github.com/other/repo/issues/5, other/repo#5, \
         `owner/demo#5`, owner/demo#7, https://example.invalid/owner/demo/issues/5"
    );
    assert_eq!(
        patch_bodies(&api),
        vec![(format!("/repos/{NEW}/issues/1"), json!({"body": rewritten}))]
    );
    assert_eq!(report.renumbered, 1);
}

/// A body someone changed in the repo is not the backup's any more: it is left as it is, and
/// the report names it.
#[test]
fn a_body_that_was_changed_in_the_repo_is_not_rewritten() {
    let issue = |n: u64, title: &str, body: &str| {
        json!({"number": n, "title": title, "body": body, "state": "open",
               "labels": [], "milestone": null})
    };
    let answers = RecordedAnswers {
        repo: "owner/demo".to_string(),
        issues: vec![issue(5, "B", ""), issue(1, "A", "see #5")],
        ..RecordedAnswers::default()
    };
    let f = fixture_of("edited", &answers);
    let api = Recorded::new(
        Vec::new(),
        vec![answer(
            200,
            json!([{"number": 9, "title": "B", "state": "open", "body": ""},
                   {"number": 8, "title": "A", "state": "open", "body": "see #5, as I said"}]),
        )],
    );
    let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(
        report,
        IssuesReport {
            issues_there: 2,
            left_out: vec![
                "1 issue bodies keep the backup's #N, not rewritten: #8 (changed in the repo \
                 since)"
                    .to_string()
            ],
            ..IssuesReport::default()
        }
    );
    assert_eq!(patches(&api), Vec::<String>::new());
    assert_eq!(queued(&f), Vec::<Value>::new());
}

/// GitHub answering a close with 403 or 429 stops the closes: the rest waits and the report
/// says so. Nothing is sent after the refused one.
#[test]
fn a_rate_limit_answer_to_a_close_stops_the_closes_and_the_rest_waits() {
    let issue = |n: u64| {
        json!({"number": n, "title": format!("issue {n}"), "body": "", "state": "closed",
               "labels": [], "milestone": null})
    };
    let answers = RecordedAnswers {
        repo: "owner/demo".to_string(),
        issues: vec![issue(2), issue(1)],
        milestones: vec![json!({"title": "1.0"})],
        ..RecordedAnswers::default()
    };
    let f = fixture_of("patch-limit", &answers);
    std::fs::write(
        f.backup.join("milestones.json"),
        r#"[{"title": "1.0", "state": "closed"}]"#,
    )
    .unwrap();
    for status in [403, 429] {
        let api = Recorded::new(
            Vec::new(),
            vec![
                listed(&[(1, "issue 1"), (2, "issue 2")]),
                answer(200, json!([{"number": 4, "title": "1.0", "state": "open"}])),
            ],
        );
        api.patch
            .lock()
            .unwrap()
            .push(answer(status, json!({"message": "rate limit"})));
        let report = restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
        assert_eq!(
            report,
            IssuesReport {
                issues_there: 2,
                state_waiting: 3,
                left_out: vec![
                    "GitHub answered a close or a body change with 403 or 429 (a rate limit, \
                     or the token may not write): the rest waits"
                        .to_string()
                ],
                ..IssuesReport::default()
            }
        );
        assert!(report.held());
        assert_eq!(
            patch_bodies(&api),
            vec![(format!("/repos/{NEW}/issues/1"), json!({"state": "closed"}))],
            "one request, then none"
        );
    }
    // Any other answer is an error, not a silent skip.
    let api = Recorded::new(Vec::new(), vec![listed(&[(1, "issue 1"), (2, "issue 2")])]);
    api.patch
        .lock()
        .unwrap()
        .push(answer(422, json!({"message": "Validation Failed"})));
    assert!(matches!(
        restore_issues(&f.fleet, &f.backup, NEW, &remote(&f, &api)),
        Err(CoreError::GitHub(_))
    ));
}

/// #347, owner decision 3: the second confirm of a restore that held comments back queues only
/// those. It pushes no ref (a commit pushed since the first confirm survives) and creates no
/// second issue, label, milestone or comment.
#[test]
fn a_second_confirm_only_queues_what_was_held_back_and_pushes_no_ref() {
    let f = fixture("second");
    std::fs::write(f.stub.join("ratelimit"), "").unwrap();
    let app = caller(Face::Gui, false);
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(200, json!([])),
            answer(200, json!([])),
            listed(&[]),
            listed(&[]),
            answer(200, json!([])),
        ],
    );
    let first =
        restore_over_live_from(&app, &f.fleet, &f.backup, NEW, true, &remote(&f, &api)).unwrap();
    assert_eq!(first.refs, 3);
    assert!(first.issues.held());
    assert_eq!(
        (first.issues.comments_waiting, first.issues.state_waiting),
        (1, 1)
    );
    let after_first = queued(&f);
    assert_eq!(after_first.len(), 5, "2 labels, 1 milestone, 2 issues");

    // Work goes on in the repo: a new commit on master, which the backup does not have.
    std::fs::write(f.source.join("c.txt"), "c\n").unwrap();
    git(&f.source, &["add", "c.txt"]);
    git(&f.source, &["commit", "-q", "-m", "third"]);
    git(
        &f.source,
        &["push", "-q", &f.target.to_string_lossy(), "master"],
    );
    let live_refs = refs_of(&f.target, "refs");
    let calls_before = std::fs::read_to_string(f.stub.join("calls.log")).unwrap();

    // The queue has made the issues; the second confirm.
    std::fs::remove_file(f.stub.join("ratelimit")).unwrap();
    made_as(&f, &[("Bug", 1), ("Old", 2), ("milestone:1.0", 1)]);
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            listed(&[(1, "Bug"), (2, "Old")]),
            answer(200, json!([])),
        ],
    );
    let second = restore_held_over_live_from(
        &app,
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &first.made,
        &remote(&f, &api),
    )
    .unwrap();
    assert_eq!(
        second,
        RestoreReport {
            issues: IssuesReport {
                issues_there: 2,
                comments: 1,
                closed: 1,
                ..IssuesReport::default()
            },
            made: second.made.clone(),
            ..RestoreReport::default()
        }
    );
    assert_eq!(
        numbers(&second.made),
        (
            vec![(1, Some(1)), (2, Some(2))],
            vec![("1.0".to_string(), Some(1))]
        )
    );
    assert_eq!(
        refs_of(&f.target, "refs"),
        live_refs,
        "no ref was pushed: the commit made since the first confirm is still there"
    );
    let mut expected = after_first.clone();
    expected.push(expected_issue_queue().remove(2));
    assert_eq!(queued(&f), expected, "only the held comment was queued");
    assert_eq!(patches(&api), closes_old(2));
    assert_eq!(
        *api.log.lock().unwrap(),
        vec![
            format!("GET /repos/{NEW}"),
            format!("GET /repos/{NEW}/issues?state=all&per_page=100&page=1"),
            format!("GET /repos/{NEW}/issues/comments?per_page=100&page=1"),
            closes_old(2).remove(0),
        ],
        "labels and milestones are not even read"
    );
    let calls = std::fs::read_to_string(f.stub.join("calls.log")).unwrap();
    assert!(calls.len() > calls_before.len());

    // A third confirm, with the repo as that left it, creates nothing at all.
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(
                200,
                json!([{"number": 1, "title": "Bug", "state": "open", "body": "broken"},
                       {"number": 2, "title": "Old", "state": "closed", "body": ""}]),
            ),
            answer(
                200,
                json!([{"body": "me too\n\n<!-- ghq:0123456789ab -->\n",
                        "issue_url": format!("https://api.github.com/repos/{NEW}/issues/1")}]),
            ),
        ],
    );
    let third = restore_held_over_live_from(
        &app,
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &second.made,
        &remote(&f, &api),
    )
    .unwrap();
    assert_eq!(third.issues.comments, 0);
    assert_eq!(third.issues.comments_there, 1);
    assert_eq!(queued(&f), expected, "no duplicate issue or comment");
    assert_eq!(patches(&api), Vec::<String>::new());
    assert_eq!(refs_of(&f.target, "refs"), live_refs);
}

/// The held pass never queues an issue, even one the repo lacks: that is the whole restore's.
#[test]
fn the_held_pass_queues_no_issue_the_repo_lacks() {
    let f = fixture("held-lacks");
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            listed(&[(1, "Bug")]),
            answer(200, json!([])),
        ],
    );
    let report = restore_held_over_live_from(
        &caller(Face::Gui, false),
        &f.fleet,
        &f.backup,
        NEW,
        true,
        &Made::default(),
        &remote(&f, &api),
    )
    .unwrap();
    assert_eq!(
        report.issues,
        IssuesReport {
            issues_there: 1,
            comments: 1,
            left_out: vec![
                "1 issues or milestones of the backup are not in the repo and the create queue \
                 did not say this restore made them, so nothing was changed for them: backup #2"
                    .to_string()
            ],
            ..IssuesReport::default()
        }
    );
    assert_eq!(queued(&f), expected_issue_queue()[2..]);
    assert!(refs_of(&f.target, "refs").is_empty());
}

/// The second confirm is guarded as the first: refused, it reads and writes nothing.
#[test]
fn a_refused_second_confirm_writes_nothing() {
    let f = fixture("held-refused");
    let api = Recorded::new(Vec::new(), Vec::new());
    for (who, confirmed) in [
        (caller(Face::Gui, false), false),
        (caller(Face::Cli, false), true),
        (caller(Face::Mcp, false), true),
        (caller(Face::Gui, true), true),
    ] {
        match restore_held_over_live_from(
            &who,
            &f.fleet,
            &f.backup,
            NEW,
            confirmed,
            &Made::default(),
            &remote(&f, &api),
        ) {
            Err(CoreError::Refused(why)) => assert!(why.contains(NEW), "{why}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert!(api.log.lock().unwrap().is_empty(), "no API call at all");
    assert!(!f.stub.join("calls.log").exists(), "no fleet call at all");
    assert!(refs_of(&f.target, "refs").is_empty());
}

/// #365: a label the live repo has with another colour is left as it is (an open decision) and
/// the report names it instead of passing over it.
#[test]
fn a_label_that_is_there_with_another_colour_is_left_and_named() {
    let f = fixture("colour");
    made_as(&f, &[("Bug", 1), ("Old", 2)]);
    std::fs::write(
        f.backup.join("labels.json"),
        r#"[{"name": "bug", "color": "d73a4a"}, {"name": "size:Easy", "color": "0E8A16"}]"#,
    )
    .unwrap();
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(
                200,
                json!([{"name": "Bug", "color": "ffffff"},
                       {"name": "size:Easy", "color": "0e8a16"}]),
            ),
            answer(200, json!([{"title": "1.0"}])),
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
    assert_eq!(report.labels, 0);
    assert_eq!(
        report.left_out,
        ["1 labels are there with another colour and were left as they are: bug"]
    );
    assert_eq!(queued(&f), expected_issue_queue(), "no label was queued");
    assert_eq!(patches(&api), closes_old(2), "and none was changed");
}

/// #365, owner ruling of 2026-11-11: a label the live repo already has keeps the live colour
/// and description. The restore queues no create for it, sends nothing that changes it, and
/// names it in the report.
#[test]
fn a_label_that_is_there_keeps_its_live_colour_and_description() {
    let f = fixture("colour-kept");
    made_as(&f, &[("Bug", 1), ("Old", 2)]);
    std::fs::write(
        f.backup.join("labels.json"),
        r#"[{"name": "bug", "color": "d73a4a", "description": "as backed up"},
            {"name": "size:Easy", "color": "0E8A16", "description": "as backed up"}]"#,
    )
    .unwrap();
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(200, json!({"private": true})),
            answer(
                200,
                json!([{"name": "Bug", "color": "ffffff", "description": "the live one"},
                       {"name": "size:Easy", "color": "0e8a16", "description": "the live one"}]),
            ),
            answer(200, json!([{"title": "1.0"}])),
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
    assert_eq!(report.labels, 0);
    assert_eq!(
        report.left_out,
        ["1 labels are there with another colour and were left as they are: bug"]
    );
    assert!(
        queued(&f).iter().all(|item| item["kind"] != "label"),
        "no label was queued"
    );
    let log = api.log.lock().unwrap().clone();
    let on_labels: Vec<&String> = log.iter().filter(|line| line.contains("/labels")).collect();
    assert!(
        on_labels.iter().all(|line| line.starts_with("GET ")),
        "labels are only read: {on_labels:?}"
    );
    assert_eq!(patches(&api), closes_old(2), "only the old issue is closed");
}
