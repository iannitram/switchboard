//! FD-W (FD-9): the `chat_implicit` command's core side. A face names an implicit signal
//! (`opened-in-claude`, `re-asked`, `abandoned`, `copied-answer`); the core builds its row with
//! `implicit_row` (#133) and appends it to the chat's thread in the thread store (FD-13).
//! Locked tests (R28): the builder never edits this file.

use std::fs;
use std::path::PathBuf;
use switchboard_core::frontdoor::implicit_log::{implicit_from_name, log_implicit};
use switchboard_core::frontdoor::sticky::{implicit_row, Implicit};
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::joblog::{read_all, RowSource};
use switchboard_core::CoreError;

/// A fresh, empty data folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-fd9w-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

const ALL: [Implicit; 4] = [
    Implicit::OpenedInClaude,
    Implicit::ReAsked,
    Implicit::Abandoned,
    Implicit::CopiedAnswer,
];

// --- the names a face sends: implicit_from_name ---

#[test]
fn each_signal_is_found_by_its_name() {
    for signal in ALL {
        assert_eq!(implicit_from_name(signal.name()), Some(signal));
    }
    assert_eq!(
        implicit_from_name("opened-in-claude"),
        Some(Implicit::OpenedInClaude)
    );
    assert_eq!(
        implicit_from_name("copied-answer"),
        Some(Implicit::CopiedAnswer)
    );
}

#[test]
fn any_other_text_is_no_signal() {
    for name in [
        "",
        "Opened-In-Claude",
        "opened_in_claude",
        "OpenedInClaude",
        " opened-in-claude",
        "opened-in-claude\n",
        "thumbs-up",
    ] {
        assert_eq!(implicit_from_name(name), None, "{name:?}");
    }
}

// --- logging: log_implicit ---

#[test]
fn a_press_appends_the_implicit_row_to_its_thread_and_returns_it() {
    let data = scratch("press");
    let row = log_implicit(
        &data,
        "handoff-7",
        "opened-in-claude",
        Some("claude"),
        None,
        Some("judgement-planning"),
    )
    .unwrap();
    let expected = implicit_row(
        Implicit::OpenedInClaude,
        Some("claude"),
        None,
        Some("judgement-planning"),
        "handoff-7",
    );
    assert_eq!(row, expected);
    assert_eq!(
        row.id.as_deref(),
        Some("handoff-7#implicit-opened-in-claude")
    );
    assert!(row.implicit);
    assert_eq!(row.rating, None);
    assert_eq!(row.outcome, None);
    assert_eq!(row.model, None);
    assert_eq!(
        ThreadStore::new(&data).read("handoff-7").unwrap(),
        vec![expected]
    );
}

#[test]
fn unknown_service_model_and_kind_stay_unset() {
    let data = scratch("bare");
    let row = log_implicit(&data, "h1", "abandoned", None, None, None).unwrap();
    assert_eq!(
        row,
        implicit_row(Implicit::Abandoned, None, None, None, "h1")
    );
    assert_eq!((row.lane, row.model, row.kind), (None, None, None));
    assert_eq!(row.handoff_id.as_deref(), Some("h1"));
}

#[test]
fn each_signal_logs_under_its_own_name() {
    let data = scratch("all");
    for signal in ALL {
        let row = log_implicit(
            &data,
            "h2",
            signal.name(),
            Some("gemini"),
            Some("flash"),
            None,
        )
        .unwrap();
        assert_eq!(
            row.id,
            Some(format!("h2#implicit-{}", signal.name())),
            "{signal:?}"
        );
    }
    let ids: Vec<Option<String>> = ThreadStore::new(&data)
        .read("h2")
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(
        ids,
        vec![
            Some("h2#implicit-opened-in-claude".to_string()),
            Some("h2#implicit-re-asked".to_string()),
            Some("h2#implicit-abandoned".to_string()),
            Some("h2#implicit-copied-answer".to_string()),
        ]
    );
}

#[test]
fn later_rows_are_appended_after_the_rows_already_there() {
    let data = scratch("append");
    let store = ThreadStore::new(&data);
    let earlier = implicit_row(Implicit::ReAsked, Some("qwen"), None, None, "h3");
    store.append("h3", &earlier).unwrap();
    let pressed = log_implicit(&data, "h3", "opened-in-claude", None, None, None).unwrap();
    // A second store over the same folder: a restart.
    assert_eq!(
        ThreadStore::new(&data).read("h3").unwrap(),
        vec![earlier, pressed]
    );
}

#[test]
fn threads_stay_apart() {
    let data = scratch("apart");
    log_implicit(&data, "ha", "copied-answer", None, None, None).unwrap();
    log_implicit(&data, "hb", "opened-in-claude", None, None, None).unwrap();
    let store = ThreadStore::new(&data);
    assert_eq!(
        store.list().unwrap(),
        vec!["ha".to_string(), "hb".to_string()]
    );
    assert_eq!(store.read("ha").unwrap().len(), 1);
    assert_eq!(store.read("hb").unwrap().len(), 1);
}

#[test]
fn an_unknown_signal_is_refused_and_writes_nothing() {
    let data = scratch("unknown");
    assert_eq!(
        log_implicit(&data, "h4", "Opened-In-Claude", None, None, None),
        Err(CoreError::Refused(
            "unknown implicit signal: Opened-In-Claude".to_string()
        ))
    );
    assert_eq!(
        log_implicit(&data, "h4", "", None, None, None),
        Err(CoreError::Refused("unknown implicit signal: ".to_string()))
    );
    assert!(!data.join("threads").exists());
}

#[test]
fn no_handoff_id_is_refused_and_writes_nothing() {
    let data = scratch("no-id");
    assert_eq!(
        log_implicit(&data, "", "opened-in-claude", None, None, None),
        Err(CoreError::Refused(
            "implicit signal with no handoff id".to_string()
        ))
    );
    assert!(!data.join("threads").exists());
}

#[test]
fn the_signal_is_checked_before_the_handoff_id() {
    let data = scratch("both-bad");
    assert_eq!(
        log_implicit(&data, "", "nope", None, None, None),
        Err(CoreError::Refused(
            "unknown implicit signal: nope".to_string()
        ))
    );
}

#[test]
fn a_handoff_id_that_escapes_the_folder_is_refused() {
    let data = scratch("escape");
    let got = log_implicit(&data, "../x", "opened-in-claude", None, None, None);
    assert!(matches!(got, Err(CoreError::Refused(_))), "{got:?}");
    assert!(!data.join("x.jsonl").exists());
}

#[test]
fn the_logged_row_reads_back_as_an_implicit_chat_row() {
    let data = scratch("joblog");
    log_implicit(&data, "h5", "opened-in-claude", Some("claude"), None, None).unwrap();
    let rows = read_all(&[], &ThreadStore::new(&data).read("h5").unwrap());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source, RowSource::Chat);
    assert!(rows[0].implicit);
    assert_eq!(rows[0].lane.as_deref(), Some("claude"));
}

// --- the shell registers the command; the code map says so ---

#[test]
fn the_shell_registers_the_chat_implicit_command() {
    let main = include_str!("../../../app/src-tauri/src/main.rs");
    let handler = main
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("generate_handler! in main.rs");
    assert!(handler.contains("commands::chat_implicit,"), "{handler}");
}

#[test]
fn the_chat_implicit_command_is_a_thin_call_into_log_implicit() {
    let commands = include_str!("../../../app/src-tauri/src/commands.rs");
    let at = commands
        .find("pub fn chat_implicit(")
        .expect("commands.rs has `pub fn chat_implicit(`");
    let before = &commands[..at];
    let attr = before.rfind("#[tauri::command]").expect("an attribute");
    assert!(
        !before[attr..].contains("fn "),
        "#[tauri::command] is not on chat_implicit"
    );
    let end = commands[at..]
        .find("\n}\n")
        .map_or(commands.len(), |i| at + i);
    let body = &commands[at..end];
    for part in [
        "handoff_id: String",
        "signal: String",
        "service: Option<String>",
        "model: Option<String>",
        "kind: Option<String>",
        "Result<(), FaceError>",
        "app_data_dir()",
        "log_implicit(",
        "FaceError::new(\"chat_implicit\"",
    ] {
        assert!(body.contains(part), "chat_implicit lacks `{part}`: {body}");
    }
}

#[test]
fn the_code_map_marks_the_command_present() {
    let map = include_str!("../../../docs/code-map.md");
    let frontdoor = map
        .split("### frontdoor")
        .nth(1)
        .and_then(|rest| rest.split("\n### ").next())
        .expect("a `### frontdoor` section in docs/code-map.md");
    assert!(
        frontdoor.contains(
            "- FD-9 implicit signal log (`implicit_from_name`, `log_implicit`, `chat_implicit`): present."
        ),
        "{frontdoor}"
    );
}
