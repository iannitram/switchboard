//! MCP-2 (#243): the write tools reach the real core. `CoreWrites` is the write core that
//! `switchboard mcp --role <role>` runs: `merge` is the CLI's own merge (CLI-3, #242),
//! `set_pool_cap` is BUD-2's `save_edit` with its ledger row and Waiting item, and `waiting`
//! adds one local Waiting on you item. Locked tests (R28): the builder never edits this file.

// The binary crate has no library target, so the module is compiled in directly.
#[path = "../src/mcp.rs"]
#[allow(dead_code)]
mod mcp;

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mcp::{serve, CoreWrites, ReadCore, WriteCore};
use serde_json::{json, Value};
use switchboard_core::clock::FixedClock;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::{PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::rules::Role;
use switchboard_core::waiting::{Item, LocalItems, Source};

const NOW: i64 = 1_794_146_400;
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CONFIG: &str = r#"{
  "version": 1,
  "pools": {
    "claude": { "label": "Claude", "kind": "weekly" },
    "deepseek": { "label": "DeepSeek", "kind": "fixed-period", "cap_usd": 10, "note": "keep me" }
  }
}
"#;
/// merge_pr.py's stand-in: logs its arguments to calls.log beside itself and exits 0.
const MERGE_STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(sys.argv[1:]) + "\n")
"#;

struct FakeRead;

impl ReadCore for FakeRead {
    fn call(&self, _: &str, _: &Value) -> Result<Value, String> {
        Ok(json!({}))
    }
}

/// A fresh folder for one test, under a plain (not job-worktree) temp folder.
fn fresh(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-mcp2-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A pipeline root: the fleet stub, its budget answer, CONFIG and the merge_pr.py stub.
fn pipeline(dir: &Path) -> PathBuf {
    let root = dir.join("pipeline");
    let stub = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/fleet-stub");
    for sub in ["tools/fleet", "tools/merge-pr", "answers"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    std::fs::copy(
        stub.join("tools/fleet/fleet.py"),
        root.join("tools/fleet/fleet.py"),
    )
    .unwrap();
    std::fs::copy(
        stub.join("answers/budget.json"),
        root.join("answers/budget.json"),
    )
    .unwrap();
    std::fs::write(root.join("tools/fleet/config.json"), CONFIG).unwrap();
    std::fs::write(root.join("tools/merge-pr/merge_pr.py"), MERGE_STUB).unwrap();
    root
}

fn config(root: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root.join("tools/fleet/config.json")).unwrap())
        .unwrap()
}

fn lines(path: &Path) -> Vec<Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
        Err(_) => vec![],
    }
}

fn call(id: u64, name: &str, arguments: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } })
    .to_string()
}

/// Serve the lines in-process from a plain folder and parse every reply.
fn run(role: Role, writes: &CoreWrites<'_>, input: &[String]) -> Vec<Value> {
    // serve() checks the current folder for a job worktree; these tests must run from a plain one.
    std::env::set_current_dir(std::env::temp_dir()).unwrap();
    let input: String = input.iter().map(|l| format!("{l}\n")).collect();
    let mut out: Vec<u8> = Vec::new();
    serve(input.as_bytes(), &mut out, &FakeRead, Some(role), writes).expect("serve");
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn text(reply: &Value) -> (bool, String) {
    let result = &reply["result"];
    let is_error = result["isError"].as_bool().expect("a tools/call result");
    (
        is_error,
        result["content"][0]["text"].as_str().unwrap().to_string(),
    )
}

fn no_merge(_: &str, _: u64) -> Result<(), String> {
    panic!("no merge in this test")
}

#[test]
fn merge_hands_repo_and_pr_to_the_cli_merge_once() {
    let clock = FixedClock::at(NOW);
    let seen = RefCell::new(Vec::new());
    let merge = |repo: &str, pr: u64| -> Result<(), String> {
        seen.borrow_mut().push((repo.to_string(), pr));
        Ok(())
    };
    let writes = CoreWrites {
        merge: &merge,
        fleet: None,
        data: None,
        clock: &clock,
    };
    let replies = run(
        Role::ProjectManager,
        &writes,
        &[call(1, "merge", json!({ "repo": "switchboard", "pr": 12 }))],
    );
    assert_eq!(text(&replies[0]), (false, "merge: done".to_string()));
    assert_eq!(*seen.borrow(), vec![("switchboard".to_string(), 12)]);
}

#[test]
fn a_refused_cli_merge_is_the_reply_text_word_for_word() {
    let clock = FixedClock::at(NOW);
    let merge =
        |_: &str, _: u64| -> Result<(), String> { Err("no VERDICT: PASS on this PR".into()) };
    let writes = CoreWrites {
        merge: &merge,
        fleet: None,
        data: None,
        clock: &clock,
    };
    let replies = run(
        Role::ProjectManager,
        &writes,
        &[call(2, "merge", json!({ "repo": "switchboard", "pr": 12 }))],
    );
    assert_eq!(
        text(&replies[0]),
        (true, "no VERDICT: PASS on this PR".to_string())
    );
}

#[test]
fn set_pool_cap_saves_config_a_ledger_row_and_a_waiting_item() {
    let dir = fresh("cap");
    let root = pipeline(&dir);
    let data = dir.join("data");
    let clock = FixedClock::at(NOW);
    let writes = CoreWrites {
        merge: &no_merge,
        fleet: Some(FleetBridge::with_root(&root)),
        data: Some(data.clone()),
        clock: &clock,
    };
    let replies = run(
        Role::BudgetManager,
        &writes,
        &[call(
            3,
            "set_pool_cap",
            json!({ "pool": "deepseek", "cap": 25.0 }),
        )],
    );
    assert_eq!(text(&replies[0]), (false, "set_pool_cap: done".to_string()));
    let pool = &config(&root)["pools"]["deepseek"];
    assert_eq!(pool["cap_usd"], json!(25.0));
    assert_eq!(pool["note"], "keep me");
    assert_eq!(
        lines(&data.join("pool-edits.jsonl")),
        vec![json!({ "at": NOW, "pool": "deepseek",
                     "set": { "cap_usd": 25.0 }, "before": { "cap_usd": 10 } })]
    );
    let items = LocalItems::new(&data).load().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].what, "Land the deepseek pool edit");
    assert_eq!(
        items[0].source,
        Source::PoolEdit {
            pool: "deepseek".into()
        }
    );
    assert_eq!(items[0].since, NOW);
}

#[test]
fn a_refused_pool_cap_writes_nothing_and_says_why() {
    let dir = fresh("cap-refused");
    let root = pipeline(&dir);
    let data = dir.join("data");
    let clock = FixedClock::at(NOW);
    let writes = CoreWrites {
        merge: &no_merge,
        fleet: Some(FleetBridge::with_root(&root)),
        data: Some(data.clone()),
        clock: &clock,
    };
    let replies = run(
        Role::BudgetManager,
        &writes,
        &[
            call(4, "set_pool_cap", json!({ "pool": "nope", "cap": 5 })),
            call(5, "set_pool_cap", json!({ "pool": "deepseek", "cap": -1 })),
        ],
    );
    assert_eq!(
        text(&replies[0]),
        (
            true,
            "set_pool_cap nope: parse: no pool nope in config.json".to_string()
        )
    );
    assert_eq!(
        text(&replies[1]),
        (
            true,
            "set_pool_cap deepseek: parse: cap must be a number 0 or more".to_string()
        )
    );
    assert_eq!(
        config(&root),
        serde_json::from_str::<Value>(CONFIG).unwrap()
    );
    assert!(!data.join("pool-edits.jsonl").exists());
    assert!(!LocalItems::new(&data).path().exists());
}

#[test]
fn set_pool_cap_without_a_pipeline_or_data_folder_names_the_variable() {
    let dir = fresh("cap-unset");
    let root = pipeline(&dir);
    let clock = FixedClock::at(NOW);
    let no_pipeline = CoreWrites {
        merge: &no_merge,
        fleet: None,
        data: Some(dir.join("data")),
        clock: &clock,
    };
    let no_data = CoreWrites {
        merge: &no_merge,
        fleet: Some(FleetBridge::with_root(&root)),
        data: None,
        clock: &clock,
    };
    let cap = call(6, "set_pool_cap", json!({ "pool": "deepseek", "cap": 25 }));
    assert_eq!(
        text(
            &run(
                Role::BudgetManager,
                &no_pipeline,
                std::slice::from_ref(&cap)
            )[0]
        ),
        (
            true,
            "set_pool_cap: set SWITCHBOARD_PIPELINE to your pipeline checkout".to_string()
        )
    );
    assert_eq!(
        text(&run(Role::BudgetManager, &no_data, &[cap])[0]),
        (
            true,
            "set_pool_cap: set SWITCHBOARD_DATA to the app data folder".to_string()
        )
    );
    assert_eq!(
        config(&root),
        serde_json::from_str::<Value>(CONFIG).unwrap()
    );
}

#[test]
fn waiting_adds_one_guarded_action_item_to_the_local_items() {
    let dir = fresh("waiting");
    let data = dir.join("data");
    let clock = FixedClock::at(NOW);
    let writes = CoreWrites {
        merge: &no_merge,
        fleet: None,
        data: Some(data.clone()),
        clock: &clock,
    };
    let asked = "merge switchboard#12: needs a confirm in the app";
    writes.waiting(asked).unwrap();
    writes.waiting(asked).unwrap();
    let want = Item {
        what: asked.to_string(),
        why: "Asked for from the CLI or MCP; only the app can do it, after I confirm (CORE-3)."
            .to_string(),
        source: Source::GuardedAction {
            text: asked.to_string(),
        },
        link: String::new(),
        action: "Confirm in the app".to_string(),
        since: NOW,
    };
    assert_eq!(LocalItems::new(&data).load().unwrap(), vec![want]);
    let none = CoreWrites {
        merge: &no_merge,
        fleet: None,
        data: None,
        clock: &clock,
    };
    assert_eq!(
        none.waiting(asked),
        Err("waiting: set SWITCHBOARD_DATA to the app data folder".to_string())
    );
}

/// The built binary in `cwd`, with the pipeline and data folders set and a clean fleet env.
/// `input` lines go to stdin (for `mcp`).
fn switchboard(argv: &[&str], cwd: &Path, root: &Path, data: &Path, input: &[String]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(argv)
        .current_dir(cwd)
        .env("SWITCHBOARD_PIPELINE", root)
        .env("SWITCHBOARD_DATA", data)
        .env("SWITCHBOARD_STATE", data.join("no-fleet-state"))
        .env_remove("FLEET_ROOT")
        .env_remove("FLEET_STATE_DIR")
        .env_remove("FLEET_STUB_EXIT")
        .env_remove("FLEET_STUB_BADJSON")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn switchboard");
    let mut stdin = child.stdin.take().unwrap();
    for line in input {
        let _ = writeln!(stdin, "{line}");
    }
    drop(stdin);
    child.wait_with_output().unwrap()
}

fn replies(out: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn the_binary_set_pool_cap_writes_the_pipeline_config() {
    let dir = fresh("bin-cap");
    let root = pipeline(&dir);
    let data = dir.join("data");
    let cwd = fresh("bin-cap-cwd");
    let out = switchboard(
        &["mcp", "--role", "budget-manager"],
        &cwd,
        &root,
        &data,
        &[call(
            7,
            "set_pool_cap",
            json!({ "pool": "deepseek", "cap": 25 }),
        )],
    );
    assert_eq!(
        text(&replies(&out)[0]),
        (false, "set_pool_cap: done".to_string())
    );
    assert_eq!(config(&root)["pools"]["deepseek"]["cap_usd"], json!(25.0));
    assert_eq!(lines(&data.join("pool-edits.jsonl")).len(), 1);
    let items = LocalItems::new(&data).load().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].source,
        Source::PoolEdit {
            pool: "deepseek".into()
        }
    );
}

/// The snapshot `switchboard merge` reads: one open PR #12 on HEAD with CI green, with or
/// without a PASS on that head.
fn snapshot(data: &Path, pass: bool) {
    let verdicts = if pass {
        vec![ReviewVerdict {
            verdict: "PASS".into(),
            sha: HEAD.into(),
            needs_me: false,
        }]
    } else {
        vec![]
    };
    let pr = PullRequest {
        number: 12,
        title: "CLI-3: a change".into(),
        state: "open".into(),
        head_sha: HEAD.into(),
        head_ref: "sat/12-change-a1".into(),
        closes: vec![7],
        verdicts,
        ci_on_head: "success".into(),
        ..PullRequest::default()
    };
    let snap = RepoSnapshot {
        repo: "iannitram/switchboard".into(),
        read_at: NOW,
        pulls: vec![pr],
        ..RepoSnapshot::default()
    };
    SnapshotStore::new(data).save(&snap).unwrap();
}

/// The CLI's `--role project-manager merge switchboard#12`, then the same merge over MCP, on the
/// same fixture (the snapshot written fresh before each). Returns, for each face, whether it
/// merged, its text (stderr's last line for the CLI) and merge_pr.py's calls.
fn cli_then_mcp(name: &str, pass: bool) -> [(bool, String, Vec<Value>); 2] {
    let dir = fresh(name);
    let root = pipeline(&dir);
    let data = dir.join("data");
    let cwd = fresh(&format!("{name}-cwd"));
    let log = root.join("tools/merge-pr/calls.log");
    snapshot(&data, pass);
    let cli = switchboard(
        &["--role", "project-manager", "merge", "switchboard#12"],
        &cwd,
        &root,
        &data,
        &[],
    );
    let cli_err = String::from_utf8_lossy(&cli.stderr).into_owned();
    let cli_text = cli_err
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .to_string();
    let cli_calls = lines(&log);
    let _ = std::fs::remove_file(&log);
    snapshot(&data, pass);
    let out = switchboard(
        &["mcp", "--role", "project-manager"],
        &cwd,
        &root,
        &data,
        &[call(8, "merge", json!({ "repo": "switchboard", "pr": 12 }))],
    );
    let (is_error, mcp_text) = text(&replies(&out)[0]);
    [
        (cli.status.success(), cli_text, cli_calls),
        (!is_error, mcp_text, lines(&log)),
    ]
}

#[test]
fn the_binary_merge_without_a_pass_is_refused_with_the_cli_text() {
    let [cli, mcp] = cli_then_mcp("bin-nopass", false);
    assert!(!cli.0 && !mcp.0, "neither face merges without a PASS");
    assert!(
        !mcp.1.is_empty() && !mcp.1.contains("not built yet"),
        "a real refusal: {}",
        mcp.1
    );
    assert_eq!(mcp.1, cli.1, "the same refusal text as the CLI");
    assert!(
        cli.2.is_empty() && mcp.2.is_empty(),
        "merge_pr.py never ran"
    );
}

#[test]
fn the_binary_merge_with_a_pass_does_what_the_cli_merge_does() {
    let [cli, mcp] = cli_then_mcp("bin-pass", true);
    assert!(
        !mcp.1.contains("not built yet"),
        "a real merge path: {}",
        mcp.1
    );
    assert_eq!(mcp.0, cli.0, "MCP merges exactly when the CLI merges");
    assert_eq!(mcp.2, cli.2, "the same merge_pr.py calls as the CLI");
    if mcp.0 {
        assert_eq!(mcp.1, "merge: done");
    } else {
        assert_eq!(mcp.1, cli.1);
    }
}

#[test]
fn inside_a_job_worktree_neither_write_tool_answers() {
    let dir = fresh("bin-worktree");
    let root = pipeline(&dir);
    let data = dir.join("data");
    snapshot(&data, true);
    let cwd = dir.join("switchboard-wt").join("fleet-12");
    std::fs::create_dir_all(&cwd).unwrap();
    for (role, tool, args) in [
        (
            "project-manager",
            "merge",
            json!({ "repo": "switchboard", "pr": 12 }),
        ),
        (
            "budget-manager",
            "set_pool_cap",
            json!({ "pool": "deepseek", "cap": 25 }),
        ),
    ] {
        let out = switchboard(
            &["mcp", "--role", role],
            &cwd,
            &root,
            &data,
            &[call(9, tool, args)],
        );
        let reply = &replies(&out)[0];
        assert_eq!(reply["error"]["code"], -32602, "{role}");
        assert_eq!(reply["error"]["message"], format!("unknown tool: {tool}"));
    }
    assert_eq!(
        config(&root),
        serde_json::from_str::<Value>(CONFIG).unwrap()
    );
    assert!(lines(&root.join("tools/merge-pr/calls.log")).is_empty());
    assert!(!data.join("pool-edits.jsonl").exists());
}
