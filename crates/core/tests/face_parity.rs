//! Face parity: each registered Tauri command has a CLI or MCP counterpart or a UI-only reason.
//! Locked (R28). After storytold/craft-anything at 2e7f276,
//! `crates/ui-egui/tests/headless_parity.rs` (MIT OR Apache-2.0).

use std::path::{Path, PathBuf};

use switchboard_core::faces::REGISTRY;
use switchboard_core::parity::{face_calls, handler_names, problems, usage_has, Row};

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
fn face_calls_finds_each_call_with_a_string_name_in_order() {
    assert_eq!(face_calls(FACE_TS), calls());
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

fn face_sources(dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("read a face folder") {
        let path = entry.expect("a folder entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            face_sources(&path, out);
        } else if (name.ends_with(".ts") || name.ends_with(".tsx")) && !name.contains(".test.") {
            out.push(std::fs::read_to_string(&path).expect("read a face file"));
        }
    }
}

fn real_calls() -> Vec<String> {
    let mut sources = Vec::new();
    face_sources(&repo().join("app/src"), &mut sources);
    let calls: Vec<String> = sources.iter().flat_map(|s| face_calls(s)).collect();
    // A moved app/src or a changed call() shape must not pass by finding nothing.
    assert!(
        calls.len() >= 10,
        "the window's face calls were found: {calls:?}"
    );
    calls
}

/// The MCP face's served tools. `tools()` in crates/cli/src/mcp.rs builds tools/list from
/// REGISTRY, so a served tool is a registry MCP name that some caller may list: role-free
/// reads by default, anything else for the roles it names. A tool no caller can list is missing.
fn real_mcp_tools() -> Vec<&'static str> {
    // A CRLF checkout (Windows CI) reads the same as LF.
    let mcp_rs = read("crates/cli/src/mcp.rs").replace("\r\n", "\n");
    let start = mcp_rs.find("fn tools(").expect("tools() in mcp.rs");
    let body = &mcp_rs[start..];
    let body = &body[..body.find("\n}\n").expect("end of tools()")];
    assert!(
        body.contains("REGISTRY") && body.contains("spec.mcp"),
        "tools/list is no longer built from REGISTRY: read the served names from mcp.rs here"
    );
    REGISTRY
        .iter()
        .filter(|s| s.listed_without_role() || !s.mcp_roles.is_empty())
        .filter_map(|s| s.mcp)
        .collect()
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

fn real_problems(main_rs: &str, usage: &str) -> Vec<String> {
    let rows = real_rows();
    problems(
        &rows,
        &handler_names(main_rs),
        &real_calls(),
        usage,
        &real_mcp_tools(),
    )
}

#[test]
fn the_real_faces_have_no_parity_problems() {
    let main_rs = read("app/src-tauri/src/main.rs");
    assert!(
        handler_names(&main_rs).len() >= 21,
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
fn an_mcp_tool_no_caller_can_list_fails_by_name() {
    let rows = real_rows();
    let row = rows
        .iter()
        .find(|r| r.tauri.is_some() && r.mcp.is_some())
        .expect("a window command with an MCP tool");
    let served: Vec<&str> = real_mcp_tools()
        .into_iter()
        .filter(|t| Some(*t) != row.mcp)
        .collect();
    let main_rs = read("app/src-tauri/src/main.rs");
    let got = problems(
        &rows,
        &handler_names(&main_rs),
        &real_calls(),
        &real_usage(),
        &served,
    );
    assert_eq!(
        got,
        vec![format!(
            "{}: MCP tool {} is missing",
            row.tauri.unwrap_or_default(),
            row.mcp.unwrap_or_default()
        )]
    );
}
