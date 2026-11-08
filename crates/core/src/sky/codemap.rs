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
    todo!("SKY-6")
}

/// SKY-6: a reuse line is a warning when the repo it names is Parked or archived.
pub fn is_warning(archived: bool, stage: Option<&str>) -> bool {
    todo!("SKY-6")
}
