//! Locked tests for BAK-1: `backup::bundle` writes a verified git bundle of every ref of a
//! mirror clone and returns the ref count. Each test builds its repos in a temp dir with git.

use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::backup::bundle;

/// A fresh, empty directory under the system temp dir, unique per process and test.
fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("switchboard-bundle-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs `git -C <dir> <args>` and panics with git's stderr when it fails. Returns stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A work repo with one commit, a second branch and a tag: 2 branches and 1 tag. Returns a
/// mirror clone of it at `<root>/mirror.git`.
fn mirror_with_two_branches_and_a_tag(root: &Path) -> PathBuf {
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q"]);
    std::fs::write(work.join("README.md"), "hello\n").unwrap();
    git(&work, &["add", "README.md"]);
    git(&work, &["commit", "-q", "-m", "first"]);
    git(&work, &["branch", "other"]);
    git(&work, &["tag", "v1"]);
    let mirror = root.join("mirror.git");
    let (work_s, mirror_s) = (work.to_str().unwrap(), mirror.to_str().unwrap());
    git(root, &["clone", "-q", "--mirror", work_s, mirror_s]);
    mirror
}

#[test]
fn a_repo_with_two_branches_and_a_tag_gives_three_refs_and_a_verified_bundle() {
    let root = scratch("three-refs");
    let mirror = mirror_with_two_branches_and_a_tag(&root);
    let out = root.join("backups");

    let refs = bundle(&mirror, &out).unwrap();

    assert_eq!(refs, 3);
    let file = out.join("repo.bundle");
    assert!(file.is_file());
    assert!(!out.join("repo.bundle.new").exists());
    git(&mirror, &["bundle", "verify", file.to_str().unwrap()]);
}

#[test]
fn a_second_run_replaces_the_first_bundle() {
    let root = scratch("second-run");
    let mirror = mirror_with_two_branches_and_a_tag(&root);
    let out = root.join("backups");
    assert_eq!(bundle(&mirror, &out).unwrap(), 3);
    let first = std::fs::read(out.join("repo.bundle")).unwrap();

    git(&mirror, &["branch", "third"]);
    let refs = bundle(&mirror, &out).unwrap();

    assert_eq!(refs, 4);
    let file = out.join("repo.bundle");
    assert_ne!(std::fs::read(&file).unwrap(), first);
    assert!(!out.join("repo.bundle.new").exists());
    git(&mirror, &["bundle", "verify", file.to_str().unwrap()]);
}

#[test]
fn an_empty_mirror_is_an_error_and_keeps_the_older_bundle() {
    let root = scratch("empty");
    let mirror = mirror_with_two_branches_and_a_tag(&root);
    let out = root.join("backups");
    bundle(&mirror, &out).unwrap();
    let kept = std::fs::read(out.join("repo.bundle")).unwrap();

    let empty = root.join("empty.git");
    git(&root, &["init", "-q", "--bare", empty.to_str().unwrap()]);
    let result = bundle(&empty, &out);

    assert!(result.is_err());
    assert_eq!(std::fs::read(out.join("repo.bundle")).unwrap(), kept);
    assert!(!out.join("repo.bundle.new").exists());
}

#[test]
fn a_missing_out_dir_is_created() {
    let root = scratch("missing-dir");
    let mirror = mirror_with_two_branches_and_a_tag(&root);
    let out = root.join("does").join("not").join("exist");

    assert_eq!(bundle(&mirror, &out).unwrap(), 3);
    assert!(out.join("repo.bundle").is_file());
}
