//! #375 (CORE-2, IT-2): `switchboard errors` and the satellite's MCP `errors` tool list the
//! fleet errors the app recorded in its data folder (`fleet-errors.jsonl`) and the open IT
//! tickets in the stored snapshots, as the built binary. The file is written here as raw lines,
//! the shape the app appends, so the test also pins that shape.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::{Issue, RepoSnapshot};

/// Two records, oldest first, with a damaged line between them. The first's stderr carries an
/// escape sequence, which the text output must not pass to the terminal (#352).
const RECORDS: &str = concat!(
    r#"{"command":["python3","fleet.py","budget","--json"],"exit_code":3,"stderr":"budget file missing\u001b[0m\n","reason":{"kind":"exit"},"undo_note":null}"#,
    "\n",
    "{\"command\":[\"python3\",\"fleet.py\"],\"exit_c\n",
    r#"{"command":["python3","fleet.py","pick","--json"],"exit_code":0,"stderr":"\u001b[31mred","reason":{"kind":"bad_json","why":"expected value at line 1 column 1"},"undo_note":null}"#,
    "\n",
);

const TEXT: &str = "\
fleet errors, newest first:
  python3 fleet.py pick --json printed output that is not JSON (expected value at line 1 column 1)
  python3 fleet.py budget --json exited 3: budget file missing\\u{1b}[0m
open IT tickets:
  owner/pipeline#12: IT: budget file missing | https://github.com/owner/pipeline/issues/12
";

/// A fresh folder under the system temp folder.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-errcli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A data folder with the RECORDS file and a pipeline snapshot holding one open IT ticket, one
/// closed one and an open issue that is not one.
fn data_with_errors(tag: &str) -> PathBuf {
    let data = scratch(tag).join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("fleet-errors.jsonl"), RECORDS).unwrap();
    let issue = |number: u64, title: &str, state: &str, labels: &[&str]| Issue {
        number,
        title: title.into(),
        state: state.into(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        ..Default::default()
    };
    SnapshotStore::new(&data)
        .save(&RepoSnapshot {
            repo: "owner/pipeline".into(),
            read_at: 1000,
            issues: vec![
                issue(12, "IT: budget file missing", "open", &["it"]),
                issue(9, "IT: old one", "closed", &["it"]),
                issue(4, "A plain bug", "open", &["bug"]),
            ],
            ..Default::default()
        })
        .unwrap();
    data
}

fn switchboard(argv: &[&str], data: &Path, cwd: &Path, stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(argv)
        .current_dir(cwd)
        .env("SWITCHBOARD_DATA", data)
        .env("SWITCHBOARD_STATE", data)
        .env("SWITCHBOARD_CONFIG", data)
        .env("SWITCHBOARD_DEV_ROOT", data)
        .env_remove("SWITCHBOARD_PIPELINE")
        .env_remove("FLEET_ROOT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run switchboard");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn errors_lists_the_recorded_errors_newest_first_then_the_open_it_tickets() {
    let data = data_with_errors("text");
    let out = switchboard(&["errors"], &data, data.parent().unwrap(), "");
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), TEXT);
    assert_eq!(
        stderr(&out),
        "switchboard errors: 1 line(s) of fleet-errors.jsonl did not read; skipped\n"
    );
}

#[test]
fn errors_json_has_every_field_of_each_record_and_ticket() {
    let data = data_with_errors("json");
    let out = switchboard(&["errors", "--json"], &data, data.parent().unwrap(), "");
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let printed: Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    assert_eq!(printed["skipped_lines"], 1);
    let errors = printed["fleet_errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
    assert_eq!(
        errors[0]["command"],
        json!(["python3", "fleet.py", "pick", "--json"])
    );
    assert_eq!(errors[0]["exit_code"], 0);
    assert_eq!(errors[0]["stderr"], "\u{1b}[31mred");
    assert_eq!(
        errors[0]["reason"],
        json!({ "kind": "bad_json", "why": "expected value at line 1 column 1" })
    );
    assert_eq!(errors[0]["undo_note"], Value::Null);
    assert_eq!(errors[1]["reason"], json!({ "kind": "exit" }));
    assert_eq!(
        errors[1]["message"],
        "python3 fleet.py budget --json exited 3: budget file missing\u{1b}[0m"
    );
    assert_eq!(
        printed["it_tickets"],
        json!([{
            "repo": "owner/pipeline",
            "number": 12,
            "title": "IT: budget file missing",
            "state": "open",
            "link": "https://github.com/owner/pipeline/issues/12"
        }])
    );
}

#[test]
fn errors_with_nothing_recorded_says_so_and_exits_0() {
    let data = scratch("empty").join("data");
    std::fs::create_dir_all(&data).unwrap();
    let out = switchboard(&["errors"], &data, data.parent().unwrap(), "");
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "no fleet errors recorded\nno open IT tickets\n"
    );
    assert_eq!(stderr(&out), "");
}

#[test]
fn errors_takes_no_other_words() {
    let data = scratch("words").join("data");
    std::fs::create_dir_all(&data).unwrap();
    let out = switchboard(&["errors", "now"], &data, data.parent().unwrap(), "");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("errors takes no other words, got now"));
}

#[test]
fn errors_writes_nothing() {
    let data = data_with_errors("readonly");
    let before = std::fs::read(data.join("fleet-errors.jsonl")).unwrap();
    let out = switchboard(&["errors"], &data, data.parent().unwrap(), "");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        std::fs::read(data.join("fleet-errors.jsonl")).unwrap(),
        before
    );
    let mut names: Vec<String> = std::fs::read_dir(&data)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["fleet-errors.jsonl", "snapshot"]);
}

fn mcp_lines(lines: &[Value]) -> String {
    lines.iter().map(|l| format!("{l}\n")).collect()
}

fn replies(out: &Output) -> Vec<Value> {
    stdout(out)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn errors_call() -> Value {
    json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": { "name": "errors", "arguments": {} } })
}

#[test]
fn the_satellite_mcp_errors_tool_answers_what_errors_json_prints() {
    let data = data_with_errors("mcp");
    let cli = switchboard(&["errors", "--json"], &data, data.parent().unwrap(), "");
    let out = switchboard(
        &["--role", "satellite", "mcp"],
        &data,
        data.parent().unwrap(),
        &mcp_lines(&[errors_call()]),
    );
    let reply = &replies(&out)[0];
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let answer: Value = serde_json::from_str(text).unwrap();
    let printed: Value = serde_json::from_str(&stdout(&cli)).unwrap();
    assert_eq!(answer, printed);
}

#[test]
fn inside_a_job_worktree_the_satellite_has_no_errors_tool() {
    let data = data_with_errors("worktree");
    let cwd = data
        .parent()
        .unwrap()
        .join("switchboard-wt")
        .join("fleet-7");
    std::fs::create_dir_all(&cwd).unwrap();
    let out = switchboard(
        &["--role", "satellite", "mcp"],
        &data,
        &cwd,
        &mcp_lines(&[errors_call()]),
    );
    let reply = &replies(&out)[0];
    assert_eq!(reply["error"]["message"], "unknown tool: errors");
}
