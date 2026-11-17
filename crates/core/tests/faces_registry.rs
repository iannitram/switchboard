use std::collections::HashSet;

use serde_json::Value;
use switchboard_core::faces::{CommandSpec, Effect, REGISTRY};

#[test]
fn ids_are_unique_and_each_row_declares_an_effect() {
    let mut ids = HashSet::new();
    for row in REGISTRY {
        assert!(ids.insert(row.id), "duplicate command id: {}", row.id);
        assert!(!row.effects.is_empty(), "{} has no declared effect", row.id);
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
        mcp: Some("fake_write"),
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
