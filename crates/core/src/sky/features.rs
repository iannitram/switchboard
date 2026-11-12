//! ROADMAP features, its `Updated:` date, and development stages for the Roadmap lens.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevelopmentStage {
    pub number: u32,
    pub name: String,
    pub features: Vec<String>,
}

/// Read the `# | Stage | Features` table under "Development stages", in numeric order.
/// The leading bold phrase is the stage's name when the cell also contains a description.
/// Feature ids are backtick-delimited; prose such as "all with a view" is not an id.
/// A feature listed more than once belongs to the first stage. No table gives `None`.
pub fn parse_stages(roadmap_md: &str) -> Option<Vec<DevelopmentStage>> {
    let mut in_section = false;
    let mut in_table = false;
    let mut fence: Option<char> = None;
    let mut stages = Vec::new();
    for line in roadmap_md.lines().map(str::trim) {
        if line.starts_with("```") || line.starts_with("~~~") {
            let marker = line.chars().next().unwrap();
            if fence == Some(marker) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(marker);
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        if line.starts_with('#') {
            if in_section {
                break;
            }
            in_section = line.trim_start_matches('#').trim() == "Development stages";
            continue;
        }
        if !in_section {
            continue;
        }
        let cells: Vec<_> = line.trim_matches('|').split('|').map(str::trim).collect();
        if !in_table {
            in_table = cells.starts_with(&["#", "Stage", "Features"]);
            continue;
        }
        if !line.contains('|') {
            break;
        }
        let Some(number) = cells.first().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if cells.len() < 3 || cells[1].is_empty() {
            continue;
        }
        let name = cells[1]
            .strip_prefix("**")
            .and_then(|s| s.split_once("**").map(|(name, _)| name))
            .unwrap_or(cells[1])
            .to_string();
        let features = cells[2]
            .split('`')
            .enumerate()
            .filter(|(i, id)| i % 2 == 1 && valid_id(id))
            .map(|(_, id)| id.to_string())
            .collect();
        stages.push(DevelopmentStage {
            number,
            name,
            features,
        });
    }
    if !in_table {
        return None;
    }
    stages.sort_by_key(|stage| stage.number);
    let mut assigned = std::collections::HashSet::new();
    for stage in &mut stages {
        stage.features.retain(|id| assigned.insert(id.clone()));
    }
    Some(stages)
}
