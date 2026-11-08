//! The ROADMAP `features:` block and its `Updated:` date (SKY-2, SKY-3, SKY-5 read them).
//!
//! The format (ROADMAP.md, "The features block"):
//!
//! ```text
//! Updated: 2026-11-08
//!
//! features:
//! - core: The core and the fleet bridge (release: 1.0)
//! - relay: A headless relay per machine (release: after-1.0)
//! ```
//!
//! One line per feature, `- <id>: <title> (release: 1.0|after-1.0)`. An id is lowercase
//! letters, digits and hyphens. The block ends at the first line that does not start with
//! `- `. A line without a release marker is `release: 1.0`.

use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    V1,
    After1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feature {
    pub id: String,
    pub title: String,
    pub release: Release,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureBlock {
    /// `Updated:` as written, `YYYY-MM-DD`; `None` when the roadmap has no such line.
    pub updated: Option<String>,
    pub features: Vec<Feature>,
}

/// Parse ROADMAP.md text. No `features:` line gives an empty list; a malformed line inside the
/// block (bad id, no `: `) is `CoreError::Parse` naming the line number (1-based).
pub fn parse(roadmap_md: &str) -> Result<FeatureBlock, CoreError> {
    todo!("sky: features block")
}
