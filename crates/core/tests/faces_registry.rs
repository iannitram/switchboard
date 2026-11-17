use std::collections::HashSet;

use serde_json::Value;
use switchboard_core::faces::{CommandSpec, Effect, REGISTRY};

const TAURI_MAIN: &str = include_str!("../../../app/src-tauri/src/main.rs");
const CLI_USAGE: &str = include_str!("../../cli/src/cli.rs");

#[test]
fn ids_are_unique_and_each_row_declares_an_effect() {
    let mut ids = HashSet::new();
    for row in REGISTRY {
        assert!(ids.insert(row.id), "duplicate command id: {}", row.id);
        assert!(!row.effects.is_empty(), "{} has no declared effect", row.id);
    }
}

#[test]
fn tauri_handler_and_registry_names_match() {
    let body = TAURI_MAIN
        .split("tauri::generate_handler![")
        .nth(1)
        .expect("generate_handler! exists")
        .split("])")
        .next()
        .expect("generate_handler! closes");
    let handlers: HashSet<&str> = body
        .split(',')
        .filter_map(|item| item.trim().strip_prefix("commands::"))
        .collect();
    let registered: HashSet<&str> = REGISTRY.iter().filter_map(|row| row.tauri).collect();
    for name in &handlers {
        assert!(
            registered.contains(name),
            "Tauri handler {name} has no registry row"
        );
    }
    for name in &registered {
        assert!(
            handlers.contains(name),
            "registry Tauri command {name} is not registered"
        );
    }
    assert_eq!(handlers.len(), 21);
}

#[test]
fn every_cli_usage_word_except_mcp_has_a_registry_row() {
    let usage = CLI_USAGE
        .split("pub const USAGE:")
        .nth(1)
        .unwrap()
        .split(";\n")
        .next()
        .unwrap();
    let alternatives = usage.split_once("<sky | ").unwrap().1;
    let mut depth = 1usize;
    let mut start = 0usize;
    let mut commands = vec!["sky"];
    for (index, ch) in alternatives.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    commands.push(&alternatives[start..index]);
                    break;
                }
            }
            '|' if depth == 1 => {
                commands.push(&alternatives[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    for alternative in commands {
        let Some(command) = alternative
            .split_whitespace()
            .next()
            .map(|word| word.trim_matches('\\'))
        else {
            continue;
        };
        if command.is_empty() {
            continue;
        }
        if command == "mcp" {
            continue;
        }
        assert!(
            REGISTRY.iter().any(|row| row.cli == Some(command)),
            "CLI command {command} has no registry row"
        );
    }
}

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
        mcp_roles: &[],
        mcp_input: Some(|| Value::Null),
        mcp_description: Some("fake"),
    };
    assert!(
        !fake.listed_without_role(),
        "a write effect must not enter the default MCP list"
    );
}
