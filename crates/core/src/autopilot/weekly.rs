//! AUTO-2, AUTO-5: jobs the core starts on its own: a routed Project Manager job when a
//! ROADMAP changes, and the weekly drift clear and backup once Sunday's 6am reset has passed.

use crate::clock::UnixSeconds;

/// AUTO-5: true when the newest Sunday 6am (local, `utc_offset_secs`) at or before `now` is
/// later than `last_run`.
pub fn weekly_due(now: UnixSeconds, utc_offset_secs: i64, last_run: Option<UnixSeconds>) -> bool {
    todo!("AUTO-5")
}

/// AUTO-2: the repos whose ROADMAP.md text changed between two snapshots (by repo name).
pub fn roadmap_changed(
    previous: &[(String, Option<String>)],
    next: &[(String, Option<String>)],
) -> Vec<String> {
    todo!("AUTO-2")
}
