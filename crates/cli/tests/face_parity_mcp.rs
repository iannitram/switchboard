//! Face parity, MCP side (#490). Locked (R28). Each registry row with a window command and an
//! MCP tool must be served by the real `tools/list` to some caller: no role, or any role.
//! The core test (crates/core/tests/face_parity.rs) checks the window, CLI and face calls.

// The binary crate has no library target, so the module is compiled in directly.
#[path = "../src/mcp.rs"]
#[allow(dead_code)]
mod mcp;

use std::io::Cursor;

use mcp::{serve_with, ReadCore, WriteCore};
use serde_json::{json, Value};
use switchboard_core::budget::PoolEdit;
use switchboard_core::faces::REGISTRY;
use switchboard_core::parity::{problems, Row};
use switchboard_core::rules::Role;

/// tools/list calls no core, so every call here is a bug.
struct Never;

impl ReadCore for Never {
    fn call(&self, _: &str, _: &Value) -> Result<Value, String> {
        unreachable!("tools/list calls no tool")
    }
}

impl WriteCore for Never {
    fn merge(&self, _: &str, _: u64) -> Result<(), String> {
        unreachable!("tools/list calls no tool")
    }
    fn pool_edit(&self, _: &str, _: &PoolEdit) -> Result<String, String> {
        unreachable!("tools/list calls no tool")
    }
    fn budget_refresh(&self) -> Result<Value, String> {
        unreachable!("tools/list calls no tool")
    }
    fn waiting(&self, _: &str) -> Result<(), String> {
        unreachable!("tools/list calls no tool")
    }
}

/// Every role. The match fails to build when a role is added, so it cannot be left out here.
fn every_role() -> [Role; 3] {
    let roles = [Role::ProjectManager, Role::BudgetManager, Role::Satellite];
    for role in &roles {
        match role {
            Role::ProjectManager | Role::BudgetManager | Role::Satellite => {}
        }
    }
    roles
}

/// The tool names the real server answers tools/list with, for one caller.
fn listed(role: Option<Role>) -> Vec<String> {
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
    let mut output = Vec::new();
    serve_with(
        Cursor::new(format!("{request}\n")),
        &mut output,
        &Never,
        role,
        &Never,
        false,
    )
    .expect("serve tools/list");
    let reply: Value = serde_json::from_slice(&output).expect("one JSON reply");
    reply["result"]["tools"]
        .as_array()
        .expect("tools/list answers with tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("a tool name").to_string())
        .collect()
}

/// Every tool some caller is served, in first-seen order.
fn served() -> Vec<String> {
    let callers = std::iter::once(None).chain(every_role().into_iter().map(Some));
    let mut names: Vec<String> = Vec::new();
    for name in callers.flat_map(listed) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    assert!(names.len() >= 6, "tools/list serves tools: {names:?}");
    names
}

fn rows() -> Vec<Row<'static>> {
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

/// The MCP problems for the rows against `served`. The window, CLI and face-call checks are
/// the core test's, so only the MCP lines are kept.
fn mcp_problems(served: &[String]) -> Vec<String> {
    let served: Vec<&str> = served.iter().map(String::as_str).collect();
    problems(&rows(), &[], &[], "", &served)
        .into_iter()
        .filter(|problem| problem.contains(": MCP tool "))
        .collect()
}

#[test]
fn every_window_command_with_an_mcp_tool_is_served() {
    assert_eq!(mcp_problems(&served()), Vec::<String>::new());
}

#[test]
fn a_tool_tools_list_stops_serving_fails_by_name() {
    let rows = rows();
    let row = rows
        .iter()
        .find(|r| r.tauri.is_some() && r.mcp.is_some())
        .expect("a window command with an MCP tool");
    let (tauri, mcp) = (row.tauri.unwrap(), row.mcp.unwrap());
    let served: Vec<String> = served().into_iter().filter(|t| t != mcp).collect();
    assert_eq!(
        mcp_problems(&served),
        vec![format!("{tauri}: MCP tool {mcp} is missing")]
    );
}

#[test]
fn a_role_only_tool_is_served_to_that_role_alone() {
    // pool_edit is the budget manager's: the role-free list leaves it out, and the union
    // finds it only because the budget manager's list is read too.
    assert!(!listed(None).contains(&"pool_edit".to_string()));
    assert!(listed(Some(Role::BudgetManager)).contains(&"pool_edit".to_string()));
    assert!(served().contains(&"pool_edit".to_string()));
}
