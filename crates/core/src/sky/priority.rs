//! SKY-9: the priority copy. The priority board lives in a claude.ai Artifact database a Claude
//! session can read; until U4 is answered, a copy of the order sits in fleet-state, refreshed by
//! a Claude session at the Sunday drift clear and whenever I re-rank.
//!
//! The copy: JSON `{"updated_at": "<RFC 3339>", "order": ["owner/repo", ...]}`.

use crate::clock::UnixSeconds;
use crate::CoreError;

/// Older than this and the copy becomes a Waiting on you item.
pub const STALE_DAYS: i64 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityCopy {
    pub updated_at: UnixSeconds,
    pub order: Vec<String>,
}

pub fn parse(json: &str) -> Result<PriorityCopy, CoreError> {
    todo!("SKY-9")
}

/// Age in whole days at `now`.
pub fn age_days(copy: &PriorityCopy, now: UnixSeconds) -> i64 {
    todo!("SKY-9")
}

pub fn is_stale(copy: &PriorityCopy, now: UnixSeconds) -> bool {
    todo!("SKY-9")
}
