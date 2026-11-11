//! #384: the front-door MCP tools `ask`, `ask_as_job` and `rate`. Only the project-manager role
//! holds them; bad arguments never reach the core. The protocol runs over an in-memory pipe
//! against a fake core, so nothing is routed, sent or stored.

// The binary crate has no library target, so the module is compiled in directly.
#[path = "../src/mcp.rs"]
#[allow(dead_code)]
mod mcp;

use std::cell::RefCell;

use mcp::frontdoor::Call;
use mcp::{serve, ReadCore, WriteCore};
use serde_json::{json, Value};
use switchboard_core::budget::PoolEdit;
use switchboard_core::rules::Role;

struct FakeRead;

impl ReadCore for FakeRead {
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String> {
        Ok(json!({ "tool": tool, "args": args }))
    }
}

/// Records each front-door call as text. `fail` makes the core refuse.
#[derive(Default)]
struct FakeWrite {
    fail: bool,
    calls: RefCell<Vec<String>>,
}

impl WriteCore for FakeWrite {
    fn merge(&self, _repo: &str, _pr: u64) -> Result<(), String> {
        Ok(())
    }

    fn pool_edit(&self, tool: &str, edit: &PoolEdit) -> Result<String, String> {
        Ok(format!("{tool} {}: saved", edit.pool))
    }

    fn budget_refresh(&self) -> Result<Value, String> {
        Ok(json!([]))
    }

    fn waiting(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }

    fn front_door(&self, call: &Call) -> Result<Value, String> {
        if self.fail {
            return Err(format!("{}: refused: no thread chat-nope", call.tool()));
        }
        let line = match call {
            Call::Ask {
                message,
                thread,
                dry_run,
            } => format!("ask {message:?} thread={thread:?} dry_run={dry_run}"),
            Call::AskAsJob {
                kind,
                request,
                unmet,
                ..
            } => format!("ask_as_job {kind} {request:?} unmet={unmet:?}"),
            Call::Rate {
                thread,
                turn,
                rating,
            } => format!("rate {thread} {turn} {rating}"),
        };
        self.calls.borrow_mut().push(line);
        Ok(json!({ "tool": call.tool() }))
    }
}

fn run(role: Option<Role>, write: &FakeWrite, lines: &[Value]) -> Vec<Value> {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut out: Vec<u8> = Vec::new();
    serve(input.as_bytes(), &mut out, &FakeRead, role, write).expect("serve");
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("each reply is one JSON line"))
        .collect()
}

fn call(id: u64, name: &str, arguments: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } })
}

fn tools(role: Option<Role>) -> Vec<Value> {
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    run(role, &FakeWrite::default(), &[list])[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .clone()
}

fn names(role: Option<Role>) -> Vec<String> {
    tools(role)
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

const DOOR: [&str; 3] = ["ask", "ask_as_job", "rate"];

#[test]
fn only_the_project_manager_lists_the_front_door() {
    assert_eq!(
        names(Some(Role::ProjectManager))[6..],
        ["merge", "ask", "ask_as_job", "rate"]
    );
    for role in [None, Some(Role::Satellite), Some(Role::BudgetManager)] {
        let listed = names(role);
        for tool in DOOR {
            assert!(!listed.iter().any(|n| n == tool), "{listed:?}");
        }
    }
}

#[test]
fn each_tool_says_what_it_needs() {
    let listed = tools(Some(Role::ProjectManager));
    let schema = |name: &str| {
        listed
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("no {name}"))["inputSchema"]
            .clone()
    };
    assert_eq!(schema("ask")["required"], json!(["message"]));
    assert_eq!(schema("ask")["properties"]["dry_run"]["type"], "boolean");
    assert_eq!(
        schema("ask_as_job")["required"],
        json!(["kind", "request", "unmet"])
    );
    assert_eq!(
        schema("rate")["required"],
        json!(["thread", "turn", "rating"])
    );
    for tool in DOOR {
        let described = listed.iter().find(|t| t["name"] == tool).unwrap();
        assert!(!described["description"].as_str().unwrap().is_empty());
    }
}

#[test]
fn the_arguments_reach_the_core_as_given() {
    let write = FakeWrite::default();
    let replies = run(
        Some(Role::ProjectManager),
        &write,
        &[
            call(1, "ask", json!({ "message": "what is a worktree" })),
            call(
                2,
                "ask",
                json!({ "message": "and then", "thread": "chat-1", "dry_run": true }),
            ),
            call(
                3,
                "ask_as_job",
                json!({ "kind": "code-change", "request": "fix it", "unmet": ["tools", "web"] }),
            ),
            call(
                4,
                "rate",
                json!({ "thread": "chat-1", "turn": 3, "rating": "down" }),
            ),
        ],
    );
    for (reply, tool) in replies.iter().zip(["ask", "ask", "ask_as_job", "rate"]) {
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(text).unwrap(),
            json!({ "tool": tool })
        );
    }
    assert_eq!(
        *write.calls.borrow(),
        [
            "ask \"what is a worktree\" thread=None dry_run=false",
            "ask \"and then\" thread=Some(\"chat-1\") dry_run=true",
            "ask_as_job code-change \"fix it\" unmet=[\"tools\", \"web\"]",
            "rate chat-1 3 down",
        ]
    );
}

#[test]
fn any_other_role_calling_the_front_door_is_an_unknown_tool() {
    let args = [
        json!({ "message": "what is a worktree" }),
        json!({ "kind": "code-change", "request": "fix it", "unmet": ["web"] }),
        json!({ "thread": "chat-1", "turn": 1, "rating": "up" }),
    ];
    for role in [None, Some(Role::Satellite), Some(Role::BudgetManager)] {
        let write = FakeWrite::default();
        let lines: Vec<Value> = DOOR
            .iter()
            .zip(&args)
            .map(|(tool, args)| call(1, tool, args.clone()))
            .collect();
        for (reply, tool) in run(role, &write, &lines).iter().zip(DOOR) {
            assert_eq!(reply["error"]["code"], -32602, "{reply}");
            assert_eq!(reply["error"]["message"], format!("unknown tool: {tool}"));
        }
        assert!(write.calls.borrow().is_empty());
    }
}

#[test]
fn bad_arguments_are_invalid_params_and_never_reach_the_core() {
    let write = FakeWrite::default();
    let cases = [
        ("ask", json!({})),
        ("ask", json!({ "message": 7 })),
        ("ask", json!({ "message": "hi", "dry_run": "yes" })),
        ("ask", json!({ "message": "hi", "thread": 4 })),
        (
            "ask_as_job",
            json!({ "kind": "code-change", "request": "fix it" }),
        ),
        (
            "ask_as_job",
            json!({ "kind": "code-change", "request": "fix it", "unmet": "web" }),
        ),
        (
            "ask_as_job",
            json!({ "kind": "code-change", "request": "fix it", "unmet": [] }),
        ),
        (
            "ask_as_job",
            json!({ "kind": "code-change", "request": "fix it", "unmet": ["web", 3] }),
        ),
        ("rate", json!({ "thread": "chat-1", "turn": 1 })),
        (
            "rate",
            json!({ "thread": "chat-1", "turn": -1, "rating": "up" }),
        ),
        (
            "rate",
            json!({ "thread": "chat-1", "turn": 1.5, "rating": "up" }),
        ),
        (
            "rate",
            json!({ "thread": "chat-1", "turn": "1", "rating": "up" }),
        ),
        (
            "rate",
            json!({ "thread": "chat-1", "turn": 1, "rating": "sideways" }),
        ),
    ];
    let lines: Vec<Value> = cases
        .iter()
        .map(|(tool, args)| call(1, tool, args.clone()))
        .collect();
    let replies = run(Some(Role::ProjectManager), &write, &lines);
    assert_eq!(replies.len(), cases.len());
    for (reply, (tool, args)) in replies.iter().zip(&cases) {
        assert_eq!(reply["error"]["code"], -32602, "{tool} {args}: {reply}");
        let message = reply["error"]["message"].as_str().unwrap();
        assert!(message.starts_with(tool), "{tool} {args}: {message}");
    }
    assert!(write.calls.borrow().is_empty());
}

#[test]
fn a_core_refusal_is_a_tool_error_with_its_text() {
    let write = FakeWrite {
        fail: true,
        ..Default::default()
    };
    let replies = run(
        Some(Role::ProjectManager),
        &write,
        &[call(
            1,
            "rate",
            json!({ "thread": "chat-nope", "turn": 1, "rating": "up" }),
        )],
    );
    assert_eq!(replies[0]["result"]["isError"], true);
    assert_eq!(
        replies[0]["result"]["content"][0]["text"],
        "rate: refused: no thread chat-nope"
    );
}

#[test]
fn a_core_with_no_front_door_says_so() {
    struct Bare;
    impl WriteCore for Bare {
        fn merge(&self, _repo: &str, _pr: u64) -> Result<(), String> {
            Ok(())
        }
        fn pool_edit(&self, _tool: &str, _edit: &PoolEdit) -> Result<String, String> {
            Ok(String::new())
        }
        fn budget_refresh(&self) -> Result<Value, String> {
            Ok(json!([]))
        }
        fn waiting(&self, _text: &str) -> Result<(), String> {
            Ok(())
        }
    }
    let line = call(1, "ask", json!({ "message": "hi" }));
    let mut out: Vec<u8> = Vec::new();
    serve(
        format!("{line}\n").as_bytes(),
        &mut out,
        &FakeRead,
        Some(Role::ProjectManager),
        &Bare,
    )
    .unwrap();
    let reply: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(reply["result"]["isError"], true);
    assert_eq!(
        reply["result"]["content"][0]["text"],
        "ask: this core has no front door"
    );
}
