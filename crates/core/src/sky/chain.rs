//! SKY-3, SKY-7: the blocked-by lines and the longest chain of them.

use crate::github::Issue;

/// SKY-3: every `Blocked by #N` in an issue body, as (blocked, blocker) pairs, in issue order.
/// `#N` may repeat on one line (`Blocked by #3, #4`).
pub fn edges(issues: &[Issue]) -> Vec<(u64, u64)> {
    todo!("SKY-3")
}

/// SKY-7: the longest chain over the open issues, as issue numbers from the first blocker to
/// the last blocked. A cycle is broken at its lowest-numbered issue. Ties go to the chain whose
/// first issue has the lower number.
pub fn longest(issues: &[Issue]) -> Vec<u64> {
    todo!("SKY-7")
}
