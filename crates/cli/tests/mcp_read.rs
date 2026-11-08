//! MCP-1: `switchboard mcp` without a role flag: JSON-RPC over stdio, six read tools.
//! The protocol runs over an in-memory pipe (a byte slice in, a Vec out) against a fake core.

// The binary crate has no library target, so the module is compiled in directly.
#[path = "../src/mcp.rs"]
#[allow(dead_code)]
mod mcp;

use std::cell::RefCell;

use mcp::{serve, ReadCore};
use serde_json::{json, Value};

/// A core that records each call and answers with the tool name and arguments, or `fail`.
#[derive(Default)]
struct FakeCore {
    calls: RefCell<Vec<(String, Value)>>,
    fail: Option<String>,
}

impl ReadCore for FakeCore {
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String> {
        self.calls
            .borrow_mut()
            .push((tool.to_string(), args.clone()));
        match &self.fail {
            Some(message) => Err(message.clone()),
            None => Ok(json!({ "tool": tool, "args": args })),
        }
    }
}

/// Send each line to the server and parse every reply line.
fn run(core: &FakeCore, lines: &[String]) -> Vec<Value> {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut out: Vec<u8> = Vec::new();
    serve(input.as_bytes(), &mut out, core).expect("serve");
    String::from_utf8(out)
        .expect("utf-8 output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("each reply is one JSON line"))
        .collect()
}

fn request(id: u64, method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
}

fn call(id: u64, name: &str, arguments: Value) -> String {
    request(
        id,
        "tools/call",
        json!({ "name": name, "arguments": arguments }),
    )
}

#[test]
fn initialize_answers_with_server_name_and_tools_capability() {
    let core = FakeCore::default();
    let replies = run(&core, &[request(1, "initialize", json!({}))]);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], 1);
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "switchboard");
    assert!(replies[0]["result"]["capabilities"]["tools"].is_object());
}

#[test]
fn initialized_notification_gets_no_reply() {
    let core = FakeCore::default();
    let line = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string();
    assert!(run(&core, &[line]).is_empty());
}

#[test]
fn ping_answers_with_an_empty_result() {
    let core = FakeCore::default();
    let replies = run(&core, &[request(7, "ping", json!({}))]);
    assert_eq!(replies[0]["id"], 7);
    assert_eq!(replies[0]["result"], json!({}));
}

#[test]
fn tools_list_returns_exactly_the_six_read_tools() {
    let core = FakeCore::default();
    let replies = run(&core, &[request(2, "tools/list", json!({}))]);
    let tools = replies[0]["result"]["tools"]
        .as_array()
        .expect("tools array");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "sky",
            "constellation",
            "star",
            "waiting",
            "reports",
            "budget"
        ]
    );
    for tool in tools {
        assert!(tool["description"].as_str().unwrap().len() > 10);
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
}

#[test]
fn tool_schemas_fit_the_byte_limit() {
    let core = FakeCore::default();
    let replies = run(&core, &[request(2, "tools/list", json!({}))]);
    let bytes = replies[0]["result"]["tools"].to_string().len();
    assert!(bytes < 8_000, "tool schemas are {bytes} bytes, limit 8000");
    assert!(bytes / 4 < 2_000, "about {} tokens, limit 2000", bytes / 4);
}

#[test]
fn tool_inputs_match_the_issue() {
    let core = FakeCore::default();
    let replies = run(&core, &[request(2, "tools/list", json!({}))]);
    let tools = replies[0]["result"]["tools"].as_array().unwrap();
    let schema = |name: &str| -> Value {
        tools.iter().find(|t| t["name"] == name).unwrap()["inputSchema"].clone()
    };
    assert_eq!(schema("constellation")["required"], json!(["repo"]));
    assert_eq!(schema("star")["required"], json!(["repo", "issue"]));
    for name in ["sky", "budget"] {
        assert!(schema(name)["properties"].as_object().unwrap().is_empty());
    }
    for name in ["waiting", "reports"] {
        assert!(schema(name)["properties"]["limit"].is_object());
        assert!(schema(name).get("required").is_none());
    }
}

#[test]
fn each_tool_call_reaches_the_core_and_returns_its_answer_as_text() {
    let cases = [
        ("sky", json!({})),
        ("constellation", json!({ "repo": "switchboard" })),
        ("star", json!({ "repo": "switchboard", "issue": 12 })),
        ("waiting", json!({ "limit": 5 })),
        ("reports", json!({})),
        ("budget", json!({})),
    ];
    for (i, (name, args)) in cases.iter().enumerate() {
        let core = FakeCore::default();
        let replies = run(&core, &[call(i as u64, name, args.clone())]);

        assert_eq!(*core.calls.borrow(), vec![(name.to_string(), args.clone())]);
        let result = &replies[0]["result"];
        assert_eq!(result["isError"], false);
        let block = &result["content"].as_array().unwrap()[0];
        assert_eq!(block["type"], "text");
        let answer: Value = serde_json::from_str(block["text"].as_str().unwrap()).unwrap();
        assert_eq!(answer, json!({ "tool": name, "args": args }));
    }
}

#[test]
fn core_error_returns_is_error_with_its_text() {
    let core = FakeCore {
        fail: Some("waiting: repo is live".to_string()),
        ..Default::default()
    };
    let replies = run(&core, &[call(3, "waiting", json!({}))]);
    let result = &replies[0]["result"];
    assert_eq!(result["isError"], true);
    assert_eq!(result["content"][0]["text"], "waiting: repo is live");
}

#[test]
fn unknown_tool_answers_invalid_params() {
    let core = FakeCore::default();
    let replies = run(&core, &[call(4, "merge", json!({}))]);
    assert_eq!(replies[0]["error"]["code"], -32602);
    assert_eq!(replies[0]["error"]["message"], "unknown tool: merge");
    assert!(core.calls.borrow().is_empty());
}

#[test]
fn unknown_method_answers_method_not_found() {
    let core = FakeCore::default();
    let replies = run(&core, &[request(5, "resources/list", json!({}))]);
    assert_eq!(replies[0]["id"], 5);
    assert_eq!(replies[0]["error"]["code"], -32601);
}

#[test]
fn malformed_line_answers_parse_error_and_the_loop_carries_on() {
    let core = FakeCore::default();
    let replies = run(
        &core,
        &["{not json".to_string(), request(6, "ping", json!({}))],
    );
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert!(replies[0]["id"].is_null());
    assert_eq!(replies[1]["id"], 6);
    assert_eq!(replies[1]["result"], json!({}));
}
