//! SKY-5: drift. Orphan stars (an issue on no feature or on two), dark stars of `release: 1.0`
//! features (an After 1.0 dark star is planned, not drift), and a ROADMAP `Updated:` date more
//! than 14 days old.

use super::features::FeatureBlock;
use crate::github::Issue;

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
    todo!("SKY-5")
}
