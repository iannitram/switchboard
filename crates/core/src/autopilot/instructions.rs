//! AUTO-7, AUTO-8: a job worktree gets the instruction file its CLI reads, written from the one
//! instruction source at the sha the job was claimed at, untracked and listed in
//! `.git/info/exclude`, never over a tracked file. A hand-kept copy that differs is drift.

use crate::CoreError;
use std::path::Path;

/// AUTO-7: write `file_name` with `contents` in `worktree`, add it to `.git/info/exclude`.
/// Refuses (CoreError::Refused) when `file_name` is tracked.
pub fn place(worktree: &Path, file_name: &str, contents: &str) -> Result<(), CoreError> {
    todo!("AUTO-7")
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
