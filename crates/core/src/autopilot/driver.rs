//! AUTO-6: the satellite's script loops. One `fleet run --next` per service lane, at most Qwen
//! plus one service job at once (R10). A model is brought in only to read a failure card.

/// The lanes running now, and the lane asked for: may it start?
pub fn may_start(running: &[String], lane: &str) -> bool {
    // A lane never runs twice at once.
    if running.iter().any(|r| r.as_str() == lane) {
        return false;
    }
    // Qwen may always start (it is the one lane that may share with a service job).
    if lane == "qwen" {
        return true;
    }
    // Any other lane starts only when no non-Qwen service job is running.
    running.iter().all(|r| r.as_str() == "qwen")
}

/// The command a loop runs for a lane: `fleet run --next --lane <lane>`.
pub fn next_command(lane: &str) -> Vec<String> {
    ["fleet", "run", "--next", "--lane", lane]
        .iter()
        .map(|s| s.to_string())
        .collect()
}
