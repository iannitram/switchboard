//! The CLI and MCP faces of #342, #343, #344, #345 and #350, run as the built binary (and
//! `refresh_with` in-process) against a temporary app data folder.

// The binary crate has no library target, so the sources are compiled in directly.
#[path = "../src/main.rs"]
mod app;
// `cli.rs` reads `super::parse_args`, so it sits at this root beside the parser's import.
#[path = "../src/cli.rs"]
#[allow(dead_code, clippy::duplicate_mod)]
mod cli;

#[allow(unused_imports)]
use app::{parse_args, Args};

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output, Stdio};

use serde_json::{json, Value};
use switchboard_core::clock::FixedClock;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::{Issue, PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::CoreError;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "switchboard-sky-gh-cli-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `owner/alpha`: open issues #1 (PR #10 with a PASS on an older commit) and #3 (on feature
/// `core`).
fn alpha() -> RepoSnapshot {
    let issue = |number: u64, labels: &[&str]| Issue {
        number,
        title: format!("Issue {number}"),
        state: "open".to_string(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        ..Default::default()
    };
    RepoSnapshot {
        repo: "owner/alpha".to_string(),
        issues: vec![issue(1, &[]), issue(3, &["feature:core"])],
        pulls: vec![PullRequest {
            number: 10,
            title: "PR 10".to_string(),
            state: "open".to_string(),
            head_sha: "bbbb".to_string(),
            head_ref: "b10".to_string(),
            closes: vec![1],
            verdicts: vec![ReviewVerdict {
                verdict: "PASS".to_string(),
                sha: "aaaa".to_string(),
                needs_me: false,
            }],
            ci_on_head: "success".to_string(),
            ..Default::default()
        }],
        roadmap_md: Some("features:\n- core: The core (release: 1.0)\n".to_string()),
        ..Default::default()
    }
}

fn data_with_alpha(tag: &str) -> PathBuf {
    let data = temp_dir(tag);
    SnapshotStore::new(&data).save(&alpha()).unwrap();
    data
}

/// A pipeline folder (no job log of its own) and a fleet state folder whose ledger has a job
/// in flight on alpha#3.
fn ledger(tag: &str) -> (PathBuf, PathBuf) {
    let pipeline = temp_dir(&format!("{tag}-pipeline"));
    let state = temp_dir(&format!("{tag}-state"));
    std::fs::create_dir_all(state.join("ledger")).unwrap();
    std::fs::write(
        state.join("ledger").join("m1.jsonl"),
        "{\"event\":\"start\",\"id\":\"owner/alpha#3\",\"lane\":\"qwen\",\"model\":\"q3\",\"machine\":\"m1\",\"attempt\":2}\n",
    )
    .unwrap();
    (pipeline, state)
}

fn command(data: &Path, ledger: Option<&(PathBuf, PathBuf)>) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_switchboard"));
    cmd.env("SWITCHBOARD_DATA", data)
        .env_remove("SWITCHBOARD_PIPELINE")
        .env_remove("FLEET_STATE_DIR")
        .env("SWITCHBOARD_STATE", data.join("no-fleet-state"));
    if let Some((pipeline, state)) = ledger {
        cmd.env("SWITCHBOARD_PIPELINE", pipeline)
            .env("SWITCHBOARD_STATE", state);
    }
    cmd
}

fn run(argv: &[&str], data: &Path, ledger: Option<&(PathBuf, PathBuf)>) -> Output {
    command(data, ledger)
        .args(argv)
        .output()
        .expect("run switchboard")
}

/// One MCP `tools/call` over the binary's stdio; the tool's answer as JSON.
fn mcp(tool: &str, arguments: Value, data: &Path, ledger: Option<&(PathBuf, PathBuf)>) -> Value {
    let mut child = command(data, ledger)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start switchboard mcp");
    let lines = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": tool, "arguments": arguments}}),
    ];
    {
        let mut stdin = child.stdin.take().unwrap();
        for line in lines {
            writeln!(stdin, "{line}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let reply: Value = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|v| v["id"] == json!(2))
        .unwrap_or_else(|| panic!("no reply: {text}"));
    let body = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text: {reply}"));
    serde_json::from_str(body).unwrap_or_else(|_| Value::String(body.to_string()))
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

// --- #342: a PASS on an older commit ---

#[test]
fn star_marks_a_pass_on_an_older_commit_than_the_head() {
    let data = data_with_alpha("older");
    let out = run(&["star", "alpha#1"], &data, None);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("\nreview: PASS on an older commit\n"),
        "{}",
        stdout(&out)
    );
    let out = run(&["--json", "star", "alpha#1"], &data, None);
    let star: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(star["pr"]["review"], json!("PASS on an older commit"));
}

// --- #343: the job in flight in every view ---

#[test]
fn star_and_the_mcp_constellation_agree_on_the_job_in_flight() {
    let data = data_with_alpha("inflight");
    let ledger = ledger("inflight");
    let out = run(&["star", "alpha#3"], &data, Some(&ledger));
    assert!(
        stdout(&out).contains("job: qwen q3 on m1, attempt 2\n"),
        "{}",
        stdout(&out)
    );

    let c = mcp(
        "constellation",
        json!({"repo": "alpha"}),
        &data,
        Some(&ledger),
    );
    let star = &c["features"][0]["stars"][0];
    assert_eq!(star["number"], json!(3), "{c}");
    assert_eq!(star["inFlight"], json!(true));
    assert_eq!(star["job"], json!("qwen q3 on m1, attempt 2"));
}

// --- #344: a stale priority copy is waiting on me ---

#[test]
fn waiting_lists_a_stale_priority_copy() {
    let data = data_with_alpha("stale");
    std::fs::write(
        data.join("priority.json"),
        r#"{"updated_at":"2026-10-01T00:00:00Z","order":["owner/alpha"]}"#,
    )
    .unwrap();
    let out = run(&["--json", "waiting"], &data, None);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let items: Value = serde_json::from_str(&stdout(&out)).unwrap();
    let sources: Vec<&Value> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| &i["source"])
        .collect();
    assert_eq!(sources, vec![&json!("StalePriority")], "{items}");

    let over_mcp = mcp("waiting", json!({}), &data, None);
    assert_eq!(over_mcp[0]["source"], json!("StalePriority"), "{over_mcp}");
}

// --- #345: the priority copy's age ---

#[test]
fn sky_text_json_and_mcp_show_the_priority_copy_age() {
    let data = data_with_alpha("age");
    std::fs::write(
        data.join("priority.json"),
        r#"{"updated_at":"2026-10-01T00:00:00Z","order":["owner/alpha"]}"#,
    )
    .unwrap();
    let copy = switchboard_core::sky::priority::parse(
        r#"{"updated_at":"2026-10-01T00:00:00Z","order":[]}"#,
    )
    .unwrap();
    use switchboard_core::clock::Clock;
    let age = switchboard_core::sky::priority::age_days(
        &copy,
        switchboard_core::clock::SystemClock.now(),
    );

    let out = run(&["sky"], &data, None);
    assert!(
        stdout(&out).ends_with(&format!("\npriority copy: {age} days old\n")),
        "{}",
        stdout(&out)
    );
    let out = run(&["--json", "sky"], &data, None);
    let rows: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(rows[0]["priority_age_days"], json!(age));
    assert_eq!(
        mcp("sky", json!({}), &data, None)[0]["priority_age_days"],
        json!(age)
    );
}

#[test]
fn sky_without_a_priority_copy_says_none() {
    let data = data_with_alpha("noage");
    let out = run(&["sky"], &data, None);
    assert!(
        stdout(&out).ends_with("\npriority copy: none\n"),
        "{}",
        stdout(&out)
    );
    let out = run(&["--json", "sky"], &data, None);
    let rows: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(rows[0]["priority_age_days"], Value::Null);
}

// --- #350: one snapshot file that does not read ---

fn with_bad_beta(tag: &str) -> (PathBuf, PathBuf) {
    let data = data_with_alpha(tag);
    let bad = SnapshotStore::new(&data).path_for("owner/beta");
    std::fs::write(&bad, "{trunc").unwrap();
    (data, bad)
}

#[test]
fn the_views_show_the_good_repo_and_name_the_bad_file() {
    let (data, bad) = with_bad_beta("views");
    let bad = bad.display().to_string();
    for argv in [&["sky"][..], &["star", "alpha#3"][..], &["waiting"][..]] {
        let out = run(argv, &data, None);
        assert_eq!(out.status.code(), Some(0), "{argv:?}: {}", stderr(&out));
        assert!(stderr(&out).contains(&bad), "{argv:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("run switchboard refresh"),
            "{argv:?}: {}",
            stderr(&out)
        );
    }
    assert!(stdout(&run(&["sky"], &data, None)).starts_with("owner/alpha: "));
}

/// GraphQL answers a tiny repository; REST answers 200 with a new ETag.
struct Fake;

impl GitHubTransport for Fake {
    fn graphql(&self, _query: &str, _variables: &Value) -> Result<Value, CoreError> {
        Ok(json!({ "repository": { "labels": { "nodes": [ { "name": "fresh" } ] } } }))
    }

    fn rest_get(&self, _path: &str, _etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        Ok(RestAnswer {
            status: 200,
            etag: Some("\"new\"".to_string()),
            body: Value::Null,
        })
    }
}

#[test]
fn refresh_replaces_the_snapshot_that_does_not_read() {
    let (data, _) = with_bad_beta("refresh");
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = cli::refresh_with(
        &Fake,
        &data,
        &FixedClock::at(1_800_000_000),
        false,
        &mut out,
        &mut err,
    );
    assert_eq!(code, ExitCode::SUCCESS, "{}", String::from_utf8_lossy(&err));
    assert_eq!(
        String::from_utf8_lossy(&out),
        "owner/alpha: refreshed\nowner/beta: refreshed\n"
    );
    let beta = SnapshotStore::new(&data)
        .load("owner/beta")
        .unwrap()
        .unwrap();
    assert_eq!(beta.labels, vec!["fresh"]);
}
