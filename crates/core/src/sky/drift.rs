//! SKY-5: drift. Orphan stars (an issue on no feature or on two), dark stars of `release: 1.0` (`after-1.0` and `after-2.0` are planned, not drift)
//! features (an After 1.0 dark star is planned, not drift), and a ROADMAP `Updated:` date more
//! than 14 days old.

use std::collections::HashSet;

use super::features::{FeatureBlock, Release};
use crate::github::Issue;

const FEATURE_PREFIX: &str = "feature:";

/// More than this many days since `Updated:` is a stale roadmap.
pub const STALE_DAYS: i64 = 14;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// Issue number and its `feature:` labels (none, or two or more).
    Orphan { number: u64, features: Vec<String> },
    /// A `release: 1.0` feature id with no issue, open or closed.
    Dark { feature: String },
    /// `Updated:` date and its age in days; `updated` is `None` when the line is missing.
    Stale {
        updated: Option<String>,
        age_days: Option<i64>,
    },
}

/// Every drift item for one project, orphans by issue number, then dark stars in block order,
/// then the stale date. `today` is `YYYY-MM-DD`.
pub fn find(issues: &[Issue], block: &FeatureBlock, today: &str) -> Vec<Drift> {
    let mut out = Vec::new();

    let mut by_number: Vec<&Issue> = issues.iter().collect();
    by_number.sort_by_key(|issue| issue.number);
    for issue in by_number {
        let ids = feature_ids(issue);
        let is_orphan = match ids.as_slice() {
            [only] => !block.features.iter().any(|f| f.id == *only),
            _ => true,
        };
        if is_orphan {
            out.push(Drift::Orphan {
                number: issue.number,
                features: ids,
            });
        }
    }

    let starred: HashSet<String> = issues.iter().flat_map(feature_ids).collect();
    for f in &block.features {
        if f.release == Release::V1 && !starred.contains(&f.id) {
            out.push(Drift::Dark {
                feature: f.id.clone(),
            });
        }
    }

    if let Some(stale) = stale(block.updated.as_deref(), today) {
        out.push(stale);
    }
    out
}

/// The `feature:` labels of an issue, prefix removed, in label order.
fn feature_ids(issue: &Issue) -> Vec<String> {
    issue
        .labels
        .iter()
        .filter_map(|label| label.strip_prefix(FEATURE_PREFIX))
        .map(str::to_string)
        .collect()
}

/// `None` when the roadmap date is present, parses, and is 14 days old or less.
fn stale(updated: Option<&str>, today: &str) -> Option<Drift> {
    let Some(updated) = updated else {
        return Some(Drift::Stale {
            updated: None,
            age_days: None,
        });
    };
    let age = day_number(updated)
        .zip(day_number(today))
        .map(|(u, t)| t - u);
    match age {
        Some(age) if age <= STALE_DAYS => None,
        _ => Some(Drift::Stale {
            updated: Some(updated.to_string()),
            age_days: age,
        }),
    }
}

/// Day number of a strict `YYYY-MM-DD` date (1970-01-01 is 0), or `None` if it does not parse.
fn day_number(date: &str) -> Option<i64> {
    let b = date.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| {
        b[range].iter().try_fold(0i64, |acc, &c| match c {
            b'0'..=b'9' => Some(acc * 10 + i64::from(c - b'0')),
            _ => None,
        })
    };
    let y = number(0..4)?;
    let m = number(5..7)?;
    let d = number(8..10)?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }

    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}
