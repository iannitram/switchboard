//! MCP-1/MCP-2 robustness: a bad line never stops the loop (#336), every non-request line gets a
//! reply (#353), and tool arguments are checked against the tool's input schema (#360).

#[path = "../src/mcp.rs"]
#[allow(dead_code)]
mod mcp;

use mcp::{serve, ReadCore, WriteCore};
use serde_json::{json, Value};
use switchboard_core::rules::Role;

struct Echo;

impl ReadCore for Echo {
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String> {
        Ok(json!({ "tool": tool, "args": args }))
    }
}

struct NoWrites;

impl WriteCore for NoWrites {
    fn merge(&self, _: &str, _: u64) -> Result<(), String> {
        panic!("a bad call must not reach the write core")
    }
    fn set_pool_cap(&self, _: &str, _: f64) -> Result<(), String> {
        panic!("a bad call must not reach the write core")
    }
    fn waiting(&self, _: &str) -> Result<(), String> {
        panic!("a bad call must not reach the write core")
    }
}

fn run_bytes(input: &[u8], role: Option<Role>) -> Vec<Value> {
    let mut out = Vec::new();
    serve(input, &mut out, &Echo, role, &NoWrites).expect("serve");
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn call(tool: &str, args: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}})
}

fn one(msg: Value, role: Option<Role>) -> Value {
    let line = format!("{msg}\n");
    run_bytes(line.as_bytes(), role).remove(0)
}

#[test]
fn non_utf8_line_is_a_parse_error_and_the_loop_goes_on() {
    let mut input = br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#.to_vec();
    input.extend_from_slice(b"\n\xff\xfe\n");
    input.extend_from_slice(br#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#);
    input.push(b'\n');
    let replies = run_bytes(&input, None);
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[0]["id"], 1);
    assert_eq!(replies[1]["error"]["code"], -32700);
    assert_eq!(replies[1]["id"], Value::Null);
    assert_eq!(replies[2]["id"], 3);
}

#[test]
fn non_request_lines_get_invalid_request() {
    for line in [
        "[]",
        r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#,
        "123",
        "{}",
        "null",
        "true",
        r#""x""#,
    ] {
        let reply = run_bytes(format!("{line}\n").as_bytes(), None).remove(0);
        assert_eq!(reply["error"]["code"], -32600, "{line}");
        assert_eq!(reply["id"], Value::Null, "{line}");
    }
    let batch = run_bytes(b"[{\"id\":1,\"method\":\"ping\"}]\n", None).remove(0);
    assert!(batch["error"]["message"]
        .as_str()
        .unwrap()
        .contains("batch"));
}

#[test]
fn bad_method_and_bad_id_get_invalid_request() {
    let r = one(json!({"jsonrpc":"2.0","id":4,"method":7}), None);
    assert_eq!(
        (r["error"]["code"].clone(), r["id"].clone()),
        (json!(-32600), json!(4))
    );
    let r = one(json!({"jsonrpc":"2.0","id":[1],"method":"ping"}), None);
    assert_eq!(
        (r["error"]["code"].clone(), r["id"].clone()),
        (json!(-32600), Value::Null)
    );
    // A notification and a client response still get no reply.
    assert!(run_bytes(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":5,\"result\":{}}\n", None).is_empty());
}

#[test]
fn wrong_typed_arguments_are_named_errors() {
    let bad = [
        ("waiting", json!({"limit": -1})),
        ("waiting", json!({"limit": "2"})),
        ("waiting", json!({"limit": 1.5})),
        ("waiting", json!({"limit": null})),
        ("reports", json!({"limit": 0})),
        ("star", json!({"repo": "a", "issue": "2"})),
        ("star", json!({"repo": 3, "issue": 2})),
        ("star", json!({"repo": "a"})),
        ("sky", json!([1])),
    ];
    for (tool, args) in bad {
        let r = one(call(tool, args.clone()), None);
        assert_eq!(r["error"]["code"], -32602, "{tool} {args}");
        assert!(r["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{tool}: ")));
    }
    let r = one(call("waiting", json!({"limit": "2"})), None);
    assert!(r["error"]["message"].as_str().unwrap().contains("limit"));
}

#[test]
fn integer_written_as_float_is_accepted_as_integer() {
    let r = one(call("star", json!({"repo": "a", "issue": 2.0})), None);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    let echoed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(echoed["args"]["issue"].as_u64(), Some(2));
    // No arguments at all is fine for a tool with none required.
    let r = one(
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"sky"}}),
        None,
    );
    assert_eq!(r["result"]["isError"], false);
}

#[test]
fn merge_pr_below_minimum_and_missing_name_are_errors() {
    let r = one(
        call("merge", json!({"repo": "a", "pr": 0})),
        Some(Role::ProjectManager),
    );
    assert_eq!(r["error"]["code"], -32602);
    assert!(r["error"]["message"].as_str().unwrap().contains("pr"));
    let r = one(
        call("set_pool_cap", json!({"pool": "p", "cap": "5"})),
        Some(Role::BudgetManager),
    );
    assert_eq!(r["error"]["code"], -32602);
    for params in [json!({}), json!({"name": 5})] {
        let r = one(
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":params}),
            None,
        );
        assert_eq!(r["error"]["code"], -32602);
        assert!(r["error"]["message"].as_str().unwrap().contains("name"));
    }
}
