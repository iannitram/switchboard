//! AUTO-7, AUTO-8: a job worktree gets the instruction file its CLI reads, written from the one
//! instruction source at the sha the job was claimed at, untracked and listed in
//! `.git/info/exclude`, never over a tracked file. A hand-kept copy that differs is drift.

use crate::CoreError;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

/// AUTO-7: write `file_name` with `contents` in `worktree`, add it to `.git/info/exclude`.
/// Refuses (CoreError::Refused) when `file_name` is tracked.
pub fn place(worktree: &Path, file_name: &str, contents: &str) -> Result<(), CoreError> {
    let rel = relative_name(file_name)?;
    if is_tracked(worktree, &rel)? {
        return Err(CoreError::Refused(format!(
            "{rel} is tracked in this worktree"
        )));
    }
    let target = worktree.join(&rel);
    make_parent(&target)?;
    std::fs::write(&target, contents).map_err(|e| io_err(&target, &e))?;
    add_exclude_line(&exclude_path(worktree)?, &rel)
}

/// A relative name made only of normal parts (no `..`, no root, no prefix), joined with `/`.
fn relative_name(file_name: &str) -> Result<String, CoreError> {
    let refuse = |why: &str| CoreError::Refused(format!("instruction file {file_name:?} {why}"));
    let mut parts = Vec::new();
    for component in Path::new(file_name).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return Err(refuse("must be a relative path inside the worktree")),
        }
    }
    if parts.is_empty() {
        return Err(refuse("names no file"));
    }
    Ok(parts.join("/"))
}

/// `git ls-files --error-unmatch`: exit 0 is tracked, exit 1 is untracked, anything else an error.
fn is_tracked(worktree: &Path, rel: &str) -> Result<bool, CoreError> {
    let out = git(worktree, &["ls-files", "--error-unmatch", "--", rel])?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(git_err("ls-files --error-unmatch", &out)),
    }
}

/// The exclude file, from `git rev-parse --git-path info/exclude` (relative paths are relative to
/// the worktree; a linked worktree's `.git` is a file, and git still answers).
fn exclude_path(worktree: &Path) -> Result<PathBuf, CoreError> {
    let out = git(worktree, &["rev-parse", "--git-path", "info/exclude"])?;
    if !out.status.success() {
        return Err(git_err("rev-parse --git-path info/exclude", &out));
    }
    let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    Ok(if path.is_absolute() {
        path
    } else {
        worktree.join(path)
    })
}

/// Append `line` unless the file already has it; creates the file when absent.
fn add_exclude_line(exclude: &Path, line: &str) -> Result<(), CoreError> {
    let existing = match std::fs::read_to_string(exclude) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(io_err(exclude, &e)),
    };
    if existing.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(line);
    next.push('\n');
    make_parent(exclude)?;
    std::fs::write(exclude, next).map_err(|e| io_err(exclude, &e))
}

fn make_parent(path: &Path) -> Result<(), CoreError> {
    match path.parent() {
        Some(parent) => std::fs::create_dir_all(parent).map_err(|e| io_err(parent, &e)),
        None => Ok(()),
    }
}

fn git(worktree: &Path, args: &[&str]) -> Result<Output, CoreError> {
    Command::new("git")
        .args(args)
        .current_dir(worktree)
        .output()
        .map_err(|e| CoreError::Io(format!("git {}: {e}", args.join(" "))))
}

fn git_err(command: &str, out: &Output) -> CoreError {
    CoreError::Io(format!(
        "git {command} failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}

fn io_err(path: &Path, e: &std::io::Error) -> CoreError {
    CoreError::Io(format!("{}: {e}", path.display()))
}

/// AUTO-8: the hand-kept copies (path, text) that differ from the source text.
/// Texts are compared with `\r\n` read as `\n` and trailing whitespace at the end trimmed; any
/// other difference is drift. Paths come back in the order of `copies`.
pub fn drifted<'a>(source: &str, copies: &'a [(String, String)]) -> Vec<&'a str> {
    let source = normalized(source);
    copies
        .iter()
        .filter(|(_, text)| normalized(text) != source)
        .map(|(path, _)| path.as_str())
        .collect()
}

/// The text as compared for drift: `\r\n` becomes `\n`, trailing whitespace at the end is dropped.
fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
}
