//! MCP-2: `switchboard mcp --role <role>` adds that role's write tools, each through CORE-3.
//! The protocol runs over an in-memory pipe against fake cores; the bad-flag case runs the
//! binary.

// The binary crate has no library target, so the module is compiled in directly.
#[path = "../src/mcp.rs"]
#[allow(dead_code)]
mod mcp;

use std::cell::RefCell;
use std::io::Write;
use std::process::{Command, Stdio};

use mcp::{serve, ReadCore, WriteCore};
use serde_json::{json, Value};
use switchboard_core::budget::PoolEdit;
use switchboard_core::rules::Role;

const READ: [&str; 6] = [
    "sky",
    "constellation",
    "star",
    "waiting",
    "reports",
    "budget",
];

/// Answers every read with its name and arguments.
struct FakeRead;

impl ReadCore for FakeRead {
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String> {
        Ok(json!({ "tool": tool, "args": args }))
    }
}

/// Records what reached it. `no_pass` makes `merge` refuse as GH-5 would: no PASS on head.
#[derive(Default)]
struct FakeWrite {
    no_pass: bool,
    merged: RefCell<Vec<(String, u64)>>,
    caps: RefCell<Vec<(String, f64)>>,
    edits: RefCell<Vec<(String, PoolEdit)>>,
    refreshes: RefCell<u32>,
    waiting: RefCell<Vec<String>>,
}

impl WriteCore for FakeWrite {
    fn merge(&self, repo: &str, pr: u64) -> Result<(), String> {
        if self.no_pass {
            return Err(format!("{repo}#{pr}: no PASS on head"));
        }
        self.merged.borrow_mut().push((repo.to_string(), pr));
        Ok(())
    }

    fn pool_edit(&self, tool: &str, edit: &PoolEdit) -> Result<String, String> {
        if let Some(cap) = edit.cap {
            self.caps.borrow_mut().push((edit.pool.clone(), cap));
        }
        self.edits
            .borrow_mut()
            .push((tool.to_string(), edit.clone()));
        Ok(format!("{tool} {}: saved", edit.pool))
    }

    fn budget_refresh(&self) -> Result<Value, String> {
        *self.refreshes.borrow_mut() += 1;
        Ok(json!([{ "pool": "claude" }]))
    }

    fn waiting(&self, text: &str) -> Result<(), String> {
        self.waiting.borrow_mut().push(text.to_string());
        Ok(())
    }
}

/// Send each line to the server and parse every reply line.
fn run(role: Option<Role>, write: &FakeWrite, lines: &[String]) -> Vec<Value> {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut out: Vec<u8> = Vec::new();
    serve(input.as_bytes(), &mut out, &FakeRead, role, write).expect("serve");
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

fn tool_names(role: Option<Role>) -> Vec<String> {
    let replies = run(
        role,
        &FakeWrite::default(),
        &[request(1, "tools/list", json!({}))],
    );
    replies[0]["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

fn with_extra(extra: &[&str]) -> Vec<String> {
    READ.iter()
        .chain(extra.iter())
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn no_role_lists_only_the_six_read_tools() {
    assert_eq!(tool_names(None), with_extra(&[]));
}

#[test]
fn no_role_merge_is_an_unknown_tool() {
    let write = FakeWrite::default();
    let replies = run(
        None,
        &write,
        &[call(4, "merge", json!({ "repo": "switchboard", "pr": 12 }))],
    );
    assert_eq!(replies[0]["error"]["code"], -32602);
    assert_eq!(replies[0]["error"]["message"], "unknown tool: merge");
    assert!(write.merged.borrow().is_empty());
}

#[test]
fn project_manager_lists_merge_after_the_read_tools() {
    assert_eq!(
        tool_names(Some(Role::ProjectManager)),
        // #385: the autopilot's status read and pass tool.
        with_extra(&["autopilot_status", "merge", "autopilot_pass"])
    );
}

#[test]
fn project_manager_merge_without_a_pass_on_head_is_refused_and_merges_nothing() {
    let write = FakeWrite {
        no_pass: true,
        ..Default::default()
    };
    let replies = run(
        Some(Role::ProjectManager),
        &write,
        &[call(5, "merge", json!({ "repo": "switchboard", "pr": 12 }))],
    );
    let result = &replies[0]["result"];
    assert_eq!(result["isError"], true);
    assert_eq!(
        result["content"][0]["text"],
        "switchboard#12: no PASS on head"
    );
    assert!(write.merged.borrow().is_empty());
}

#[test]
fn project_manager_merge_with_a_pass_merges_once() {
    let write = FakeWrite::default();
    let replies = run(
        Some(Role::ProjectManager),
        &write,
        &[call(6, "merge", json!({ "repo": "switchboard", "pr": 12 }))],
    );
    assert_eq!(replies[0]["result"]["isError"], false);
    assert_eq!(
        *write.merged.borrow(),
        vec![("switchboard".to_string(), 12)]
    );
}

#[test]
fn budget_manager_lists_set_pool_cap_and_no_merge() {
    assert_eq!(
        tool_names(Some(Role::BudgetManager)),
        with_extra(&["set_pool_cap", "pool_edit", "budget_refresh"])
    );
    let write = FakeWrite::default();
    let replies = run(
        Some(Role::BudgetManager),
        &write,
        &[call(7, "merge", json!({ "repo": "switchboard", "pr": 12 }))],
    );
    assert_eq!(replies[0]["error"]["code"], -32602);
    assert_eq!(replies[0]["error"]["message"], "unknown tool: merge");
    assert!(write.merged.borrow().is_empty());
}

#[test]
fn budget_manager_set_pool_cap_reaches_the_core_once() {
    let write = FakeWrite::default();
    let replies = run(
        Some(Role::BudgetManager),
        &write,
        &[call(
            8,
            "set_pool_cap",
            json!({ "pool": "claude", "cap": 40.0 }),
        )],
    );
    assert_eq!(replies[0]["result"]["isError"], false);
    assert_eq!(*write.caps.borrow(), vec![("claude".to_string(), 40.0)]);
}

#[test]
fn satellite_lists_no_write_tool() {
    // #375: the satellite's one extra tool is a read (`errors`), never a write.
    // #385: and the autopilot's status read and the satellite's own tick.
    assert_eq!(
        tool_names(Some(Role::Satellite)),
        with_extra(&["errors", "autopilot_status", "satellite_tick"])
    );
    let write = FakeWrite::default();
    let replies = run(
        Some(Role::Satellite),
        &write,
        &[
            call(9, "merge", json!({ "repo": "switchboard", "pr": 12 })),
            call(10, "set_pool_cap", json!({ "pool": "claude", "cap": 40.0 })),
        ],
    );
    for reply in &replies {
        assert_eq!(reply["error"]["code"], -32602);
    }
    assert_eq!(replies[0]["error"]["message"], "unknown tool: merge");
    assert_eq!(replies[1]["error"]["message"], "unknown tool: set_pool_cap");
    assert!(write.merged.borrow().is_empty());
    assert!(write.caps.borrow().is_empty());
}

#[test]
fn a_bad_role_exits_before_any_reply() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(["mcp", "--role", "boss"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn switchboard");
    let mut stdin = child.stdin.take().expect("child stdin");
    // The server may already have exited; a broken pipe here is the point of the test.
    let _ = stdin.write_all(format!("{}\n", request(1, "ping", json!({}))).as_bytes());
    drop(stdin);
    let output = child.wait_with_output().expect("wait for switchboard");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no reply before the error");
}

#[test]
fn budget_manager_pool_edit_reaches_the_core_with_each_field() {
    let write = FakeWrite::default();
    let replies = run(
        Some(Role::BudgetManager),
        &write,
        &[call(
            11,
            "pool_edit",
            json!({ "pool": "claude", "plan": "max-5x", "reset": "Sun 6:00am", "cost_usd": 100 }),
        )],
    );
    assert_eq!(replies[0]["result"]["isError"], false, "{replies:?}");
    assert_eq!(
        replies[0]["result"]["content"][0]["text"],
        "pool_edit claude: saved"
    );
    let edits = write.edits.borrow();
    assert_eq!(edits.len(), 1);
    assert_eq!(
        edits[0],
        (
            "pool_edit".to_string(),
            PoolEdit {
                pool: "claude".into(),
                plan: Some("max-5x".into()),
                cap: None,
                reset: Some("Sun 6:00am".into()),
                cost_usd: Some(100.0),
            }
        )
    );
}

#[test]
fn pool_edit_without_a_pool_or_with_a_bad_field_never_reaches_the_core() {
    let write = FakeWrite::default();
    let replies = run(
        Some(Role::BudgetManager),
        &write,
        &[
            call(12, "pool_edit", json!({ "plan": "max-5x" })),
            call(13, "pool_edit", json!({ "pool": "claude", "cap": "lots" })),
        ],
    );
    for reply in &replies {
        assert_eq!(reply["error"]["code"], -32602, "{reply}");
    }
    assert!(write.edits.borrow().is_empty());
}

#[test]
fn budget_manager_budget_refresh_reaches_the_core_once() {
    let write = FakeWrite::default();
    let replies = run(
        Some(Role::BudgetManager),
        &write,
        &[call(14, "budget_refresh", json!({}))],
    );
    assert_eq!(replies[0]["result"]["isError"], false, "{replies:?}");
    let text = replies[0]["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap(),
        json!([{ "pool": "claude" }])
    );
    assert_eq!(*write.refreshes.borrow(), 1);
}

#[test]
fn other_roles_get_no_pool_edit_and_no_budget_refresh() {
    for role in [Role::ProjectManager, Role::Satellite] {
        let write = FakeWrite::default();
        let replies = run(
            Some(role.clone()),
            &write,
            &[
                call(
                    15,
                    "pool_edit",
                    json!({ "pool": "claude", "plan": "max-5x" }),
                ),
                call(16, "budget_refresh", json!({})),
            ],
        );
        assert_eq!(replies[0]["error"]["message"], "unknown tool: pool_edit");
        assert_eq!(
            replies[1]["error"]["message"],
            "unknown tool: budget_refresh"
        );
        assert!(write.edits.borrow().is_empty());
        assert_eq!(*write.refreshes.borrow(), 0);
    }
}

/// #375: the satellite reads the recorded fleet errors and the IT tickets; the call reaches the
/// read core with no write made.
#[test]
fn satellite_errors_reaches_the_read_core() {
    let write = FakeWrite::default();
    let replies = run(
        Some(Role::Satellite),
        &write,
        &[call(30, "errors", json!({}))],
    );
    assert_eq!(replies[0]["result"]["isError"], false, "{}", replies[0]);
    let text = replies[0]["result"]["content"][0]["text"].as_str().unwrap();
    let answer: Value = serde_json::from_str(text).unwrap();
    assert_eq!(answer["tool"], "errors");
    assert!(write.merged.borrow().is_empty());
    assert!(write.caps.borrow().is_empty());
    assert!(write.edits.borrow().is_empty());
}

/// #375: MCP-1's six stay the list with no role, and the other roles do not read `errors`.
#[test]
fn errors_is_unknown_without_the_satellite_role() {
    for role in [None, Some(Role::ProjectManager), Some(Role::BudgetManager)] {
        assert!(
            !tool_names(role.clone()).iter().any(|t| t == "errors"),
            "{role:?} lists errors"
        );
        let replies = run(
            role,
            &FakeWrite::default(),
            &[call(31, "errors", json!({}))],
        );
        assert_eq!(replies[0]["error"]["code"], -32602);
        assert_eq!(replies[0]["error"]["message"], "unknown tool: errors");
    }
}
