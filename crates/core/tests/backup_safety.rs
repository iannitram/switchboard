//! BAK-1 and BAK-2 data safety: what must never happen to the last good backup. Two backups of
//! one repo at once, a wiki that could not be read, releases, one failing repo among several,
//! and a last backup whose manifest is damaged or missing. Local bare repos made by `git` stand
//! in for `https://github.com`; the API is a stub that answers by path, so it can be asked from
//! several threads.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use switchboard_core::backup::{
    backup_all_from, backup_repo_from, BackupOutcome, FileSource, HttpFiles, Manifest, Remote,
};
use switchboard_core::backup_run::{failed_row, BackupRow};
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::settings::Settings;
use switchboard_core::CoreError;

const REPO: &str = "owner/demo";
const TAKEN_AT: &str = "2026-11-09T16:45:00Z";

/// Answers every repo's lists the same way, by the list's name; logs each request.
#[derive(Default)]
struct ByPath {
    releases: Vec<Value>,
    log: Mutex<Vec<String>>,
}

impl GitHubTransport for ByPath {
    fn graphql(&self, _query: &str, _variables: &Value) -> Result<Value, CoreError> {
        Err(CoreError::GitHub("graphql: not expected".to_string()))
    }

    fn rest_get(&self, path: &str, _etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        self.log.lock().unwrap().push(format!("GET {path}"));
        let list = path.split('?').next().unwrap().rsplit('/').next().unwrap();
        let body = match list {
            "issues" => json!([
                {"number": 1, "state": "open", "labels": [{"name": "bug"}]},
                {"number": 2, "state": "open", "labels": []},
            ]),
            "pulls" => json!([]),
            "comments" => json!([{"id": 1, "body": "hi"}]),
            "labels" => json!([{"name": "bug"}]),
            "milestones" => json!([{"number": 1, "title": "1.0"}]),
            "releases" => Value::Array(self.releases.clone()),
            other => return Err(CoreError::GitHub(format!("GET {path}: no list {other}"))),
        };
        Ok(RestAnswer {
            status: 200,
            etag: None,
            body,
        })
    }
}

/// Release files by address; logs each fetch.
#[derive(Default)]
struct Files {
    by_url: HashMap<String, Vec<u8>>,
    log: Mutex<Vec<String>>,
}

impl FileSource for Files {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, CoreError> {
        self.log.lock().unwrap().push(url.to_string());
        self.by_url
            .get(url)
            .cloned()
            .ok_or_else(|| CoreError::GitHub(format!("GET {url}: not expected")))
    }
}

fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-bak-safe-{tag}-{}-{nanos}", std::process::id()));
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

/// A bare repo at `<root>/server/<name>.git` with one commit holding `file`.
fn bare(root: &Path, name: &str, file: &str) -> PathBuf {
    let work = root.join(format!("work-{}", name.replace('/', "-")));
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "master"]);
    std::fs::write(work.join(file), "text\n").unwrap();
    git(&work, &["add", file]);
    git(&work, &["commit", "-q", "-m", "first"]);
    let bare = root.join("server").join(format!("{name}.git"));
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
    bare
}

/// `<root>/server` with the repo `repo` (one ref) and, when `wiki` is set, its wiki.
fn server(root: &Path, repo: &str, wiki: bool) -> PathBuf {
    bare(root, repo, "a.txt");
    if wiki {
        bare(root, &format!("{repo}.wiki"), "Home.md");
    }
    root.join("server")
}

fn settings(backups: &Path) -> Settings {
    Settings {
        nextcloud_backups: Some(backups.to_path_buf()),
        ..Settings::default()
    }
}

fn remote<'a>(server: &Path, api: &'a ByPath, files: &'a Files) -> Remote<'a> {
    Remote {
        git_base: server.to_string_lossy().into_owned(),
        git_config: Vec::new(),
        api,
        files,
    }
}

fn folder(backups: &Path, repo: &str) -> PathBuf {
    let (owner, name) = repo.split_once('/').unwrap();
    backups.join("github").join(owner).join(name)
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

/// Every file under `dir` as `(path from dir, bytes)`, sorted: what "untouched" compares.
fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                let name = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.push((name, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// What a whole backup of a repo with a wiki holds.
const WHOLE: [&str; 10] = [
    "comments.json",
    "issues.json",
    "labels.json",
    "manifest.json",
    "milestones.json",
    "pulls.json",
    "releases",
    "releases.json",
    "repo.bundle",
    "wiki.git",
];

/// One backup of `REPO`, which must be written.
fn back_up(backups: &Path, server: &Path, api: &ByPath) -> Manifest {
    let files = Files::default();
    match backup_repo_from(
        &settings(backups),
        REPO,
        &remote(server, api, &files),
        TAKEN_AT,
    ) {
        Ok(BackupOutcome::Written(manifest)) => manifest,
        other => panic!("expected a written backup, got {other:?}"),
    }
}

/// #363: backups of one repo started at the same moment (the weekly run and a click, say). Each
/// must come back written, and the folder left behind must be one whole backup: every file, a
/// bundle git verifies, and nothing else beside it.
#[test]
fn backups_of_one_repo_at_once_each_write_a_whole_backup() {
    let root = scratch("at-once");
    let server = server(&root, REPO, true);
    let backups = root.join("Backups");
    let api = ByPath::default();
    let dir = folder(&backups, REPO);

    for round in 0..3 {
        let start = std::sync::Barrier::new(4);
        let outcomes: Vec<Result<BackupOutcome, CoreError>> = std::thread::scope(|scope| {
            let runs: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        let files = Files::default();
                        start.wait();
                        backup_repo_from(
                            &settings(&backups),
                            REPO,
                            &remote(&server, &api, &files),
                            TAKEN_AT,
                        )
                    })
                })
                .collect();
            runs.into_iter().map(|run| run.join().unwrap()).collect()
        });

        for outcome in &outcomes {
            assert!(
                matches!(outcome, Ok(BackupOutcome::Written(_))),
                "round {round}: {outcome:?}"
            );
        }
        assert_eq!(names(&dir), WHOLE, "round {round}");
        let manifest = read_manifest(&dir);
        assert_eq!(
            (manifest.refs, manifest.issues, manifest.wiki),
            (1, 2, true)
        );
        let bundle = dir.join("repo.bundle");
        git(
            &server.join(format!("{REPO}.git")),
            &["bundle", "verify", "-q", &bundle.to_string_lossy()],
        );
        git(
            &dir.join("wiki.git"),
            &["rev-parse", "--verify", "-q", "master"],
        );
        assert_eq!(
            names(dir.parent().unwrap()),
            ["demo"],
            "round {round}: no staging or set-aside folder is left"
        );
    }
}

/// The folder the parent of `two_processes_child` hands it.
const CHILD_ROOT: &str = "SWITCHBOARD_TEST_BACKUP_CHILD_ROOT";

/// One backup in a process of its own, for the test below. Does nothing in a normal test run:
/// only the parent sets `CHILD_ROOT`. Waits for `<root>/go` so every child starts together.
#[test]
fn two_processes_child() {
    let Ok(root) = std::env::var(CHILD_ROOT) else {
        return;
    };
    let root = PathBuf::from(root);
    while !root.join("go").exists() {
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
    let api = ByPath::default();
    let files = Files::default();
    let outcome = backup_repo_from(
        &settings(&root.join("Backups")),
        REPO,
        &remote(&root.join("server"), &api, &files),
        TAKEN_AT,
    );
    match outcome {
        Ok(BackupOutcome::Written(_)) => println!("child outcome: written"),
        Ok(BackupOutcome::Refused(why)) => println!("child outcome: refused {why}"),
        Err(e) => println!("child outcome: error {e}"),
    }
}

/// #363: backups of one repo started at the same moment in separate processes (the app's weekly
/// run and `switchboard backup`), which the lock inside one process does not keep apart. A
/// process that loses the race for the folder may come back with an error; what must hold is
/// that at least one is written and the folder left behind is one whole backup, with the bundle
/// its manifest was written for and no staging or set-aside folder beside it.
#[test]
fn backups_of_one_repo_from_several_processes_leave_one_whole_backup() {
    let root = scratch("processes");
    let server = server(&root, REPO, true);
    let backups = root.join("Backups");
    back_up(&backups, &server, &ByPath::default());
    let dir = folder(&backups, REPO);
    let go = root.join("go");

    for round in 0..5 {
        let _ = std::fs::remove_file(&go);
        let children: Vec<_> = (0..4)
            .map(|_| {
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "two_processes_child", "--nocapture"])
                    .env(CHILD_ROOT, &root)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        std::thread::sleep(std::time::Duration::from_millis(200));
        std::fs::write(&go, "").unwrap();
        let said: Vec<String> = children
            .into_iter()
            .map(|child| {
                let out = child.wait_with_output().unwrap();
                String::from_utf8_lossy(&out.stdout).into_owned()
            })
            .collect();

        for text in &said {
            assert!(
                text.contains("child outcome: written") || text.contains("child outcome: error"),
                "round {round}: {text}"
            );
        }
        assert!(
            said.iter().any(|t| t.contains("child outcome: written")),
            "round {round}: no backup was written: {said:?}"
        );
        assert_eq!(names(&dir), WHOLE, "round {round}");
        let manifest = read_manifest(&dir);
        assert_eq!(
            (manifest.refs, manifest.issues, manifest.wiki),
            (1, 2, true)
        );
        assert_eq!(
            manifest.bundle_hash,
            git(&dir, &["hash-object", "--no-filters", "repo.bundle"]),
            "round {round}: the bundle is the manifest's"
        );
        let bundle = dir.join("repo.bundle");
        git(
            &server.join(format!("{REPO}.git")),
            &["bundle", "verify", "-q", &bundle.to_string_lossy()],
        );
        git(
            &dir.join("wiki.git"),
            &["rev-parse", "--verify", "-q", "master"],
        );
        assert_eq!(
            names(dir.parent().unwrap()),
            ["demo"],
            "round {round}: no staging or set-aside folder is left"
        );
    }
}

/// #364: the manifest holds the hash of the bundle it was written for.
#[test]
fn the_manifest_holds_the_bundles_hash() {
    let root = scratch("hash");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    let manifest = back_up(&backups, &server, &ByPath::default());
    let dir = folder(&backups, REPO);
    assert_eq!(
        manifest.bundle_hash,
        git(&dir, &["hash-object", "--no-filters", "repo.bundle"])
    );
    assert_eq!(manifest.bundle_hash.len(), 40);
    assert_eq!(read_manifest(&dir), manifest);
}

/// #367: the wiki is there but cannot be read (no permission, as a dropped connection would
/// be): the backup is not written with "no wiki", and the last backup keeps its wiki copy.
#[cfg(unix)]
#[test]
fn a_wiki_that_cannot_be_read_never_replaces_a_backup_that_has_one() {
    use std::os::unix::fs::PermissionsExt;
    let root = scratch("wiki-unreadable");
    let server = server(&root, REPO, true);
    let backups = root.join("Backups");
    let api = ByPath::default();
    assert!(back_up(&backups, &server, &api).wiki);
    let dir = folder(&backups, REPO);
    let before = tree(&dir);

    let wiki = server.join(format!("{REPO}.wiki.git"));
    let lock = |mode: u32| {
        std::fs::set_permissions(&wiki, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    lock(0o000);
    if std::fs::read_dir(&wiki).is_ok() {
        lock(0o755);
        return; // Run as root: the folder cannot be made unreadable.
    }
    let files = Files::default();
    let outcome = backup_repo_from(
        &settings(&backups),
        REPO,
        &remote(&server, &api, &files),
        TAKEN_AT,
    );
    lock(0o755);

    match outcome {
        Ok(BackupOutcome::Refused(why)) => assert!(why.contains("wiki"), "{why}"),
        Err(e) => assert!(e.to_string().contains("wiki"), "{e}"),
        Ok(BackupOutcome::Written(m)) => panic!("written with wiki: {}", m.wiki),
    }
    assert_eq!(tree(&dir), before, "the last backup is untouched");
    assert!(dir.join("wiki.git").is_dir());
    assert_eq!(names(dir.parent().unwrap()), ["demo"]);
}

/// #367: a wiki whose clone fails for another reason than "no such repo" is an error, even with
/// no last backup to compare with: nothing is written that says "no wiki".
#[test]
fn a_wiki_clone_that_fails_is_an_error_not_no_wiki() {
    let root = scratch("wiki-broken");
    let server = server(&root, REPO, true);
    let backups = root.join("Backups");
    // The wiki repo is there, but git cannot read its refs.
    let refs = server.join(format!("{REPO}.wiki.git")).join("packed-refs");
    std::fs::write(refs, "garbage\n").unwrap();

    let api = ByPath::default();
    let files = Files::default();
    let outcome = backup_repo_from(
        &settings(&backups),
        REPO,
        &remote(&server, &api, &files),
        TAKEN_AT,
    );
    assert!(matches!(outcome, Err(CoreError::Io(_))), "{outcome:?}");
    assert!(!folder(&backups, REPO).exists(), "nothing is written");
    assert!(names(&backups.join("github").join("owner")).is_empty());
}

/// A repo with no wiki at all is still backed up, with `wiki: false`.
#[test]
fn a_repo_with_no_wiki_is_written_without_one() {
    let root = scratch("no-wiki");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    let manifest = back_up(&backups, &server, &ByPath::default());
    assert!(!manifest.wiki);
    assert!(!folder(&backups, REPO).join("wiki.git").exists());
}

/// #367: a wiki that was really removed. The refusal says what to do, and doing it (moving the
/// last backup away) lets the next backup be written, without a wiki.
#[test]
fn a_wiki_removed_on_purpose_has_a_stated_way_forward() {
    let root = scratch("wiki-removed");
    let server = server(&root, REPO, true);
    let backups = root.join("Backups");
    let api = ByPath::default();
    assert!(back_up(&backups, &server, &api).wiki);
    let dir = folder(&backups, REPO);
    let before = tree(&dir);

    std::fs::remove_dir_all(server.join(format!("{REPO}.wiki.git"))).unwrap();
    let files = Files::default();
    let outcome = backup_repo_from(
        &settings(&backups),
        REPO,
        &remote(&server, &api, &files),
        TAKEN_AT,
    );
    match outcome {
        Ok(BackupOutcome::Refused(why)) => {
            assert!(
                why.starts_with("the new backup has no wiki, the last had one"),
                "{why}"
            );
            assert!(why.contains("move the last backup's folder away"), "{why}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(tree(&dir), before, "the last backup is untouched");

    std::fs::rename(&dir, root.join("demo-with-wiki")).unwrap();
    assert!(!back_up(&backups, &server, &api).wiki);
    assert!(!dir.join("wiki.git").exists());
}

/// #368: a backup holds the repo's releases: the list, and each file under its tag.
#[test]
fn a_backup_holds_the_releases_and_their_files() {
    let root = scratch("releases");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    let asset = "https://api.github.com/repos/owner/demo/releases/assets/7";
    let api = ByPath {
        releases: vec![
            json!({"tag_name": "v1.0", "name": "One", "assets": [{
                "name": "app.zip", "size": 9, "url": asset,
                "browser_download_url": "https://github.com/owner/demo/releases/download/v1.0/app.zip",
            }]}),
            json!({"tag_name": "v0.9", "name": "Before", "assets": []}),
        ],
        ..ByPath::default()
    };
    let files = Files {
        by_url: HashMap::from([(asset.to_string(), b"zip bytes".to_vec())]),
        ..Files::default()
    };

    let outcome = backup_repo_from(
        &settings(&backups),
        REPO,
        &remote(&server, &api, &files),
        TAKEN_AT,
    );
    let Ok(BackupOutcome::Written(manifest)) = outcome else {
        panic!("expected a written backup, got {outcome:?}");
    };

    let dir = folder(&backups, REPO);
    assert_eq!(manifest.releases, 2);
    assert_eq!(read_manifest(&dir).releases, 2);
    let list: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(dir.join("releases.json")).unwrap()).unwrap();
    let tags: Vec<&str> = list
        .iter()
        .map(|r| r["tag_name"].as_str().unwrap())
        .collect();
    assert_eq!(tags, ["v1.0", "v0.9"]);
    assert_eq!(
        std::fs::read(dir.join("releases").join("v1.0").join("app.zip")).unwrap(),
        b"zip bytes"
    );
    assert_eq!(*files.log.lock().unwrap(), [asset]);
    assert!(api
        .log
        .lock()
        .unwrap()
        .contains(&"GET /repos/owner/demo/releases?per_page=100&page=1".to_string()));
}

/// #368: a release file that cannot be fetched fails the backup; the last one stays.
#[test]
fn a_release_file_that_cannot_be_fetched_keeps_the_last_backup() {
    let root = scratch("release-fails");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    back_up(&backups, &server, &ByPath::default());
    let dir = folder(&backups, REPO);
    let before = tree(&dir);

    let api = ByPath {
        releases: vec![json!({"tag_name": "v1.0", "assets": [{
            "name": "app.zip", "size": 9, "url": "https://api.github.com/gone",
        }]})],
        ..ByPath::default()
    };
    let files = Files::default();
    let outcome = backup_repo_from(
        &settings(&backups),
        REPO,
        &remote(&server, &api, &files),
        TAKEN_AT,
    );
    assert!(matches!(outcome, Err(CoreError::GitHub(_))), "{outcome:?}");
    assert_eq!(tree(&dir), before);
    assert_eq!(names(dir.parent().unwrap()), ["demo"]);
}

/// #368: a release file that came short of the size the release gives (a download cut off) is
/// not kept as if it were whole: the backup fails and the last one stays.
#[test]
fn a_release_file_cut_short_keeps_the_last_backup() {
    let root = scratch("release-short");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    back_up(&backups, &server, &ByPath::default());
    let dir = folder(&backups, REPO);
    let before = tree(&dir);

    let asset = "https://api.github.com/repos/owner/demo/releases/assets/7";
    let api = ByPath {
        releases: vec![json!({"tag_name": "v1.0", "assets": [{
            "name": "app.zip", "size": 9, "url": asset,
        }]})],
        ..ByPath::default()
    };
    let files = Files {
        by_url: HashMap::from([(asset.to_string(), b"zip".to_vec())]),
        ..Files::default()
    };
    let outcome = backup_repo_from(
        &settings(&backups),
        REPO,
        &remote(&server, &api, &files),
        TAKEN_AT,
    );
    match outcome {
        Err(CoreError::GitHub(why)) => {
            assert!(why.contains("came as 3 bytes, the release says 9"), "{why}");
        }
        other => panic!("expected an error, got {other:?}"),
    }
    assert_eq!(tree(&dir), before);
    assert_eq!(names(dir.parent().unwrap()), ["demo"]);
}

struct Token;

impl switchboard_core::platform::TokenStore for Token {
    fn get(&self, _service: &str) -> Option<String> {
        Some("test-token-not-real".to_string())
    }
}

/// A local HTTP server for `connections` requests, one after the other: `/app.zip` is nine
/// bytes, `/short.zip` says nine and sends three, anything else is 404. Returns its address and
/// a handle that gives every request's text.
fn http_server(connections: usize) -> (String, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..connections {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                request.push(byte[0]);
            }
            let request = String::from_utf8_lossy(&request).into_owned();
            let answer: &[u8] = if request.starts_with("GET /app.zip ") {
                b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nzip bytes"
            } else if request.starts_with("GET /short.zip ") {
                b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nzip"
            } else {
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            };
            let _ = stream.write_all(answer);
            requests.push(request);
        }
        requests
    });
    (base, handle)
}

/// #368: `HttpFiles`, the file source the app uses, against a local server: a file comes back
/// whole, a 404 and a download cut short are errors, and the GitHub token is not sent to a host
/// that is not GitHub's.
#[test]
fn http_files_fetches_a_file_and_fails_on_a_missing_or_short_one() {
    let (base, server) = http_server(3);
    let files = HttpFiles::new(&Token).unwrap();

    assert_eq!(
        files.fetch(&format!("{base}/app.zip")).unwrap(),
        b"zip bytes"
    );
    let gone = files.fetch(&format!("{base}/gone.zip"));
    assert!(matches!(gone, Err(CoreError::GitHub(_))), "{gone:?}");
    let short = files.fetch(&format!("{base}/short.zip"));
    assert!(matches!(short, Err(CoreError::GitHub(_))), "{short:?}");

    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    for request in &requests {
        let lower = request.to_ascii_lowercase();
        assert!(!lower.contains("authorization"), "{request}");
        assert!(!lower.contains("test-token-not-real"), "{request}");
        assert!(
            lower.contains("accept: application/octet-stream"),
            "{request}"
        );
    }
}

/// #369: the repo in the middle cannot be backed up; the ones before and after it still are,
/// and every repo has its own outcome.
#[test]
fn backup_all_goes_on_past_a_repo_that_fails() {
    let root = scratch("all");
    server(&root, "owner/one", false);
    let server = server(&root, "owner/three", false);
    let backups = root.join("Backups");
    let api = ByPath::default();
    let files = Files::default();
    let repos: Vec<String> = ["owner/one", "owner/missing", "owner/three"]
        .map(String::from)
        .to_vec();

    let outcomes = backup_all_from(
        &settings(&backups),
        &repos,
        &remote(&server, &api, &files),
        TAKEN_AT,
    )
    .unwrap();

    let asked: Vec<&str> = outcomes.iter().map(|(repo, _)| repo.as_str()).collect();
    assert_eq!(asked, ["owner/one", "owner/missing", "owner/three"]);
    assert!(matches!(outcomes[0].1, Ok(BackupOutcome::Written(_))));
    match &outcomes[1].1 {
        Err(e) => assert!(e.to_string().contains("owner/missing"), "{e}"),
        other => panic!("expected an error, got {other:?}"),
    }
    assert!(matches!(outcomes[2].1, Ok(BackupOutcome::Written(_))));
    assert_eq!(
        names(&backups.join("github").join("owner")),
        ["one", "three"]
    );
}

/// The refusal for a last backup that cannot be compared with, and proof nothing was read or
/// changed.
fn refused_untouched(backups: &Path, server: &Path, needle: &str) {
    let dir = folder(backups, REPO);
    let before = tree(&dir);
    let api = ByPath::default();
    let files = Files::default();
    let outcome = backup_repo_from(
        &settings(backups),
        REPO,
        &remote(server, &api, &files),
        TAKEN_AT,
    );
    match outcome {
        Ok(BackupOutcome::Refused(why)) => {
            assert!(why.contains(needle), "{why}");
            assert!(why.contains("move it away or remove it"), "{why}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(tree(&dir), before, "the last backup is untouched");
    assert_eq!(names(dir.parent().unwrap()), ["demo"]);
    assert!(api.log.lock().unwrap().is_empty(), "nothing was read");
}

/// #370: a manifest cut short (a sync conflict, a full disk) must not let a new backup replace
/// the folder unchecked, and must not be a bare error either: the backup is refused with what
/// to do, like any other BAK-2 refusal.
#[test]
fn a_damaged_manifest_in_the_last_backup_refuses_the_new_one() {
    let root = scratch("damaged");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    back_up(&backups, &server, &ByPath::default());
    let manifest = folder(&backups, REPO).join("manifest.json");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, &text[..text.len() / 2]).unwrap();

    refused_untouched(&backups, &server, "manifest.json is damaged");
}

/// #370: a last backup with files but no manifest is not "no last backup".
#[test]
fn a_last_backup_with_no_manifest_refuses_the_new_one() {
    let root = scratch("no-manifest");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    back_up(&backups, &server, &ByPath::default());
    std::fs::remove_file(folder(&backups, REPO).join("manifest.json")).unwrap();

    refused_untouched(&backups, &server, "has no manifest.json");
}

/// #370: once the folder is moved away (or was only ever an empty folder), backups go on.
#[test]
fn an_empty_folder_is_no_last_backup() {
    let root = scratch("empty");
    let server = server(&root, REPO, false);
    let backups = root.join("Backups");
    std::fs::create_dir_all(folder(&backups, REPO)).unwrap();
    back_up(&backups, &server, &ByPath::default());
    assert!(folder(&backups, REPO).join("manifest.json").is_file());
}

/// #369: a repo whose backup failed is a row of its own, so the rows of the other repos in a
/// "Back up all" still come back.
#[test]
fn a_failed_backup_is_a_row_with_the_error_as_its_reason() {
    let backups = Path::new("/nc/Backups");
    let error = CoreError::Io("o/gone: git clone --mirror: not found".to_string());
    assert_eq!(
        failed_row(backups, "o/gone", &error),
        BackupRow {
            repo: "o/gone".to_string(),
            outcome: "failed".to_string(),
            folder: switchboard_core::backup_run::folder(backups, "o/gone")
                .display()
                .to_string(),
            manifest: None,
            reason: Some(error.to_string()),
        }
    );
}
