//! AUTO-6: the loop driver. At most Qwen plus one service job at once (R10), and `next_command`
//! is the argv a lane's loop runs. Both are pure functions, so the tests need no fleet stub.

use switchboard_core::autopilot::driver::{may_start, next_command};

fn running(lanes: &[&str]) -> Vec<String> {
    lanes.iter().map(|l| l.to_string()).collect()
}

#[test]
fn nothing_running_lets_qwen_and_gemini_start() {
    assert!(may_start(&running(&[]), "qwen"));
    assert!(may_start(&running(&[]), "gemini"));
}

#[test]
fn qwen_running_lets_gemini_start_but_not_a_second_qwen() {
    let r = running(&["qwen"]);
    assert!(may_start(&r, "gemini"));
    assert!(!may_start(&r, "qwen"));
}

#[test]
fn gemini_running_lets_qwen_start_but_no_other_service_lane() {
    let r = running(&["gemini"]);
    assert!(may_start(&r, "qwen"));
    assert!(!may_start(&r, "chatgpt"));
    assert!(!may_start(&r, "claudeapi"));
    assert!(!may_start(&r, "gemini"));
}

#[test]
fn qwen_and_gemini_running_lets_no_lane_start() {
    let r = running(&["qwen", "gemini"]);
    for &lane in [
        "qwen",
        "gemini",
        "chatgpt",
        "claudeapi",
        "claude",
        "deepseek",
    ]
    .iter()
    {
        assert!(!may_start(&r, lane), "{lane} may not start");
    }
}

#[test]
fn next_command_is_the_fleet_run_for_the_lane() {
    assert_eq!(
        next_command("gemini"),
        vec!["fleet", "run", "--next", "--lane", "gemini"]
    );
}
