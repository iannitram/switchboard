//! BAK-1 and BAK-2: `backup_repo` and `backup_all`, against local bare repos made by `git` in a
//! temp dir (standing in for `https://github.com`) and recorded API answers.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::backup::{
    backup_all, backup_all_from, backup_repo, backup_repo_from, BackupOutcome, FileSource,
    Manifest, Remote,
};
use switchboard_core::github::transport::{Recorded, RestAnswer};
use switchboard_core::settings::Settings;
use switchboard_core::CoreError;

const REPO: &str = "owner/demo";

/// No test here has a release file to fetch.
struct NoFiles;

impl FileSource for NoFiles {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, CoreError> {
        Err(CoreError::GitHub(format!("GET {url}: not expected")))
    }
}

fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-bak1-run-{tag}-{}-{nanos}", std::process::id()));
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

/// A bare repo at `<server>/<repo>.git` with two commits on master, a `feature` branch and a
/// `v1` tag (3 refs), and when `wiki` is set a bare `<server>/<repo>.wiki.git`.
fn server(root: &Path, repo: &str, wiki: bool) -> PathBuf {
    let server = root.join("server");
    let work = root.join(format!("work-{}", repo.replace('/', "-")));
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "master"]);
    std::fs::write(work.join("a.txt"), "a\n").unwrap();
    git(&work, &["add", "a.txt"]);
    git(&work, &["commit", "-q", "-m", "first"]);
    git(&work, &["tag", "v1"]);
    git(&work, &["branch", "feature"]);
    std::fs::write(work.join("b.txt"), "b\n").unwrap();
    git(&work, &["add", "b.txt"]);
    git(&work, &["commit", "-q", "-m", "second"]);
    let bare = server.join(format!("{repo}.git"));
    std::fs::create_dir_all(bare.parent().unwrap()).unwrap();
    git(
        root,
        &[
            "clone",
            "-q",
            "--bare",
            &work.to_string_lossy(),
            &bare.to_string_lossy(),
        ],
    );
    if wiki {
        let wiki_work = root.join(format!("wiki-{}", repo.replace('/', "-")));
        std::fs::create_dir_all(&wiki_work).unwrap();
        git(&wiki_work, &["init", "-q", "-b", "master"]);
        std::fs::write(wiki_work.join("Home.md"), "home\n").unwrap();
        git(&wiki_work, &["add", "Home.md"]);
        git(&wiki_work, &["commit", "-q", "-m", "wiki first"]);
        let wiki_bare = server.join(format!("{repo}.wiki.git"));
        git(
            root,
            &[
                "clone",
                "-q",
                "--bare",
                &wiki_work.to_string_lossy(),
                &wiki_bare.to_string_lossy(),
            ],
        );
    }
    server
}

fn ok(body: Value) -> RestAnswer {
    RestAnswer {
        status: 200,
        etag: None,
        body,
    }
}

fn issue(n: u64, state: &str) -> Value {
    json!({"number": n, "state": state, "labels": [{"name": "bug"}]})
}

/// The answers for one backup, in the order it asks: issues (with one PR mixed in, as the REST
/// issues list has), pulls, comments, labels, milestones, releases (none).
fn answers(open: u64, closed: u64) -> Vec<RestAnswer> {
    let mut issues: Vec<Value> = (1..=open).map(|n| issue(n, "open")).collect();
    issues.extend((open + 1..=open + closed).map(|n| issue(n, "closed")));
    issues.push(json!({"number": 999, "state": "open", "labels": [], "pull_request": {}}));
    vec![
        ok(Value::Array(issues)),
        ok(json!([
            {"number": 999, "state": "open", "merged_at": null, "body": ""},
            {"number": 998, "state": "closed", "merged_at": "2026-11-01T10:00:00Z", "body": "Closes #1"}
        ])),
        ok(json!([{"id": 1, "body": "hi"}, {"id": 2, "body": "there"}])),
        ok(json!([{"name": "bug"}, {"name": "size:hard"}])),
        ok(json!([{"number": 1, "title": "1.0"}])),
        ok(json!([])),
    ]
}

fn settings(backups: &Path) -> Settings {
    Settings {
        nextcloud_backups: Some(backups.to_path_buf()),
        ..Settings::default()
    }
}

fn remote<'a>(server: &Path, api: &'a Recorded) -> Remote<'a> {
    Remote {
        git_base: server.to_string_lossy().into_owned(),
        git_config: Vec::new(),
        api,
        files: &NoFiles,
    }
}

fn folder(backups: &Path, repo: &str) -> PathBuf {
    let (owner, name) = repo.split_once('/').unwrap();
    backups.join("github").join(owner).join(name)
}

fn written(outcome: BackupOutcome) -> Manifest {
    match outcome {
        BackupOutcome::Written(m) => m,
        BackupOutcome::Refused(why) => panic!("refused: {why}"),
    }
}

fn read_manifest(dir: &Path) -> Manifest {
    serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap()
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_backup_writes_one_folder_with_a_verified_bundle_the_json_the_wiki_and_the_manifest() {
    let root = scratch("write");
    let server = server(&root, REPO, true);
    let backups = root.join("Backups");
    let api = Recorded::new(vec![], answers(3, 2));

    let manifest = written(
        backup_repo_from(
            &settings(&backups),
            REPO,
            &remote(&server, &api),
            "2026-11-09T16:45:00Z",
        )
        .unwrap(),
    );

    let dir = folder(&backups, REPO);
    assert_eq!(
        names(&dir),
        [
            "comments.json",
            "issues.json",
            "labels.json",
            "manifest.json",
            "milestones.json",
            "pulls.json",
            "releases",
            "releases.json",
            "repo.bundle",
            "wiki.git"
        ]
    );
    assert_eq!(manifest.repo, REPO);
    assert_eq!(manifest.taken_at, "2026-11-09T16:45:00Z");
    assert_eq!(manifest.refs, 3);
    assert_eq!(
        manifest.issues, 5,
        "the PR in the issues list is not an issue"
    );
    assert_eq!(manifest.open_issues, 3);
    assert_eq!(manifest.pulls, 2);
    assert_eq!(manifest.merged_pulls, 1);
    assert_eq!(manifest.comments, 2);
    assert_eq!(manifest.labels, 2);
    assert_eq!(manifest.milestones, 1);
    assert!(manifest.wiki);
    assert_eq!(read_manifest(&dir), manifest);

    let bundle = dir.join("repo.bundle");
    git(
        &server.join(format!("{REPO}.git")),
        &["bundle", "verify", "-q", &bundle.to_string_lossy()],
    );
    let issues: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(dir.join("issues.json")).unwrap()).unwrap();
    assert!(issues.iter().all(|i| i.get("pull_request").is_none()));

    let log = api.log.lock().unwrap().clone();
    assert_eq!(
        log,
        [
            "GET /repos/owner/demo/issues?state=all&per_page=100&page=1",
            "GET /repos/owner/demo/pulls?state=all&per_page=100&page=1",
            "GET /repos/owner/demo/issues/comments?per_page=100&page=1",
            "GET /repos/owner/demo/labels?per_page=100&page=1",
            "GET /repos/owner/demo/milestones?state=all&per_page=100&page=1",
            "GET /repos/owner/demo/releases?per_page=100&page=1",
        ]
    );
    assert_eq!(names(&backups.join("github").join("owner")), ["demo"]);
}

#[test]
fn a_second_backup_replaces_the_last() {
    let root = scratch("replace");
    let server = server(&root, REPO, true);
    let backups = root.join("Backups");
    let api = Recorded::new(vec![], answers(3, 2));
    written(
        backup_repo_from(
            &settings(&backups),
            REPO,
            &remote(&server, &api),
            "2026-11-02T06:00:00Z",
        )
        .unwrap(),
    );
    let dir = folder(&backups, REPO);
    std::fs::write(dir.join("stray.txt"), "from the last backup").unwrap();

    let api = Recorded::new(vec![], answers(4, 2));
    let manifest = written(
        backup_repo_from(
            &settings(&backups),
            REPO,
            &remote(&server, &api),
            "2026-11-09T06:00:00Z",
        )
        .unwrap(),
    );

    assert_eq!(manifest.issues, 6);
    assert_eq!(read_manifest(&dir).taken_at, "2026-11-09T06:00:00Z");
    assert!(
        !dir.join("stray.txt").exists(),
        "the last folder is replaced"
    );
    assert_eq!(names(&backups.join("github").join("owner")), ["demo"]);
}

#[test]
fn a_refused_backup_keeps_the_old_folder_untouched_and_returns_the_reason() {
    let root = scratch("refuse");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    let api = Recorded::new(vec![], answers(40, 2));
    written(
        backup_repo_from(
            &settings(&backups),
            REPO,
            &remote(&server, &api),
            "2026-11-02T06:00:00Z",
        )
        .unwrap(),
    );
    let dir = folder(&backups, REPO);
    let before_manifest = std::fs::read(dir.join("manifest.json")).unwrap();
    let before_issues = std::fs::read(dir.join("issues.json")).unwrap();
    let before_bundle = std::fs::read(dir.join("repo.bundle")).unwrap();

    // 40 open issues become 10, all 30 lost without a merged PR closing them.
    let api = Recorded::new(vec![], answers(10, 2));
    let outcome = backup_repo_from(
        &settings(&backups),
        REPO,
        &remote(&server, &api),
        "2026-11-09T06:00:00Z",
    )
    .unwrap();

    let BackupOutcome::Refused(reason) = outcome else {
        panic!("expected a refusal, got {outcome:?}");
    };
    assert!(reason.contains("the last had"), "{reason}");
    assert_eq!(
        std::fs::read(dir.join("manifest.json")).unwrap(),
        before_manifest
    );
    assert_eq!(
        std::fs::read(dir.join("issues.json")).unwrap(),
        before_issues
    );
    assert_eq!(
        std::fs::read(dir.join("repo.bundle")).unwrap(),
        before_bundle
    );
    assert_eq!(names(&backups.join("github").join("owner")), ["demo"]);
}

#[test]
fn a_repo_with_no_wiki_has_no_wiki_copy() {
    let root = scratch("nowiki");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    let api = Recorded::new(vec![], answers(1, 0));

    let manifest = written(
        backup_repo_from(
            &settings(&backups),
            REPO,
            &remote(&server, &api),
            "2026-11-09T06:00:00Z",
        )
        .unwrap(),
    );

    assert!(!manifest.wiki);
    assert!(!folder(&backups, REPO).join("wiki.git").exists());
}

#[test]
fn a_full_page_asks_for_the_next() {
    let root = scratch("pages");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    let mut rest = answers(0, 0);
    let first: Vec<Value> = (1..=100).map(|n| issue(n, "open")).collect();
    rest[0] = ok(Value::Array(first));
    rest.insert(1, ok(json!([issue(101, "open")])));
    let api = Recorded::new(vec![], rest);

    let manifest = written(
        backup_repo_from(
            &settings(&backups),
            REPO,
            &remote(&server, &api),
            "2026-11-09T06:00:00Z",
        )
        .unwrap(),
    );

    assert_eq!(manifest.issues, 101);
    assert_eq!(
        api.log.lock().unwrap()[1],
        "GET /repos/owner/demo/issues?state=all&per_page=100&page=2"
    );
}

#[test]
fn a_failed_read_leaves_the_old_folder_and_no_new_one() {
    let root = scratch("fail");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    let api = Recorded::new(vec![], answers(3, 0));
    written(
        backup_repo_from(
            &settings(&backups),
            REPO,
            &remote(&server, &api),
            "2026-11-02T06:00:00Z",
        )
        .unwrap(),
    );
    let dir = folder(&backups, REPO);
    let before = std::fs::read(dir.join("manifest.json")).unwrap();

    let mut rest = answers(3, 0);
    rest[1] = RestAnswer {
        status: 502,
        etag: None,
        body: Value::Null,
    };
    let api = Recorded::new(vec![], rest);
    let err = backup_repo_from(
        &settings(&backups),
        REPO,
        &remote(&server, &api),
        "2026-11-09T06:00:00Z",
    )
    .unwrap_err();

    assert!(
        matches!(err, CoreError::GitHub(ref why) if why.contains("502")),
        "{err:?}"
    );
    assert_eq!(std::fs::read(dir.join("manifest.json")).unwrap(), before);
    assert_eq!(names(&backups.join("github").join("owner")), ["demo"]);
}

#[test]
fn a_missing_repo_is_an_error_and_writes_nothing() {
    let root = scratch("norepo");
    let server = root.join("server");
    std::fs::create_dir_all(&server).unwrap();
    let backups = root.join("Backups");
    let api = Recorded::new(vec![], answers(1, 0));

    let err = backup_repo_from(
        &settings(&backups),
        REPO,
        &remote(&server, &api),
        "2026-11-09T06:00:00Z",
    )
    .unwrap_err();

    assert!(matches!(err, CoreError::Io(_)), "{err:?}");
    assert!(!folder(&backups, REPO).exists());
}

#[test]
fn backup_all_backs_up_each_repo_in_order() {
    let root = scratch("all");
    server(&root, "owner/two", false);
    let server = server(&root, "owner/one", true);
    let backups = root.join("Backups");
    let mut rest = answers(2, 0);
    rest.extend(answers(5, 1));
    let api = Recorded::new(vec![], rest);
    let repos = vec!["owner/one".to_string(), "owner/two".to_string()];

    let outcomes = backup_all_from(
        &settings(&backups),
        &repos,
        &remote(&server, &api),
        "2026-11-09T06:00:00Z",
    )
    .unwrap();

    let got: Vec<(String, u64, bool)> = outcomes
        .into_iter()
        .map(|(repo, outcome)| {
            let m = written(outcome.unwrap());
            (repo, m.issues, m.wiki)
        })
        .collect();
    assert_eq!(
        got,
        [
            ("owner/one".to_string(), 2, true),
            ("owner/two".to_string(), 6, false)
        ]
    );
    assert_eq!(names(&backups.join("github").join("owner")), ["one", "two"]);
}

#[test]
fn an_unset_backups_folder_is_an_error_naming_the_setting() {
    for unset in [Settings::default(), settings(Path::new(""))] {
        assert_eq!(
            backup_repo(&unset, REPO).unwrap_err(),
            CoreError::MissingPath("nextcloud_backups")
        );
        assert_eq!(
            backup_all(&unset, &[REPO.to_string()]).unwrap_err(),
            CoreError::MissingPath("nextcloud_backups")
        );
        let api = Recorded::new(vec![], vec![]);
        let err = backup_repo_from(
            &unset,
            REPO,
            &remote(Path::new("nowhere"), &api),
            "2026-11-09T06:00:00Z",
        )
        .unwrap_err();
        assert_eq!(err, CoreError::MissingPath("nextcloud_backups"));
        assert_eq!(err.to_string(), "setting not set: nextcloud_backups");
        assert!(api.log.lock().unwrap().is_empty(), "nothing is read");
    }
}

#[test]
fn a_repo_that_is_not_owner_slash_name_is_refused_before_any_read() {
    let root = scratch("badname");
    let backups = root.join("Backups");
    let api = Recorded::new(vec![], vec![]);
    for bad in ["demo", "../demo", "owner/../x", "owner/demo/extra"] {
        let err = backup_repo_from(
            &settings(&backups),
            bad,
            &remote(&root, &api),
            "2026-11-09T06:00:00Z",
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::Parse(_)), "{bad}: {err:?}");
    }
    assert!(api.log.lock().unwrap().is_empty());
    assert!(!backups.exists());
}
