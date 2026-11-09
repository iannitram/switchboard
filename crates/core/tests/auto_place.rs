//! AUTO-7: `place` writes an instruction file untracked (listed in `.git/info/exclude`), refuses
//! tracked files and names that leave the worktree. Tests use real temp git repos made with `git`.

use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::autopilot::instructions::place;
use switchboard_core::CoreError;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("git prints utf-8")
}

/// A fresh repo under the temp dir with one commit (README.md).
fn fresh_repo(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-auto7-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("README.md"), "seed\n").unwrap();
    git(&dir, &["add", "README.md"]);
    commit(&dir, "seed");
    dir
}

fn commit(dir: &Path, message: &str) {
    git(
        dir,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-q",
            "-m",
            message,
        ],
    );
}

fn exclude_lines(dir: &Path) -> Vec<String> {
    exclude_lines_at(&dir.join(".git").join("info").join("exclude"))
}

fn exclude_lines_at(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn count(lines: &[String], wanted: &str) -> usize {
    lines.iter().filter(|l| l.as_str() == wanted).count()
}

#[test]
fn writes_untracked_file_and_status_stays_clean() {
    let dir = fresh_repo("write");
    place(&dir, "CLAUDE.md", "rules v1\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap(),
        "rules v1\n"
    );
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");
    assert_eq!(count(&exclude_lines(&dir), "CLAUDE.md"), 1);
}

#[test]
fn relative_path_inside_worktree_is_written_and_excluded() {
    let dir = fresh_repo("relative");
    place(&dir, "docs/AGENTS.md", "rules\n").unwrap();
    assert!(dir.join("docs").join("AGENTS.md").is_file());
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");
    assert_eq!(count(&exclude_lines(&dir), "docs/AGENTS.md"), 1);
}

#[test]
fn twice_leaves_one_exclude_line_and_newer_contents() {
    let dir = fresh_repo("twice");
    place(&dir, "CLAUDE.md", "rules v1\n").unwrap();
    place(&dir, "CLAUDE.md", "rules v2\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap(),
        "rules v2\n"
    );
    assert_eq!(count(&exclude_lines(&dir), "CLAUDE.md"), 1);
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");
}

#[test]
fn tracked_file_is_refused_and_unchanged() {
    let dir = fresh_repo("tracked");
    std::fs::write(dir.join("CLAUDE.md"), "tracked rules\n").unwrap();
    git(&dir, &["add", "CLAUDE.md"]);
    commit(&dir, "add CLAUDE.md");

    let result = place(&dir, "CLAUDE.md", "replacement\n");
    assert!(
        matches!(result, Err(CoreError::Refused(_))),
        "got {result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap(),
        "tracked rules\n"
    );
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");
}

#[test]
fn linked_worktree_gets_the_exclude_entry() {
    let dir = fresh_repo("linked");
    let wt = std::env::temp_dir().join(format!("sb-auto7-linked-wt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&wt);
    git(
        &dir,
        &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
    );
    assert!(wt.join(".git").is_file(), "linked worktree has a .git file");

    place(&wt, "CLAUDE.md", "rules\n").unwrap();
    assert_eq!(git(&wt, &["status", "--porcelain"]), "");

    let exclude = PathBuf::from(git(&wt, &["rev-parse", "--git-path", "info/exclude"]).trim());
    let exclude = if exclude.is_absolute() {
        exclude
    } else {
        wt.join(exclude)
    };
    assert_eq!(count(&exclude_lines_at(&exclude), "CLAUDE.md"), 1);
}

#[test]
fn absolute_and_parent_names_are_refused_and_write_nothing() {
    let dir = fresh_repo("escape");
    let escaped = dir
        .parent()
        .unwrap()
        .join(format!("sb-auto7-escaped-{}.md", std::process::id()));
    let _ = std::fs::remove_file(&escaped);
    let absolute = dir.join("absolute.md");
    let absolute_name = absolute.to_str().unwrap().to_string();
    let parent_name = format!("../sb-auto7-escaped-{}.md", std::process::id());

    for name in [
        absolute_name.as_str(),
        parent_name.as_str(),
        "docs/../../escape.md",
        "",
    ] {
        let result = place(&dir, name, "x\n");
        assert!(
            matches!(result, Err(CoreError::Refused(_))),
            "{name:?} gave {result:?}"
        );
    }

    assert!(!absolute.exists());
    assert!(!escaped.exists());
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");
}
