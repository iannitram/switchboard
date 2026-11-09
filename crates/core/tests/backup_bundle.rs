//! Locked tests for #90 (BAK-1: a git bundle of each repo's mirror). Builders never edit this
//! file (R28).
//!
//! Every repo here is made by local `git` in a temp dir; nothing touches the network.

use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::backup::bundle;

/// A fresh, empty temp dir for one test.
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

/// A mirror clone of a repo with 2 branches (`main`, `feature`) and 1 tag (`v1`): 3 refs.
fn mirror_with_three_refs(root: &Path) -> PathBuf {
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
    mirror
}

/// `git bundle verify` on `file`, run inside `repo`; true when it passes.
fn verifies(repo: &Path, file: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["bundle", "verify", "-q"])
        .arg(file)
        .output()
        .expect("git runs")
        .status
        .success()
}

/// The ref names the bundle holds (`git bundle list-heads`).
fn heads(repo: &Path, file: &Path) -> Vec<String> {
    git(repo, &["bundle", "list-heads", file.to_str().unwrap()])
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1).map(String::from))
        .collect()
}

fn assert_holds(repo: &Path, file: &Path, refs: &[&str]) {
    let got = heads(repo, file);
    for r in refs {
        assert!(
            got.iter().any(|h| h == r),
            "bundle should hold {r}; it holds {got:?}"
        );
    }
}

#[test]
fn two_branches_and_a_tag_give_three_refs_and_a_bundle_that_verifies() {
    let root = scratch("three");
    let mirror = mirror_with_three_refs(&root);
    let out = root.join("out");
    std::fs::create_dir_all(&out).unwrap();

    assert_eq!(bundle(&mirror, &out).unwrap(), 3);

    let file = out.join("repo.bundle");
    assert!(file.is_file(), "repo.bundle should be written");
    assert!(verifies(&mirror, &file), "git bundle verify should pass");
    assert_holds(
        &mirror,
        &file,
        &["refs/heads/main", "refs/heads/feature", "refs/tags/v1"],
    );
    assert!(
        !out.join("repo.bundle.new").exists(),
        "the .new file should be renamed over repo.bundle"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_second_run_replaces_the_first_bundle() {
    let root = scratch("again");
    let mirror = mirror_with_three_refs(&root);
    let out = root.join("out");

    assert_eq!(bundle(&mirror, &out).unwrap(), 3);
    git(&mirror, &["branch", "extra", "main"]);
    assert_eq!(bundle(&mirror, &out).unwrap(), 4);

    let file = out.join("repo.bundle");
    assert!(verifies(&mirror, &file), "git bundle verify should pass");
    assert_holds(
        &mirror,
        &file,
        &[
            "refs/heads/main",
            "refs/heads/feature",
            "refs/heads/extra",
            "refs/tags/v1",
        ],
    );
    assert!(!out.join("repo.bundle.new").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_empty_mirror_is_an_error_and_keeps_the_older_bundle() {
    let root = scratch("empty");
    let good = mirror_with_three_refs(&root);
    let out = root.join("out");
    assert_eq!(bundle(&good, &out).unwrap(), 3);
    let file = out.join("repo.bundle");
    let before = std::fs::read(&file).unwrap();

    let empty = root.join("empty.git");
    std::fs::create_dir_all(&empty).unwrap();
    git(&empty, &["init", "-q", "--bare"]);

    assert!(
        bundle(&empty, &out).is_err(),
        "a mirror with no refs should be an error, not an empty bundle"
    );
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "the older repo.bundle should be left as it was"
    );
    assert!(verifies(&good, &file));
    assert!(
        !out.join("repo.bundle.new").exists(),
        "a failed run should leave no half-written file"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_missing_out_dir_is_created() {
    let root = scratch("mkdir");
    let mirror = mirror_with_three_refs(&root);
    let out = root.join("Backups").join("github").join("demo");
    assert!(!out.exists());

    assert_eq!(bundle(&mirror, &out).unwrap(), 3);

    let file = out.join("repo.bundle");
    assert!(
        file.is_file(),
        "repo.bundle should be written in the new dir"
    );
    assert!(verifies(&mirror, &file));
    let _ = std::fs::remove_dir_all(&root);
}
