//! FD-W (FD-9): the `chat_rate` command's core side. A face presses thumbs up or down on one
//! answer (#179) and sends the thread's handoff id, the turn's index, the answer's service and
//! model, and the route's kind; the core builds the rating row with `rating_row` (#133), gives it
//! the id `<handoff_id>#rating-<turn_index>` and appends it to the chat's thread (FD-13).
//! Locked tests (R28): the builder never edits this file.

use std::fs;
use std::path::{Path, PathBuf};
use switchboard_core::frontdoor::adapters::Answer;
use switchboard_core::frontdoor::rating_log::{
    log_rating, rating_entry, rating_id, thumbs_from_name,
};
use switchboard_core::frontdoor::router::learned_pick;
use switchboard_core::frontdoor::sticky::rating_row;
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::joblog::{read_all, JobRow, RowSource};
use switchboard_core::CoreError;

/// A fresh, empty data folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-fd9r-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// The rating row these tests expect, field by field; every other field stays `None`.
fn expected(
    id: &str,
    handoff: &str,
    service: &str,
    model: &str,
    kind: Option<&str>,
    up: bool,
) -> JobRow {
    JobRow {
        id: Some(id.to_string()),
        lane: Some(service.to_string()),
        model: Some(model.to_string()),
        kind: kind.map(str::to_string),
        handoff_id: Some(handoff.to_string()),
        rating: Some(if up { "up" } else { "down" }.to_string()),
        outcome: Some(if up { "passed" } else { "failed" }.to_string()),
        source: RowSource::Chat,
        implicit: false,
        moved: None,
        ..JobRow::default()
    }
}

fn refused(text: &str) -> Result<JobRow, CoreError> {
    Err(CoreError::Refused(text.to_string()))
}

// --- the rating a face sends: thumbs_from_name ---

#[test]
fn up_and_down_are_the_two_ratings() {
    assert_eq!(thumbs_from_name("up"), Some(true));
    assert_eq!(thumbs_from_name("down"), Some(false));
}

#[test]
fn any_other_text_is_no_rating() {
    for name in [
        "",
        "Up",
        "UP",
        "Down",
        " up",
        "up\n",
        "down ",
        "thumbs-up",
        "pass",
        "passed",
        "1",
        "true",
    ] {
        assert_eq!(thumbs_from_name(name), None, "{name:?}");
    }
}

// --- the row id: rating_id ---

#[test]
fn a_rating_id_names_the_thread_and_the_turn() {
    assert_eq!(rating_id("h1", 0), "h1#rating-0");
    assert_eq!(rating_id("handoff-7", 12), "handoff-7#rating-12");
    assert_eq!(rating_id("h1", u32::MAX), "h1#rating-4294967295");
}

// --- the row: rating_entry (no file is touched) ---

#[test]
fn a_thumbs_down_is_a_failed_rating_row_for_that_answer() {
    let row = rating_entry(
        "h1",
        3,
        "down",
        "gemini",
        "gemini-3-flash",
        Some("quick-lookup"),
    )
    .unwrap();
    assert_eq!(
        row,
        expected(
            "h1#rating-3",
            "h1",
            "gemini",
            "gemini-3-flash",
            Some("quick-lookup"),
            false
        )
    );
}

#[test]
fn a_thumbs_up_is_a_passed_rating_row_for_that_answer() {
    let row = rating_entry("handoff-7", 1, "up", "qwen", "qwen3-coder", Some("code")).unwrap();
    assert_eq!(
        row,
        expected(
            "handoff-7#rating-1",
            "handoff-7",
            "qwen",
            "qwen3-coder",
            Some("code"),
            true
        )
    );
    assert_eq!(row.rating.as_deref(), Some("up"));
    assert_eq!(row.outcome.as_deref(), Some("passed"));
    assert!(!row.implicit);
    assert_eq!(row.moved, None);
}

#[test]
fn no_kind_stays_unset() {
    let row = rating_entry("h1", 0, "up", "deepseek", "deepseek-chat", None).unwrap();
    assert_eq!(row.kind, None);
    assert_eq!(
        row,
        expected("h1#rating-0", "h1", "deepseek", "deepseek-chat", None, true)
    );
}

#[test]
fn the_row_is_rating_rows_row_with_the_turns_id() {
    let answer = Answer {
        text: "the answer the face showed".to_string(),
        service: "gemini".to_string(),
        model: "gemini-3-flash".to_string(),
        tokens_in: Some(10),
        tokens_out: Some(20),
        cost_usd: Some(0.5),
        handoff: Some("big".to_string()),
    };
    for (rating, up) in [("up", true), ("down", false)] {
        let row = rating_entry(
            "h9",
            4,
            rating,
            "gemini",
            "gemini-3-flash",
            Some("web-search"),
        )
        .unwrap();
        let mut want = rating_row(&answer, Some("web-search"), "h9", up);
        want.id = Some("h9#rating-4".to_string());
        assert_eq!(row, want, "{rating}");
    }
}

#[test]
fn an_unknown_rating_is_refused() {
    assert_eq!(
        rating_entry("h1", 0, "Up", "gemini", "gemini-3-flash", None),
        refused("unknown rating: Up")
    );
    assert_eq!(
        rating_entry("h1", 0, "", "gemini", "gemini-3-flash", None),
        refused("unknown rating: ")
    );
}

#[test]
fn no_handoff_id_service_or_model_is_refused() {
    assert_eq!(
        rating_entry("", 0, "up", "gemini", "gemini-3-flash", None),
        refused("rating with no handoff id")
    );
    assert_eq!(
        rating_entry("h1", 0, "up", "", "gemini-3-flash", None),
        refused("rating with no service")
    );
    assert_eq!(
        rating_entry("h1", 0, "up", "gemini", "", None),
        refused("rating with no model")
    );
}

#[test]
fn the_checks_run_rating_then_handoff_id_then_service_then_model() {
    assert_eq!(
        rating_entry("", 0, "nope", "", "", None),
        refused("unknown rating: nope")
    );
    assert_eq!(
        rating_entry("", 0, "down", "", "", None),
        refused("rating with no handoff id")
    );
    assert_eq!(
        rating_entry("h1", 0, "down", "", "", None),
        refused("rating with no service")
    );
}

#[test]
fn rating_rows_are_rated_chat_rows_the_learned_pick_counts() {
    let mut rows = Vec::new();
    for turn in 0..6 {
        rows.push(
            rating_entry(
                "h1",
                turn,
                "up",
                "gemini",
                "gemini-3-flash",
                Some("quick-lookup"),
            )
            .unwrap(),
        );
    }
    let read = read_all(&[], &rows);
    assert_eq!(read, rows);
    assert!(read
        .iter()
        .all(|r| r.source == RowSource::Chat && !r.implicit));
    assert_eq!(
        learned_pick("quick-lookup", &read),
        Some(("gemini".to_string(), "gemini-3-flash".to_string()))
    );
}

// --- logging: log_rating (the thread store, FD-13) ---

#[test]
fn a_press_appends_its_row_to_the_thread_and_returns_it() {
    let data = scratch("press");
    let row = log_rating(
        &data,
        "handoff-7",
        1,
        "down",
        "gemini",
        "gemini-3-flash",
        Some("quick-lookup"),
    )
    .unwrap();
    let want = expected(
        "handoff-7#rating-1",
        "handoff-7",
        "gemini",
        "gemini-3-flash",
        Some("quick-lookup"),
        false,
    );
    assert_eq!(row, want);
    assert_eq!(
        ThreadStore::new(&data).read("handoff-7").unwrap(),
        vec![want]
    );
}

#[test]
fn a_second_press_on_a_turn_is_appended_after_the_first() {
    let data = scratch("again");
    let first = log_rating(&data, "h3", 1, "down", "qwen", "qwen3-coder", None).unwrap();
    let other = log_rating(&data, "h3", 3, "up", "qwen", "qwen3-coder", None).unwrap();
    let last = log_rating(&data, "h3", 1, "up", "gemini", "gemini-3-flash", None).unwrap();
    // A second store over the same folder: a restart.
    let rows = ThreadStore::new(&data).read("h3").unwrap();
    assert_eq!(rows, vec![first, other, last.clone()]);
    assert_eq!(last.id.as_deref(), Some("h3#rating-1"));
    assert_eq!(rows[2].rating.as_deref(), Some("up"));
}

#[test]
fn threads_stay_apart() {
    let data = scratch("apart");
    log_rating(&data, "hb", 0, "up", "qwen", "qwen3-coder", None).unwrap();
    log_rating(&data, "ha", 0, "down", "qwen", "qwen3-coder", None).unwrap();
    let store = ThreadStore::new(&data);
    assert_eq!(
        store.list().unwrap(),
        vec!["ha".to_string(), "hb".to_string()]
    );
    assert_eq!(store.read("ha").unwrap().len(), 1);
    assert_eq!(store.read("hb").unwrap().len(), 1);
}

#[test]
fn a_refused_press_writes_nothing() {
    let data = scratch("refused");
    assert_eq!(
        log_rating(&data, "h4", 0, "sideways", "qwen", "qwen3-coder", None),
        refused("unknown rating: sideways")
    );
    assert_eq!(
        log_rating(&data, "", 0, "up", "qwen", "qwen3-coder", None),
        refused("rating with no handoff id")
    );
    assert_eq!(
        log_rating(&data, "h4", 0, "up", "", "qwen3-coder", None),
        refused("rating with no service")
    );
    assert_eq!(
        log_rating(&data, "h4", 0, "up", "qwen", "", None),
        refused("rating with no model")
    );
    assert!(!data.join("threads").exists());
}

#[test]
fn a_handoff_id_that_escapes_the_folder_is_refused() {
    let data = scratch("escape");
    let got = log_rating(&data, "../x", 0, "up", "qwen", "qwen3-coder", None);
    assert!(matches!(got, Err(CoreError::Refused(_))), "{got:?}");
    assert!(!data.join("x.jsonl").exists());
}

// --- the shell registers the command; the code map says so ---

fn shell(file: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../app/src-tauri/src")
            .join(file),
    )
    .unwrap()
}

#[test]
fn the_shell_registers_the_chat_rate_command() {
    let main = shell("main.rs");
    let handler = main
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("generate_handler! in main.rs");
    assert!(handler.contains("commands::chat_rate,"), "{handler}");
}

#[test]
fn the_chat_rate_command_is_a_thin_call_into_log_rating() {
    let commands = shell("commands.rs");
    let at = commands
        .find("pub fn chat_rate(")
        .expect("commands.rs has `pub fn chat_rate(`");
    let before = &commands[..at];
    let attr = before.rfind("#[tauri::command]").expect("an attribute");
    assert!(
        !before[attr..].contains("fn "),
        "#[tauri::command] is not on chat_rate"
    );
    let end = commands[at..]
        .find("\n}\n")
        .map_or(commands.len(), |i| at + i);
    let body = &commands[at..end];
    for part in [
        "handoff_id: String",
        "turn_index: u32",
        "rating: String",
        "service: String",
        "model: String",
        "kind: Option<String>",
        "Result<(), FaceError>",
        "app_data_dir()",
        "switchboard_core::frontdoor::rating_log::log_rating(",
        "FaceError::new(\"chat_rate\"",
    ] {
        assert!(body.contains(part), "chat_rate lacks `{part}`: {body}");
    }
}

#[test]
fn the_chat_rate_command_sits_after_record_fleet_error_with_no_new_use_line() {
    let commands = shell("commands.rs");
    let helper = commands
        .find("pub fn record_fleet_error(")
        .expect("commands.rs has `pub fn record_fleet_error(`");
    let at = commands
        .find("pub fn chat_rate(")
        .expect("`pub fn chat_rate(`");
    let next = commands
        .find("pub fn budget_rows(")
        .expect("commands.rs has `pub fn budget_rows(`");
    assert!(
        helper < at && at < next,
        "chat_rate is not between record_fleet_error and budget_rows"
    );
    assert!(
        !commands
            .lines()
            .any(|l| l.starts_with("use ") && l.contains("rating_log")),
        "a `use` line for rating_log"
    );
}

#[test]
fn the_code_map_marks_the_command_present() {
    let map =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md"))
            .unwrap();
    let frontdoor = map
        .split("### frontdoor")
        .nth(1)
        .and_then(|rest| rest.split("\n### ").next())
        .expect("a `### frontdoor` section in docs/code-map.md");
    assert!(
        frontdoor.contains(
            "- FD-9 thumbs rating log (`thumbs_from_name`, `rating_entry`, `log_rating`, `chat_rate`): present."
        ),
        "{frontdoor}"
    );
}
