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

fn listed_tools(role: Option<Role>) -> Vec<Value> {
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
    let mut output = Vec::new();
    serve_with(
        Cursor::new(format!("{request}\n")),
        &mut output,
        &Read,
        role,
        &Write,
        false,
    )
    .unwrap();
    let reply: Value = serde_json::from_slice(&output).unwrap();
    reply["result"]["tools"].as_array().unwrap().clone()
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

fn expected_schema(name: &str) -> Value {
    match name {
        "sky" | "budget" | "errors" | "autopilot_status" | "budget_refresh" => {
            json!({"type":"object","properties":{}})
        }
        "constellation" => {
            json!({"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"}},"required":["repo"]})
        }
        "star" => {
            json!({"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"},"issue":{"type":"integer","minimum":1}},"required":["repo","issue"]})
        }
        "waiting" => json!({"type":"object","properties":{"limit":{"type":"integer","minimum":1}}}),
        "reports" => {
            json!({"type":"object","properties":{"limit":{"type":"integer","minimum":1},"project":{"type":"string"},"outcome":{"type":"string"},"pool":{"type":"string"}}})
        }
        "merge" => {
            json!({"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"},"pr":{"type":"integer","minimum":1}},"required":["repo","pr"]})
        }
        "request_archive" | "request_restore" => {
            json!({"type":"object","properties":{"repo":{"type":"string","description":"The repo's name or owner/name"}},"required":["repo"]})
        }
        "drop_request" => {
            json!({"type":"object","properties":{"kind":{"type":"string","description":"archive or restore"},"repo":{"type":"string","description":"The repo's name or owner/name"}},"required":["kind","repo"]})
        }
        "set_pool_cap" => {
            json!({"type":"object","properties":{"pool":{"type":"string","description":"The pool's name"},"cap":{"type":"number","minimum":0}},"required":["pool","cap"]})
        }
        "pool_edit" => {
            json!({"type":"object","properties":{"pool":{"type":"string","description":"The pool's name"},"plan":{"type":"string"},"cap":{"type":"number","minimum":0},"reset":{"type":"string","description":"e.g. Sun 6:00am"},"cost_usd":{"type":"number","minimum":0}},"required":["pool"]})
        }
        "autopilot_pass" => {
            json!({"type":"object","properties":{"pass":{"type":"string","enum":["roadmap","review","merge","weekly"]},"dry_run":{"type":"boolean"}},"required":["pass"]})
        }
        "satellite_tick" => json!({"type":"object","properties":{"dry_run":{"type":"boolean"}}}),
        "ask" => {
            json!({"type":"object","properties":{"message":{"type":"string"},"thread":{"type":"string","description":"An open thread's id; none starts a thread"},"dry_run":{"type":"boolean"}},"required":["message"]})
        }
        "ask_as_job" => {
            json!({"type":"object","properties":{"kind":{"type":"string","description":"The request kind"},"request":{"type":"string"},"unmet":{"type":"array","items":{"type":"string"},"minItems":1,"description":"The needs this machine does not meet; left out, they are read from this machine's loadout"},"thread":{"type":"string","description":"The chat's thread id, to send the chat with its packet"}},"required":["kind","request"]})
        }
        "rate" => {
            json!({"type":"object","properties":{"thread":{"type":"string"},"turn":{"type":"integer","minimum":0,"description":"The answer's index among the thread's turns"},"rating":{"type":"string","enum":["up","down"]}},"required":["thread","turn","rating"]})
        }
        other => panic!("unexpected MCP tool: {other}"),
    }
}

#[test]
fn every_role_lists_its_existing_tools_and_exact_input_schemas() {
    let cases: [(Option<Role>, &[&str]); 4] = [
        (
            None,
            &[
                "sky",
                "constellation",
                "star",
                "waiting",
                "reports",
                "budget",
            ],
        ),
        (
            Some(Role::ProjectManager),
            &[
                "sky",
                "constellation",
                "star",
                "waiting",
                "reports",
                "budget",
                "autopilot_status",
                "merge",
                "ask",
                "ask_as_job",
                "rate",
                "request_archive",
                "request_restore",
                "drop_request",
                "autopilot_pass",
            ],
        ),
        (
            Some(Role::BudgetManager),
            &[
                "sky",
                "constellation",
                "star",
                "waiting",
                "reports",
                "budget",
                "set_pool_cap",
                "pool_edit",
                "budget_refresh",
            ],
        ),
        (
            Some(Role::Satellite),
            &[
                "sky",
                "constellation",
                "star",
                "waiting",
                "reports",
                "budget",
                "errors",
                "autopilot_status",
                "satellite_tick",
            ],
        ),
    ];

    for (role, expected_names) in cases {
        let tools = listed_tools(role.clone());
        let names: Vec<&str> = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, expected_names, "role {role:?}");
        for tool in tools {
            let name = tool["name"].as_str().unwrap();
            assert_eq!(
                tool["inputSchema"],
                expected_schema(name),
                "{name}, role {role:?}"
            );
        }
    }
}
