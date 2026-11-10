//! BAK-3: `restore_history` puts a backup's history, labels and milestones into a new private
//! repo. Git pushes go to a local bare repo (standing in for `https://github.com`), the API is
//! recorded answers, and the create queue is a stub `fleet.py` that keeps its own counts.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::backup::{
    restore_history_from, write_json, FileSource, RecordedAnswers, Remote, RestoreReport,
};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::transport::{Recorded, RestAnswer};
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
/// `failed`, anything else is `done`.
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
fail_file = os.path.join(here, "fail")
failing = open(fail_file).read().split() if os.path.exists(fail_file) else []
counts = {"pending": 0, "done": 0, "exists": 0, "failed": 0}
for item in items:
    taken_repo = item["kind"] == "repo" and item["repo"] in taken
    state = "exists" if taken_repo else "failed" if item["kind"] in failing else "done"
    counts[state] += 1
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
            milestones: 1,
            ..RestoreReport::default()
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

#[test]
fn a_label_the_queue_fails_is_an_error_not_a_report() {
    let f = fixture("label-failed");
    std::fs::write(f.stub.join("fail"), "label").unwrap();
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(404, json!({})),
            answer(200, json!({"full_name": NEW, "private": true})),
        ],
    );
    match restore_history_from(&f.fleet, &f.backup, NEW, &remote(&f, &api)) {
        Err(CoreError::GitHub(why)) => assert!(why.contains("2 label or milestone"), "{why}"),
        other => panic!("expected a queue failure, got {other:?}"),
    }
}

/// Asserts a restore was refused with `needle` in the reason and nothing was touched: no API
/// call, no queue call (so no repo was created), nothing pushed.
fn refused_before_any_write(f: &Fixture, backup: &Path, needle: &str) {
    let api = Recorded::new(Vec::new(), Vec::new());
    match restore_history_from(&f.fleet, backup, NEW, &remote(f, &api)) {
        Err(CoreError::Parse(why)) => assert!(why.contains(needle), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(api.log.lock().unwrap().is_empty(), "no API call at all");
    assert!(!f.stub.join("calls.log").exists(), "no fleet call at all");
    assert!(refs_of(&f.target, "refs").is_empty());
}

fn set_manifest(backup: &Path, change: impl Fn(&mut Value)) {
    let path = backup.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    change(&mut manifest);
    std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
}

/// #364: a valid bundle that holds only `master`, where the manifest says 4 refs.
#[test]
fn a_bundle_with_fewer_refs_than_the_manifest_is_refused_before_a_repo_is_created() {
    let f = fixture("fewer-refs");
    let bundle = f.backup.join("repo.bundle");
    std::fs::remove_file(&bundle).unwrap();
    git(
        &f.source,
        &[
            "bundle",
            "create",
            "-q",
            &bundle.to_string_lossy(),
            "master",
        ],
    );
    refused_before_any_write(&f, &f.backup, "1 refs, the manifest says 4");
}

/// #364: a manifest edited to claim more refs than the bundle holds.
#[test]
fn a_manifest_that_claims_more_refs_than_the_bundle_is_refused() {
    let f = fixture("more-refs");
    set_manifest(&f.backup, |m| m["refs"] = json!(99));
    refused_before_any_write(&f, &f.backup, "4 refs, the manifest says 99");
}

/// #364: the bundle is not the file the manifest was written for.
#[test]
fn a_bundle_that_is_not_the_manifests_is_refused() {
    let f = fixture("hash");
    // A hash is 40 hex characters; this is not this bundle's.
    set_manifest(&f.backup, |m| m["bundle_hash"] = json!("0".repeat(40)));
    refused_before_any_write(&f, &f.backup, "repo.bundle is not the one the manifest");

    // With the bundle's own hash the same backup restores.
    let hash = git(&f.backup, &["hash-object", "--no-filters", "repo.bundle"]);
    set_manifest(&f.backup, |m| m["bundle_hash"] = json!(hash));
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(404, json!({"message": "Not Found"})),
            answer(200, json!({"full_name": NEW, "private": true})),
        ],
    );
    let report = restore_history_from(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(report.refs, 3);
}

/// #364: the folder is the backup of `owner/demo`, the manifest in it names another repo.
#[test]
fn a_manifest_for_another_repo_than_its_folder_is_refused() {
    let f = fixture("wrong-repo");
    let home = f.backup.parent().unwrap().join("Backups/github/owner/demo");
    std::fs::create_dir_all(home.parent().unwrap()).unwrap();
    std::fs::rename(&f.backup, &home).unwrap();
    set_manifest(&home, |m| m["repo"] = json!("other/zzz"));
    refused_before_any_write(&f, &home, "the manifest is for other/zzz");

    set_manifest(&home, |m| m["repo"] = json!("zzz"));
    refused_before_any_write(&f, &home, "not an owner/name repo");
}

/// #364: a copy of the backup kept anywhere (not in `github/<owner>/<name>`), its manifest edited
/// to name another repo: what the folder holds still says which repo it is the backup of.
#[test]
fn a_manifest_for_another_repo_than_the_backups_items_is_refused_wherever_it_sits() {
    let f = fixture("wrong-repo-copy");
    std::fs::write(
        f.backup.join("comments.json"),
        r#"[{"body": "me too",
             "issue_url": "https://api.github.com/repos/owner/demo/issues/1"}]"#,
    )
    .unwrap();
    set_manifest(&f.backup, |m| m["repo"] = json!("other/zzz"));
    // No `resolved_repo` in the manifest (hand-edited, or written before the manifest had the
    // field): refused, with both names and what to do if the repo was renamed before the backup.
    refused_before_any_write(
        &f,
        &f.backup,
        "the manifest is for other/zzz, the backup holds items of owner/demo; if the repo was \
         renamed or moved to owner/demo before this backup was taken, add \
         \"resolved_repo\": \"owner/demo\" to its manifest.json and restore again",
    );
    // A recorded rename covers that one name only.
    set_manifest(&f.backup, |m| m["resolved_repo"] = json!("third/repo"));
    refused_before_any_write(
        &f,
        &f.backup,
        "the manifest is for other/zzz (GitHub answered for third/repo), the backup holds \
         items of owner/demo",
    );
    set_manifest(&f.backup, |m| {
        m.as_object_mut().unwrap().remove("resolved_repo");
    });

    // With the manifest's own repo (any case) the same copy restores.
    set_manifest(&f.backup, |m| m["repo"] = json!("Owner/Demo"));
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(404, json!({"message": "Not Found"})),
            answer(200, json!({"full_name": NEW, "private": true})),
        ],
    );
    let report = restore_history_from(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(report.refs, 3);
}

/// #364: a repo renamed or moved before its backup, backed up under the old name. GitHub
/// answered with the items of the new name; the backup recorded it, so the restore goes on.
#[test]
fn a_backup_that_followed_a_rename_restores() {
    let f = fixture("renamed");
    let at = |rest: &str| format!("https://api.github.com/repos/new-owner/renamed/{rest}");
    let answers = RecordedAnswers {
        repo: "owner/demo".to_string(),
        labels: vec![
            json!({"name": "bug", "url": at("labels/bug")}),
            json!({"name": "size:Easy", "url": at("labels/size:Easy")}),
        ],
        milestones: vec![json!({"title": "1.0", "url": at("milestones/1")})],
        comments: vec![json!({"body": "me too", "issue_url": at("issues/1")})],
        ..RecordedAnswers::default()
    };
    let mut manifest = write_json(&answers, &f.backup, "2026-11-09T14:00:00Z").unwrap();
    assert_eq!(manifest.repo, "owner/demo");
    assert_eq!(manifest.resolved_repo.as_deref(), Some("new-owner/renamed"));
    manifest.refs = 4;
    std::fs::write(
        f.backup.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(404, json!({"message": "Not Found"})),
            answer(200, json!({"full_name": NEW, "private": true})),
        ],
    );
    let report = restore_history_from(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!((report.refs, report.labels, report.milestones), (3, 2, 1));

    // The same backup with the recorded name taken out of the manifest is refused.
    set_manifest(&f.backup, |m| {
        m.as_object_mut().unwrap().remove("resolved_repo");
    });
    match restore_history_from(&f.fleet, &f.backup, NEW, &remote(&f, &api)) {
        Err(CoreError::Parse(why)) => assert!(
            why.contains(
                "the manifest is for owner/demo, the backup holds items of new-owner/renamed"
            ),
            "{why}"
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Items of more than one other repo are no rename: the JSON is not written.
#[test]
fn answers_for_several_other_repos_are_not_written() {
    let root = scratch("several");
    let at = |repo: &str| format!("https://api.github.com/repos/{repo}/labels/x");
    let answers = RecordedAnswers {
        repo: "owner/demo".to_string(),
        labels: vec![
            json!({"name": "a", "url": at("one/repo")}),
            json!({"name": "b", "url": at("two/repo")}),
        ],
        ..RecordedAnswers::default()
    };
    let out = root.join("backup");
    match write_json(&answers, &out, "2026-11-09T14:00:00Z") {
        Err(CoreError::GitHub(why)) => assert!(
            why.contains("more than one other repo (one/repo, two/repo)"),
            "{why}"
        ),
        other => panic!("expected an error, got {other:?}"),
    }
    assert!(!out.join("labels.json").exists());
}

/// #365: a label comes back with its colour and description, a milestone with its description
/// and due date.
#[test]
fn labels_and_milestones_keep_their_colour_description_and_due_date() {
    let f = fixture("colours");
    std::fs::write(
        f.backup.join("labels.json"),
        r#"[{"name": "bug", "color": "d73a4a", "description": "Something is broken"},
            {"name": "size:Easy", "color": "", "description": null}]"#,
    )
    .unwrap();
    std::fs::write(
        f.backup.join("milestones.json"),
        r#"[{"title": "1.0", "state": "open", "description": "First release",
             "due_on": "2026-12-01T08:00:00Z"}]"#,
    )
    .unwrap();
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(404, json!({"message": "Not Found"})),
            answer(200, json!({"full_name": NEW, "private": true})),
        ],
    );
    restore_history_from(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(
        queued(&f)[1..],
        [
            json!({"kind": "label", "repo": NEW, "name": "bug", "color": "d73a4a",
                   "description": "Something is broken"}),
            json!({"kind": "label", "repo": NEW, "name": "size:Easy"}),
            json!({"kind": "milestone", "repo": NEW, "title": "1.0",
                   "description": "First release", "due": "2026-12-01"}),
        ]
    );
}

/// #365: what a restore cannot bring back is named in the report, not passed over in silence.
#[test]
fn what_is_not_restored_is_listed_in_the_report() {
    let f = fixture("left-out");
    set_manifest(&f.backup, |m| {
        m["wiki"] = json!(true);
        m["pulls"] = json!(2);
        m["releases"] = json!(1);
    });
    std::fs::write(
        f.backup.join("milestones.json"),
        r#"[{"title": "1.0", "state": "closed"}]"#,
    )
    .unwrap();
    let api = Recorded::new(
        Vec::new(),
        vec![
            answer(404, json!({"message": "Not Found"})),
            answer(200, json!({"full_name": NEW, "private": true})),
        ],
    );
    let report = restore_history_from(&f.fleet, &f.backup, NEW, &remote(&f, &api)).unwrap();
    assert_eq!(
        report.left_out,
        [
            "the wiki (wiki.git) is not restored",
            "2 PRs (pulls.json) are not restored",
            "1 releases (releases.json, releases/) are not restored; their tags are",
            "1 closed milestones come back open: the create queue cannot close one",
        ]
    );
}
