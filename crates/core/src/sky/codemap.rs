//! SKY-6: the code map's `reuses:` block, drawn as dashed reuse lines.
//!
//! The format (docs/code-map.md):
//!
//! ```text
//! reuses:
//! - brocrew: the chat parts, Waiting on you and the fixed report (Apache-2.0)
//! ```
//!
//! One line per project, `- <repo>: <what>`; the block ends at the first line that does not
//! start with `- `.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reuse {
    /// The repo name it reuses, as written.
    pub repo: String,
    pub what: String,
}

/// Parse code-map text; no `reuses:` line gives an empty list.
pub fn reuses(code_map_md: &str) -> Vec<Reuse> {
    let mut lines = code_map_md
        .lines()
        .skip_while(|line| line.trim_end() != "reuses:");
    // Skip the `reuses:` line itself; with no such line the iterator is already empty.
    lines.next();
    let mut found = Vec::new();
    for line in lines {
        let Some(rest) = line.strip_prefix("- ") else {
            break;
        };
        let (repo, what) = match rest.split_once(": ") {
            Some((repo, what)) => (repo.trim(), what.trim()),
            None => (rest.trim(), ""),
        };
        found.push(Reuse {
            repo: repo.to_string(),
            what: what.to_string(),
        });
    }
    found
}

/// SKY-6: a reuse line is a warning when the repo it names is Parked or archived.
pub fn is_warning(archived: bool, stage: Option<&str>) -> bool {
    archived || stage.is_some_and(|stage| stage.trim().eq_ignore_ascii_case("parked"))
}
