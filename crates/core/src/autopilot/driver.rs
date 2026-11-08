//! AUTO-6: the satellite's script loops. One `fleet run --next` per service lane, at most Qwen
//! plus one service job at once (R10). A model is brought in only to read a failure card.

/// The lanes running now, and the lane asked for: may it start?
pub fn may_start(running: &[String], lane: &str) -> bool {
    todo!("AUTO-6")
}

/// The command a loop runs for a lane: `fleet run --next --lane <lane>`.
pub fn next_command(lane: &str) -> Vec<String> {
    todo!("AUTO-6")
}
