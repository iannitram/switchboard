//! Locked tests for BAK-1 (the wiki as a bare mirror in `wiki.git`, and `manifest.wiki`).
//! Builders never edit this file (R28). Every repo here is made by local `git` in a temp dir.

use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::backup::{copy_wiki, Manifest};

fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-bak1-{tag}-{}-{nanos}", std::process::id()));
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

/// A bare wiki repo (as GitHub serves `<repo>.wiki.git`) with one commit, "wiki first".
/// Returns (work tree, bare mirror).
fn wiki(root: &Path) -> (PathBuf, PathBuf) {
    let work = root.join("wiki-work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "master"]);
    std::fs::write(work.join("Home.md"), "home\n").unwrap();
    git(&work, &["add", "Home.md"]);
    git(&work, &["commit", "-q", "-m", "wiki first"]);
    let bare = root.join("demo.wiki.git");
    git(
        root,
        &[
            "clone",
            "-q",
            "--mirror",
            work.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    (work, bare)
}

fn subject(repo: &Path) -> String {
    git(repo, &["log", "-1", "--format=%s", "master"])
}

#[test]
fn a_wiki_gives_a_bare_wiki_git_and_manifest_wiki_true() {
    let root = scratch("wiki");
    let (_, bare) = wiki(&root);
    let out = root.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut m = Manifest {
        releases: 3,
        ..Manifest::default()
    };

    copy_wiki(Some(&bare), &out, &mut m).unwrap();

    let copy = out.join("wiki.git");
    assert!(copy.is_dir(), "wiki.git should be written");
    assert_eq!(git(&copy, &["rev-parse", "--is-bare-repository"]), "true");
    assert_eq!(subject(&copy), "wiki first");
    assert!(!out.join("wiki.git.new").exists());
    assert_eq!(
        m,
        Manifest {
            releases: 3,
            wiki: true,
            ..Manifest::default()
        }
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn no_wiki_sets_false_and_writes_nothing() {
    let root = scratch("nowiki");
    let out = root.join("out");
    let mut m = Manifest {
        wiki: true,
        ..Manifest::default()
    };

    copy_wiki(None, &out, &mut m).unwrap();

    assert!(!m.wiki);
    assert!(!out.exists(), "no wiki writes nothing, not even out_dir");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_second_run_replaces_the_first_copy() {
    let root = scratch("wiki-again");
    let (work, bare) = wiki(&root);
    let out = root.join("Backups").join("github").join("demo");
    let mut m = Manifest::default();
    copy_wiki(Some(&bare), &out, &mut m).unwrap();

    std::fs::write(work.join("Home.md"), "home two\n").unwrap();
    git(&work, &["commit", "-q", "-am", "wiki second"]);
    git(&work, &["push", "-q", bare.to_str().unwrap(), "master"]);
    copy_wiki(Some(&bare), &out, &mut m).unwrap();

    assert_eq!(subject(&out.join("wiki.git")), "wiki second");
    assert!(m.wiki);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_path_that_is_not_a_repo_is_an_error_and_keeps_the_old_copy() {
    let root = scratch("wiki-bad");
    let (_, bare) = wiki(&root);
    let out = root.join("out");
    let mut m = Manifest::default();
    copy_wiki(Some(&bare), &out, &mut m).unwrap();

    let not_repo = root.join("not-a-repo");
    std::fs::create_dir_all(&not_repo).unwrap();
    let mut fresh = Manifest::default();
    let result = copy_wiki(Some(&not_repo), &out, &mut fresh);

    assert!(result.is_err(), "a path that is not a git repo is an error");
    assert!(!fresh.wiki, "a failed copy leaves manifest.wiki false");
    assert_eq!(subject(&out.join("wiki.git")), "wiki first");
    assert!(
        !out.join("wiki.git.new").exists(),
        "a failed run leaves no half-written copy"
    );
    let _ = std::fs::remove_dir_all(&root);
}
