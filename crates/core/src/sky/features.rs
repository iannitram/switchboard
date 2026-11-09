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
//! - gamification: A gamification plugin (release: after-2.0)
//! ```
//!
//! One line per feature, `- <id>: <title> (release: 1.0|after-1.0|after-2.0)`. An id is lowercase
//! letters, digits and hyphens. The block ends at the first line that does not start with
//! `- `. A line without a release marker is `release: 1.0`.

use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    V1,
    After1,
    After2,
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
    let mut updated: Option<String> = None;
    let mut features: Vec<Feature> = Vec::new();
    let mut in_block = false;
    let mut block_done = false;

    for (index, line) in roadmap_md.lines().enumerate() {
        let line_no = index + 1;

        if updated.is_none() && line.starts_with("Updated:") {
            updated = Some(line["Updated:".len()..].trim().to_string());
        }

        if block_done {
            continue;
        }
        if !in_block {
            in_block = line.trim_end() == "features:";
            continue;
        }
        if !line.starts_with("- ") {
            block_done = true;
            continue;
        }

        let rest = line["- ".len()..]
            .split_once(": ")
            .ok_or_else(|| bad(line_no, line))?;
        let (id, rest) = rest;
        if !valid_id(id) {
            return Err(bad(line_no, line));
        }
        let rest = rest.trim();
        let (title, release) = if let Some(before) = rest.strip_suffix("(release: 1.0)") {
            (before.trim(), Release::V1)
        } else if let Some(before) = rest.strip_suffix("(release: after-1.0)") {
            (before.trim(), Release::After1)
        } else if let Some(before) = rest.strip_suffix("(release: after-2.0)") {
            (before.trim(), Release::After2)
        } else if rest.contains("(release: ") {
            return Err(bad(line_no, line));
        } else {
            (rest, Release::V1)
        };
        if title.is_empty() {
            return Err(bad(line_no, line));
        }
        features.push(Feature {
            id: id.to_string(),
            title: title.to_string(),
            release,
        });
    }

    Ok(FeatureBlock { updated, features })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn bad(line_no: usize, line: &str) -> CoreError {
    CoreError::Parse(format!("ROADMAP.md line {line_no}: {line}"))
}
