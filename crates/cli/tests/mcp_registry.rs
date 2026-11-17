#[path = "../src/mcp.rs"]
#[allow(dead_code)]
mod mcp;

use std::io::Cursor;

use mcp::{serve_with, ReadCore, WriteCore};
use serde_json::{json, Value};
use switchboard_core::budget::PoolEdit;
use switchboard_core::rules::Role;

struct Read;

impl ReadCore for Read {
    fn call(&self, _: &str, _: &Value) -> Result<Value, String> {
        Ok(Value::Null)
    }
}

struct Write;

impl WriteCore for Write {
    fn merge(&self, _: &str, _: u64) -> Result<(), String> {
        Ok(())
    }
    fn pool_edit(&self, _: &str, _: &PoolEdit) -> Result<String, String> {
        Ok(String::new())
    }
    fn budget_refresh(&self) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn waiting(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}

fn listed(role: Option<Role>, in_job_worktree: bool) -> Vec<String> {
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
    let mut output = Vec::new();
    serve_with(
        Cursor::new(format!("{request}\n")),
        &mut output,
        &Read,
        role,
        &Write,
        in_job_worktree,
    )
    .unwrap();
    let reply: Value = serde_json::from_slice(&output).unwrap();
    reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect()
}

const READ_TOOLS: [&str; 6] = [
    "sky",
    "constellation",
    "star",
    "waiting",
    "reports",
    "budget",
];

#[test]
fn the_default_list_and_every_job_worktree_list_are_read_only() {
    assert_eq!(listed(None, false), READ_TOOLS);
    for role in [Role::ProjectManager, Role::BudgetManager, Role::Satellite] {
        assert_eq!(listed(Some(role), true), READ_TOOLS);
    }
}
