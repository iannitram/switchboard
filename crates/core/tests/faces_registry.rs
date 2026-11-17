use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use serde_json::Value;
use switchboard_core::faces::{CommandSpec, Effect, REGISTRY};

/// Reads a repo file relative to this crate (`crates/core`).
fn repo_file(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The names in `main.rs`'s `tauri::generate_handler![...]`, without `commands::`.
fn handler_names() -> Vec<String> {
    let main = repo_file("../../app/src-tauri/src/main.rs");
    let start = main
        .find("generate_handler![")
        .expect("main.rs has generate_handler![")
        + 18;
    let body = &main[start..start + main[start..].find(']').expect("generate_handler! closes")];
    body.split(',')
        .map(|s| s.trim().trim_start_matches("commands::").to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The subcommand words of `cli::USAGE`: the first word of each top-level `|` alternative
/// inside the `<sky | ... | mcp>` group.
fn usage_words() -> BTreeSet<String> {
    let cli = repo_file("../cli/src/cli.rs");
    let start = cli
        .find("pub const USAGE: &str = \"")
        .expect("cli.rs has USAGE")
        + 25;
    let raw = &cli[start..start + cli[start..].find("\";").expect("USAGE ends with \";")];
    let mut usage = String::new();
    let mut lines = raw.split('\n');
    usage.push_str(lines.next().unwrap_or_default().trim_end_matches('\\'));
    for line in lines {
        usage.push_str(line.trim_start().trim_end_matches('\\'));
    }
    let usage = usage.replace("\\\"", "\"");
    let group = &usage[usage.find("<sky").expect("USAGE lists <sky ...") + 1..];
    let (mut depth, mut words, mut current) = (0i32, BTreeSet::new(), String::new());
    for c in group.chars() {
        match c {
            '<' | '[' => depth += 1,
            '>' | ']' if depth == 0 => break,
            '>' | ']' => depth -= 1,
            '|' if depth == 0 => {
                words.insert(current.split_whitespace().next().unwrap_or("").to_string());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    words.insert(current.split_whitespace().next().unwrap_or("").to_string());
    words.remove("");
    words
}

// Catches: a duplicated id, or a row that declares no effect.
#[test]
fn ids_are_unique_and_each_row_declares_an_effect() {
    let mut ids = HashSet::new();
    for row in REGISTRY {
        assert!(ids.insert(row.id), "duplicate command id: {}", row.id);
        assert!(!row.effects.is_empty(), "{} has no declared effect", row.id);
    }
}

// Catches: a write row, or a Tauri/CLI-only row, leaking into the no-role MCP list.
#[test]
fn default_mcp_filter_is_read_only_and_role_free() {
    let default: Vec<&str> = REGISTRY
        .iter()
        .filter(|row| row.listed_without_role())
        .filter_map(|row| row.mcp)
        .collect();
    assert_eq!(
        default,
        [
            "sky",
            "constellation",
            "star",
            "waiting",
            "reports",
            "budget"
        ]
    );

    let fake = CommandSpec {
        id: "fake_write",
        effects: &[Effect::WriteLocal],
        tauri: None,
        cli: None,
        mcp: Some("fake_write"),
        ui_only: None,
        front_door: false,
        mcp_roles: &[],
        mcp_input: Some(|| Value::Null),
        mcp_description: Some("fake"),
    };
    assert!(
        !fake.listed_without_role(),
        "a write effect must not enter the default MCP list"
    );
}

// Catches: a parser that finds nothing, which would let both coverage tests pass vacuously.
#[test]
fn the_parsers_read_the_real_lists() {
    let names = handler_names();
    assert_eq!(names.len(), 21, "generate_handler! names: {names:?}");
    assert!(names.contains(&"views".to_string()) && names.contains(&"drop_guarded".to_string()));
    let words = usage_words();
    assert_eq!(words.len(), 24, "USAGE words: {words:?}");
    for w in [
        "sky",
        "ask_as_job",
        "confirm",
        "autopilot",
        "roadmap",
        "mcp",
    ] {
        assert!(words.contains(w), "USAGE parse is missing {w}: {words:?}");
    }
}

// Done when 1 (forward): catches a `tauri` value that is misspelled or not registered.
#[test]
fn every_tauri_value_is_in_generate_handler() {
    let names = handler_names();
    for row in REGISTRY {
        if let Some(t) = row.tauri {
            assert!(
                names.iter().any(|n| n == t),
                "row {} has tauri {t:?}, which generate_handler! does not list",
                row.id
            );
        }
    }
}

// Done when 1 (back): catches a Tauri command with no registry row, or two rows claiming one.
#[test]
fn every_generate_handler_name_has_exactly_one_row() {
    for name in handler_names() {
        let rows: Vec<&str> = REGISTRY
            .iter()
            .filter(|r| r.tauri == Some(name.as_str()))
            .map(|r| r.id)
            .collect();
        assert_eq!(
            rows.len(),
            1,
            "generate_handler! name {name:?} has rows {rows:?}; want exactly one"
        );
    }
}

// Done when 2: catches a USAGE subcommand word (other than mcp) with no row carrying that `cli`.
#[test]
fn every_usage_word_has_a_cli_row() {
    let missing: Vec<String> = usage_words()
        .into_iter()
        .filter(|w| w != "mcp" && !REGISTRY.iter().any(|r| r.cli == Some(w.as_str())))
        .collect();
    assert!(
        missing.is_empty(),
        "USAGE words with no registry row: {missing:?}"
    );
}

// Catches: a `cli` value that is not a USAGE word (a typo such as "pool-edit").
#[test]
fn every_cli_value_is_a_usage_word() {
    let words = usage_words();
    for row in REGISTRY {
        if let Some(c) = row.cli {
            assert!(
                words.contains(c),
                "row {} has cli {c:?}, which USAGE does not name",
                row.id
            );
        }
    }
}

// Catches: a CLI-only command given a Tauri command, or left without a row.
#[test]
fn cli_only_rows_have_no_tauri_command() {
    for word in ["confirm", "autopilot", "spec", "roadmap", "mcp"] {
        let rows: Vec<&CommandSpec> = REGISTRY.iter().filter(|r| r.cli == Some(word)).collect();
        assert!(!rows.is_empty(), "no row has cli {word:?}");
        for r in rows {
            assert_eq!(
                r.tauri, None,
                "row {} (cli {word:?}) must have tauri: None",
                r.id
            );
        }
    }
}

// Catches: a Tauri row mapped to the wrong CLI word or MCP tool, or an MCP row whose `tauri`
// was left empty (#489's pinned table).
#[test]
fn tauri_rows_match_the_489_table() {
    let table: &[(&str, Option<&str>, Option<&str>)] = &[
        ("views", None, None),
        ("settings_missing", Some("settings"), None),
        ("settings_get", Some("settings"), None),
        ("settings_save", None, None),
        ("fleet_errors", Some("errors"), Some("errors")),
        ("budget_rows", Some("budget"), Some("budget")),
        ("budget_refresh", Some("budget"), Some("budget_refresh")),
        ("budget_save_edit", Some("pool_edit"), Some("pool_edit")),
        ("reports", Some("reports"), Some("reports")),
        ("waiting", Some("waiting"), Some("waiting")),
        ("sky", Some("sky"), Some("sky")),
        ("chat_rate", Some("rate"), Some("rate")),
        ("loadout", Some("loadout"), None),
        ("chat_implicit", None, None),
        ("refresh_all", Some("refresh"), None),
        ("backup_repo", Some("backup"), None),
        ("backup_all", Some("backup"), None),
        ("chat_route", Some("ask"), Some("ask")),
        ("chat_send_as_job", Some("ask_as_job"), Some("ask_as_job")),
        ("confirm_guarded", None, None),
        ("drop_guarded", Some("drop"), Some("drop_request")),
    ];
    for &(tauri, cli, mcp) in table {
        let row = REGISTRY
            .iter()
            .find(|r| r.tauri == Some(tauri))
            .unwrap_or_else(|| panic!("no row has tauri {tauri:?}"));
        assert_eq!(
            (row.cli, row.mcp),
            (cli, mcp),
            "row {} (tauri {tauri:?}): (cli, mcp)",
            row.id
        );
    }
}

// Catches: a GUI-only row (no CLI word, no MCP tool) without its reason, or a reason on a row
// that does have a counterpart.
#[test]
fn ui_only_reason_is_set_exactly_on_gui_only_rows() {
    for row in REGISTRY.iter().filter(|r| r.tauri.is_some()) {
        let gui_only = row.cli.is_none() && row.mcp.is_none();
        assert_eq!(
            row.ui_only.is_some(),
            gui_only,
            "row {} (tauri {:?}): ui_only is {:?}",
            row.id,
            row.tauri,
            row.ui_only
        );
        if let Some(reason) = row.ui_only {
            assert!(
                !reason.trim().is_empty(),
                "row {} has an empty ui_only reason",
                row.id
            );
        }
    }
}
