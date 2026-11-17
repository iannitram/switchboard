//! Face parity: each registered Tauri command has a CLI or MCP counterpart or a UI-only reason.
//! Locked (R28). After storytold/craft-anything at 2e7f276,
//! `crates/ui-egui/tests/headless_parity.rs` (MIT OR Apache-2.0).

use std::path::{Path, PathBuf};

use switchboard_core::faces::REGISTRY;
use switchboard_core::parity::{
    face_call_gaps, face_calls, handler_names, problems, read_handlers, usage_has,
    usage_subcommands, Row,
};

const MAIN_RS: &str = r#"
fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::sky,
            commands::backup_repo,
            settings_save,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Switchboard");
}
"#;

const FACE_TS: &str = r#"
export async function call<T>(command: string): Promise<T> { return invoke<T>(command); }
export const sky = () => call<SkyRow[]>("sky");
export const backup = (repo: string) => call("backup_repo", { repo });
export const save = () => call<Record<string, Row[]>>( "settings_save");
const r = recall("not_a_call");
const f = fn.call(this, "not_either");
export const again = () => call("sky");
"#;

const USAGE: &str = "usage: switchboard [--json] [--pool <p>] <sky | backup <repo> | \
                     backup --all | ask \"<text>\" | ask_as_job <kind> \"<text>\" | \
                     pool_edit <pool> [--cap <n>]>";

const ROWS: [Row<'static>; 3] = [
    Row {
        id: "sky",
        tauri: Some("sky"),
        cli: Some("sky"),
        mcp: Some("sky"),
        ui_only: None,
    },
    Row {
        id: "backup_repo",
        tauri: Some("backup_repo"),
        cli: Some("backup"),
        mcp: None,
        ui_only: None,
    },
    Row {
        id: "settings_save",
        tauri: Some("settings_save"),
        cli: None,
        mcp: None,
        ui_only: Some("settings are changed in the app only (#373)"),
    },
];

const MCP: [&str; 1] = ["sky"];

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

fn handlers() -> Vec<String> {
    strings(&["sky", "backup_repo", "settings_save"])
}

fn calls() -> Vec<String> {
    strings(&["sky", "backup_repo", "settings_save", "sky"])
}

#[test]
fn handler_names_reads_the_generate_handler_list_in_order_without_paths() {
    assert_eq!(handler_names(MAIN_RS), handlers());
}

#[test]
fn handler_names_is_empty_without_a_generate_handler_list() {
    assert_eq!(handler_names("fn main() {}"), Vec::<String>::new());
}

#[test]
fn read_handlers_names_a_missing_or_open_list() {
    assert_eq!(read_handlers(MAIN_RS), Ok(handlers()));
    assert_eq!(
        read_handlers("fn main() {}"),
        Err("main.rs has no generate_handler![ list".to_string())
    );
    assert_eq!(
        read_handlers("generate_handler![commands::sky,"),
        Err("main.rs's generate_handler![ list has no closing ]".to_string())
    );
}

#[test]
fn face_calls_finds_each_call_with_a_string_name_in_order() {
    assert_eq!(face_calls(FACE_TS), calls());
}

#[test]
fn face_call_gaps_skip_the_definition_and_method_calls() {
    assert_eq!(face_call_gaps(FACE_TS), Vec::<usize>::new());
}

#[test]
fn face_call_gaps_name_each_call_without_a_string_name_by_line() {
    let ts = "const a = () => call(command);\n\
              const b = () => call(`sky`);\n\
              const c = () => call<Record<string, () => void>>(name);\n\
              const d = () => call<Record<string, () => void>>(\"sky\");\n\
              const e = () => call<Open(\"sky\");\n";
    assert_eq!(face_call_gaps(ts), vec![1, 2, 3, 5]);
    assert_eq!(face_calls(ts), strings(&["sky"]));
}

#[test]
fn usage_has_matches_whole_words_only() {
    for word in ["sky", "backup", "ask", "ask_as_job", "pool_edit"] {
        assert!(usage_has(USAGE, word), "{word} is a subcommand");
    }
    for word in ["pool", "kind", "all", "edit", "as", "text", "cap"] {
        assert!(!usage_has(USAGE, word), "{word} is not a subcommand");
    }
}

#[test]
fn usage_subcommands_are_the_top_level_alternatives() {
    assert_eq!(
        usage_subcommands(USAGE),
        strings(&[
            "sky",
            "backup <repo>",
            "backup --all",
            "ask \"<text>\"",
            "ask_as_job <kind> \"<text>\"",
            "pool_edit <pool> [--cap <n>]",
        ])
    );
    assert_eq!(
        usage_subcommands("x <a | \\\n    b <c | d>>"),
        strings(&["a", "b <c | d>"])
    );
}

#[test]
fn a_subcommand_word_left_only_as_an_argument_is_named() {
    let usage = USAGE.replace("<sky | ", "<drop <sky | backup> | ");
    assert!(usage_has(&usage, "sky"));
    assert_eq!(
        problems(&ROWS, &handlers(), &calls(), &usage, &MCP),
        strings(&["sky: CLI subcommand sky is missing"])
    );
}

#[test]
fn a_usage_line_with_no_subcommand_list_is_named() {
    assert_eq!(
        problems(
            &ROWS,
            &handlers(),
            &calls(),
            "usage: switchboard [--json]",
            &MCP
        ),
        strings(&[
            "the CLI usage has no <a | b> subcommand list",
            "sky: CLI subcommand sky is missing",
            "backup_repo: CLI subcommand backup is missing",
        ])
    );
}

#[test]
fn matching_faces_have_no_problems() {
    assert_eq!(
        problems(&ROWS, &handlers(), &calls(), USAGE, &MCP),
        Vec::<String>::new()
    );
}

#[test]
fn a_row_with_no_tauri_command_is_skipped() {
    let mut rows = ROWS.to_vec();
    rows.push(Row {
        id: "spec_validate",
        tauri: None,
        cli: None,
        mcp: None,
        ui_only: None,
    });
    assert_eq!(
        problems(&rows, &handlers(), &calls(), USAGE, &MCP),
        Vec::<String>::new()
    );
}

#[test]
fn a_tauri_command_with_no_row_is_named() {
    let handlers = strings(&["sky", "backup_repo", "settings_save", "brand_new"]);
    assert_eq!(
        problems(&ROWS, &handlers, &calls(), USAGE, &MCP),
        strings(&["tauri command brand_new has no row"])
    );
}

#[test]
fn a_face_call_nothing_handles_is_named_once() {
    let mut calls = calls();
    calls.extend(strings(&["ghost", "ghost"]));
    assert_eq!(
        problems(&ROWS, &handlers(), &calls, USAGE, &MCP),
        strings(&["face calls ghost, which no tauri command handles"])
    );
}

#[test]
fn a_row_with_no_counterpart_and_no_reason_is_named() {
    let mut rows = ROWS;
    rows[1].cli = None;
    assert_eq!(
        problems(&rows, &handlers(), &calls(), USAGE, &MCP),
        strings(&["backup_repo has no CLI or MCP counterpart and no UI-only reason"])
    );
}

#[test]
fn a_blank_ui_only_reason_is_named() {
    let mut rows = ROWS;
    rows[2].ui_only = Some("  ");
    assert_eq!(
        problems(&rows, &handlers(), &calls(), USAGE, &MCP),
        strings(&["settings_save is UI-only with an empty reason"])
    );
}

#[test]
fn a_removed_cli_subcommand_is_named() {
    let usage = USAGE.replace("backup <repo> | backup --all | ", "");
    assert_eq!(
        problems(&ROWS, &handlers(), &calls(), &usage, &MCP),
        strings(&["backup_repo: CLI subcommand backup is missing"])
    );
}

#[test]
fn a_missing_mcp_tool_is_named() {
    assert_eq!(
        problems(&ROWS, &handlers(), &calls(), USAGE, &[]),
        strings(&["sky: MCP tool sky is missing"])
    );
}

#[test]
fn a_row_for_an_unregistered_tauri_command_is_named() {
    let handlers = strings(&["sky", "backup_repo"]);
    let calls = strings(&["sky", "backup_repo"]);
    assert_eq!(
        problems(&ROWS, &handlers, &calls, USAGE, &MCP),
        strings(&["row settings_save names tauri command settings_save, which is not registered"])
    );
}

#[test]
fn problems_come_in_step_order() {
    let mut rows = ROWS;
    rows[1].cli = None;
    let handlers = strings(&["sky", "backup_repo", "settings_save", "brand_new"]);
    let mut calls = calls();
    calls.push("ghost".to_string());
    let usage = USAGE.replace("<sky | ", "<");
    assert_eq!(
        problems(&rows, &handlers, &calls, &usage, &[]),
        strings(&[
            "tauri command brand_new has no row",
            "face calls ghost, which no tauri command handles",
            "sky: CLI subcommand sky is missing",
            "sky: MCP tool sky is missing",
            "backup_repo has no CLI or MCP counterpart and no UI-only reason",
        ])
    );
}

// The real repo: handler list, face calls, CLI usage and the registry agree.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn real_usage() -> String {
    let cli = read("crates/cli/src/cli.rs");
    let start = cli
        .find("pub const USAGE: &str = \"")
        .expect("USAGE in cli.rs");
    let rest = &cli[start..];
    rest[..rest.find("\";").expect("end of USAGE")].to_string()
}

/// Each window source file under app/src (not tests), as (path from the repo root, text).
fn face_files(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).expect("read a face folder") {
        let path = entry.expect("a folder entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            face_files(&path, out);
        } else if (name.ends_with(".ts") || name.ends_with(".tsx")) && !name.contains(".test.") {
            let rel = path.strip_prefix(repo()).unwrap_or(&path);
            let rel = rel.to_string_lossy().replace('\\', "/");
            out.push((
                rel,
                std::fs::read_to_string(&path).expect("read a face file"),
            ));
        }
    }
}

fn real_face_files() -> Vec<(String, String)> {
    let mut files = Vec::new();
    face_files(&repo().join("app/src"), &mut files);
    files
}

fn calls_in(files: &[(String, String)]) -> Vec<String> {
    let calls: Vec<String> = files.iter().flat_map(|(_, ts)| face_calls(ts)).collect();
    // A moved app/src or a changed call() shape must not pass by finding nothing.
    assert!(
        calls.len() >= 10,
        "the window's face calls were found: {calls:?}"
    );
    calls
}

/// The call sites this test cannot read, by file and line.
fn gaps_in(files: &[(String, String)]) -> Vec<String> {
    files
        .iter()
        .flat_map(|(path, ts)| {
            face_call_gaps(ts)
                .into_iter()
                .map(move |line| format!("{path}:{line}: call() has no string command name"))
        })
        .collect()
}

/// Every registry MCP name. Which of them tools/list really serves is checked against the
/// served list itself in crates/cli/tests/face_parity_mcp.rs (core cannot run the MCP face).
fn registry_mcp_names() -> Vec<&'static str> {
    REGISTRY.iter().filter_map(|s| s.mcp).collect()
}

fn real_rows() -> Vec<Row<'static>> {
    REGISTRY
        .iter()
        .map(|s| Row {
            id: s.id,
            tauri: s.tauri,
            cli: s.cli,
            mcp: s.mcp,
            ui_only: s.ui_only,
        })
        .collect()
}

fn problems_in(main_rs: &str, usage: &str, files: &[(String, String)]) -> Vec<String> {
    let rows = real_rows();
    let mut got = gaps_in(files);
    let handlers = read_handlers(main_rs).unwrap_or_else(|e| {
        got.push(e);
        Vec::new()
    });
    got.extend(problems(
        &rows,
        &handlers,
        &calls_in(files),
        usage,
        &registry_mcp_names(),
    ));
    got
}

fn real_problems(main_rs: &str, usage: &str) -> Vec<String> {
    problems_in(main_rs, usage, &real_face_files())
}

#[test]
fn the_real_faces_have_no_parity_problems() {
    let main_rs = read("app/src-tauri/src/main.rs");
    assert!(
        read_handlers(&main_rs)
            .expect("the window's handler list")
            .len()
            >= 21,
        "the window registers its commands"
    );
    assert_eq!(real_problems(&main_rs, &real_usage()), Vec::<String>::new());
}

#[test]
fn a_new_tauri_command_with_no_row_fails_by_name() {
    let main_rs = read("app/src-tauri/src/main.rs").replacen(
        "generate_handler![",
        "generate_handler![\n            commands::brand_new_command,",
        1,
    );
    assert_eq!(
        real_problems(&main_rs, &real_usage()),
        strings(&["tauri command brand_new_command has no row"])
    );
}

#[test]
fn a_moved_handler_list_fails_with_its_cause_first() {
    let main_rs = read("app/src-tauri/src/main.rs").replace("generate_handler![", "handlers![");
    let got = real_problems(&main_rs, &real_usage());
    assert_eq!(
        got.first().map(String::as_str),
        Some("main.rs has no generate_handler![ list"),
        "{got:?}"
    );
}

#[test]
fn removing_a_cli_subcommand_fails_with_its_name() {
    let usage = real_usage();
    assert!(usage.contains("backup <repo> | backup --all | "));
    let got = real_problems(
        &read("app/src-tauri/src/main.rs"),
        &usage.replace("backup <repo> | backup --all | ", ""),
    );
    assert!(
        got.contains(&"backup_repo: CLI subcommand backup is missing".to_string()),
        "{got:?}"
    );
}

#[test]
fn a_face_call_without_a_string_name_fails_by_file_and_line() {
    let mut files = real_face_files();
    let (path, ts) = files
        .iter_mut()
        .find(|(_, ts)| !face_calls(ts).is_empty())
        .expect("a face file with calls");
    ts.push_str("\nexport const dynamic = (name: string) => call(name);\n");
    let line = ts.lines().count();
    let want = format!("{path}:{line}: call() has no string command name");
    assert_eq!(
        problems_in(&read("app/src-tauri/src/main.rs"), &real_usage(), &files),
        vec![want]
    );
}
