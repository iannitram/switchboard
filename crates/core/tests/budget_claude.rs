//! BUD-4 (#28): Claude's pool from the newest recorded reading plus a marked estimate from my
//! sessions' local usage logs. Locked tests (R28): the builder never edits this file.

use std::path::{Path, PathBuf};
use switchboard_core::budget::{claude_pool, parse_utc, session_log_paths, ClaudePool, PoolRow};
use switchboard_core::CoreError;

/// 2026-11-08 2:00pm UTC.
const SINCE: i64 = 1_794_146_400;

const LOG_ONE: &str = r#"{"type":"assistant","timestamp":"2026-11-08T14:00:00Z","requestId":"req_1","message":{"id":"msg_1","usage":{"input_tokens":100,"output_tokens":200,"cache_creation_input_tokens":300,"cache_read_input_tokens":5000}}}
{"type":"assistant","timestamp":"2026-11-08T13:59:59Z","requestId":"req_0","message":{"id":"msg_0","usage":{"input_tokens":1000,"output_tokens":1000}}}
{"type":"assistant","timestamp":"2026-11-08T14:00:00Z","requestId":"req_1","message":{"id":"msg_1","usage":{"input_tokens":100,"output_tokens":200,"cache_creation_input_tokens":300}}}
{"type":"user","timestamp":"2026-11-08T14:10:00Z","message":{"role":"user","content":"hi"}}
garbage
{"type":"assistant","timestamp":"yesterday","message":{"id":"msg_9","usage":{"input_tokens":7,"output_tokens":7}}}
"#;

const LOG_TWO: &str = r#"{"type":"assistant","timestamp":"2026-11-08T15:05:30.250Z","requestId":"req_2","message":{"id":"msg_2","usage":{"input_tokens":10,"output_tokens":20}}}
{"type":"assistant","timestamp":"2026-11-08T15:06:00Z","message":{"usage":{"input_tokens":1,"output_tokens":2}}}
"#;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-bud4-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(path: &Path, text: &str) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    path.to_path_buf()
}

fn reading(pool: &str, pct: Option<f64>, age: Option<i64>) -> PoolRow {
    PoolRow {
        pool: pool.into(),
        pct,
        pace: Some(3.0),
        delta: Some(-1.0),
        verdict: "on-pace".into(),
        reading_age_min: age,
        stale: false,
        why: String::new(),
    }
}

fn logs(name: &str) -> Vec<PathBuf> {
    let d = dir(name);
    vec![
        write(&d.join("one.jsonl"), LOG_ONE),
        write(&d.join("two.jsonl"), LOG_TWO),
    ]
}

#[test]
fn parse_utc_reads_whole_and_fractional_seconds() {
    assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_utc("2026-11-08T14:00:00Z"), Some(1_794_146_400));
    assert_eq!(parse_utc("2026-11-08T13:59:59Z"), Some(1_794_146_399));
    assert_eq!(parse_utc("2026-11-08T15:05:30.250Z"), Some(1_794_150_330));
    assert_eq!(parse_utc("2000-02-29T00:00:00Z"), Some(951_782_400));
}

#[test]
fn parse_utc_refuses_anything_else() {
    for bad in [
        "",
        "2026-11-08 14:00:00Z",
        "2026-11-08T14:00:00+00:00",
        "2026-11-08T14:00:00",
        "2026-13-01T00:00:00Z",
        "2026-11-08T24:00:00Z",
        "yesterday",
    ] {
        assert_eq!(parse_utc(bad), None, "{bad}");
    }
}

#[test]
fn the_reading_is_kept_apart_from_the_estimate() {
    let pool = claude_pool(
        &reading("claude", Some(2.0), Some(65)),
        &logs("apart"),
        SINCE,
    )
    .unwrap();
    assert_eq!(
        pool,
        ClaudePool {
            reading_pct: Some(2.0),
            reading_age_min: Some(65),
            estimate_tokens_since: 633,
            estimate_messages: 3,
        }
    );
}

#[test]
fn only_lines_at_or_after_since_count() {
    let later = claude_pool(
        &reading("claude", Some(2.0), Some(65)),
        &logs("later"),
        SINCE + 1,
    )
    .unwrap();
    assert_eq!(later.estimate_tokens_since, 33);
    assert_eq!(later.estimate_messages, 2);
}

#[test]
fn a_missing_log_is_skipped() {
    let mut paths = logs("missing");
    paths.insert(0, dir("missing-gone").join("gone.jsonl"));
    let pool = claude_pool(&reading("claude", Some(2.0), Some(65)), &paths, SINCE).unwrap();
    assert_eq!(pool.estimate_tokens_since, 633);
}

#[test]
fn a_log_that_cannot_be_read_is_an_io_error() {
    let d = dir("unreadable");
    match claude_pool(
        &reading("claude", Some(2.0), Some(65)),
        std::slice::from_ref(&d),
        SINCE,
    ) {
        Err(CoreError::Io(msg)) => assert!(msg.starts_with(&d.display().to_string()), "{msg}"),
        other => panic!("expected an io error, got {other:?}"),
    }
}

#[test]
fn no_logs_gives_a_zero_estimate() {
    let pool = claude_pool(&reading("claude", Some(2.0), Some(65)), &[], SINCE).unwrap();
    assert_eq!(pool.estimate_tokens_since, 0);
    assert_eq!(pool.estimate_messages, 0);
}

#[test]
fn only_the_claude_row_is_accepted() {
    assert_eq!(
        claude_pool(&reading("chatgpt", Some(2.0), Some(65)), &[], SINCE),
        Err(CoreError::Parse(
            "claude_pool needs the claude row, got chatgpt".into()
        ))
    );
}

#[test]
fn the_labels_say_which_is_the_reading_and_which_the_estimate() {
    let pool = claude_pool(
        &reading("claude", Some(2.0), Some(65)),
        &logs("labels"),
        SINCE,
    )
    .unwrap();
    assert_eq!(pool.reading_label(), "2.0% (reading, 65 min old)");
    assert_eq!(
        pool.estimate_label(),
        "estimate: 633 tokens in 3 messages since the reading"
    );
}

#[test]
fn a_missing_reading_is_never_filled_in() {
    let none = claude_pool(&reading("claude", None, None), &logs("none"), SINCE).unwrap();
    assert_eq!(none.reading_pct, None);
    assert_eq!(none.reading_label(), "no reading");
    let no_age = claude_pool(&reading("claude", Some(2.0), None), &[], SINCE).unwrap();
    assert_eq!(no_age.reading_label(), "2.0% (reading, age not recorded)");
}

#[test]
fn the_reading_row_is_not_changed() {
    let r = reading("claude", Some(2.0), Some(65));
    let before = r.clone();
    let _ = claude_pool(&r, &logs("frozen"), SINCE).unwrap();
    assert_eq!(r, before);
}

#[test]
fn session_log_paths_finds_every_jsonl_at_any_depth_sorted() {
    let d = dir("walk");
    write(&d.join("a").join("s1.jsonl"), "");
    write(
        &d.join("a").join("s1").join("subagents").join("x.jsonl"),
        "",
    );
    write(&d.join("b").join("s2.jsonl"), "");
    write(&d.join("b").join("notes.txt"), "");
    assert_eq!(
        session_log_paths(&d).unwrap(),
        vec![
            d.join("a").join("s1").join("subagents").join("x.jsonl"),
            d.join("a").join("s1.jsonl"),
            d.join("b").join("s2.jsonl"),
        ]
    );
}

#[test]
fn session_log_paths_of_a_missing_folder_is_empty() {
    let d = dir("nofolder").join("projects");
    assert_eq!(session_log_paths(&d).unwrap(), Vec::<PathBuf>::new());
}

#[test]
fn the_code_map_marks_the_claude_pool_present() {
    let map = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md"),
    )
    .unwrap();
    assert!(map.contains(
        "- BUD-4 Claude pool (`claude_pool`, `session_log_paths`, `parse_utc`): present."
    ));
}
