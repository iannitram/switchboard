//! #385: the MCP's autopilot tools. `autopilot_status` (Project Manager, satellite),
//! `autopilot_pass` (Project Manager) and `satellite_tick` (satellite) run over an in-memory
//! pipe against the headless core of `autopilot_cli.rs`: a stub `fleet.py`, a fake GitHub and
//! a fake weekly backup.

// The binary crate has no library target, so the sources are compiled in directly.
#[path = "../src/main.rs"]
mod app;
mod autopilot_support;

use std::time::Duration;

use app::autopilot::{Headless, McpCore};
use app::mcp::{serve, ReadCore, WriteCore};
use autopilot_support::{fake_backup, fake_github, headless, world, Seams, World};
use serde_json::{json, Value};
use switchboard_core::autopilot::run::{passes_lock_target, satellite_lock_target};
use switchboard_core::budget::PoolEdit;
use switchboard_core::lock::{self, FileLock};
use switchboard_core::rules::Role;

/// Answers every other read with its name.
struct FakeRead;

impl ReadCore for FakeRead {
    fn call(&self, tool: &str, _args: &Value) -> Result<Value, String> {
        Ok(json!({ "tool": tool }))
    }
}

/// No write of MCP-2 is asked for here.
struct NoWrite;

impl WriteCore for NoWrite {
    fn merge(&self, _: &str, _: u64) -> Result<(), String> {
        Err("not in this test".into())
    }
    fn pool_edit(&self, _: &str, _: &PoolEdit) -> Result<String, String> {
        Err("not in this test".into())
    }
    fn budget_refresh(&self) -> Result<Value, String> {
        Err("not in this test".into())
    }
    fn waiting(&self, _: &str) -> Result<(), String> {
        Err("not in this test".into())
    }
}

fn request(id: u64, method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
}

fn call(name: &str, arguments: Value) -> String {
    request(
        1,
        "tools/call",
        json!({ "name": name, "arguments": arguments }),
    )
}

/// Send each line to a server of `role` over `headless` and parse every reply line.
fn run(role: Option<Role>, headless: &Headless<'_>, lines: &[String]) -> Vec<Value> {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut out: Vec<u8> = Vec::new();
    let core = McpCore {
        reads: &FakeRead,
        headless,
    };
    serve(input.as_bytes(), &mut out, &core, role, &NoWrite).expect("serve");
    String::from_utf8(out)
        .expect("utf-8 output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("each reply is one JSON line"))
        .collect()
}

/// One tool call: whether the reply is an error result, and its text parsed as JSON (or the
/// text itself when it is not JSON).
fn tool(role: Role, headless: &Headless<'_>, name: &str, arguments: Value) -> (bool, Value) {
    let replies = run(Some(role), headless, &[call(name, arguments)]);
    let result = &replies[0]["result"];
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no result: {}", replies[0]));
    let value = serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()));
    (result["isError"] == true, value)
}

fn tool_names(role: Option<Role>, headless: &Headless<'_>) -> Vec<String> {
    let replies = run(role, headless, &[request(1, "tools/list", json!({}))]);
    replies[0]["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

/// Run `test` over a fresh world and its headless core.
fn with_world(tag: &str, test: impl FnOnce(&World, &Seams, &Headless<'_>)) {
    let w = world(tag);
    let seams = Seams::default();
    let (github, backup) = (fake_github(&seams), fake_backup(&seams));
    let headless = headless(&w, &seams, &github, &backup);
    test(&w, &seams, &headless);
}

const PM: Role = Role::ProjectManager;

#[test]
fn each_role_lists_its_autopilot_tools() {
    with_world("list", |_, _, h| {
        let has = |role: Option<Role>, name: &str| tool_names(role, h).iter().any(|t| t == name);
        assert!(has(Some(PM), "autopilot_status"));
        assert!(has(Some(PM), "autopilot_pass"));
        assert!(!has(Some(PM), "satellite_tick"));
        assert!(has(Some(Role::Satellite), "autopilot_status"));
        assert!(has(Some(Role::Satellite), "satellite_tick"));
        assert!(!has(Some(Role::Satellite), "autopilot_pass"));
        for role in [None, Some(Role::BudgetManager)] {
            for name in ["autopilot_status", "autopilot_pass", "satellite_tick"] {
                assert!(!has(role.clone(), name), "{role:?} {name}");
            }
        }
    });
}

#[test]
fn a_role_without_the_tool_gets_unknown_tool_and_nothing_runs() {
    with_world("refused", |w, _, h| {
        let before = w.files();
        for (role, name, args) in [
            (None, "autopilot_status", json!({})),
            (None, "autopilot_pass", json!({ "pass": "merge" })),
            (None, "satellite_tick", json!({})),
            (Some(Role::BudgetManager), "autopilot_status", json!({})),
            (
                Some(Role::BudgetManager),
                "autopilot_pass",
                json!({ "pass": "merge" }),
            ),
            (
                Some(Role::Satellite),
                "autopilot_pass",
                json!({ "pass": "merge" }),
            ),
            (
                Some(Role::Satellite),
                "autopilot_pass",
                json!({ "pass": "merge", "dry_run": true }),
            ),
            (Some(PM), "satellite_tick", json!({})),
            (Some(PM), "satellite_tick", json!({ "dry_run": true })),
        ] {
            let replies = run(role.clone(), h, &[call(name, args)]);
            assert_eq!(replies[0]["error"]["code"], -32602, "{role:?} {name}");
            assert_eq!(
                replies[0]["error"]["message"],
                format!("unknown tool: {name}")
            );
        }
        assert_eq!(w.files(), before);
        assert!(w.calls("fleet").is_empty() && w.calls("merge-pr").is_empty());
    });
}

#[test]
fn bad_arguments_are_refused_before_anything_runs() {
    with_world("args", |w, _, h| {
        let before = w.files();
        for (name, args, says) in [
            (
                "autopilot_pass",
                json!({}),
                "autopilot_pass: pass is required",
            ),
            (
                "autopilot_pass",
                json!({ "pass": 3 }),
                "autopilot_pass: pass must be a string, got 3",
            ),
            (
                "autopilot_pass",
                json!({ "pass": "merge", "dry_run": "yes" }),
                "autopilot_pass: dry_run must be true or false, got \"yes\"",
            ),
        ] {
            let replies = run(Some(PM), h, &[call(name, args)]);
            assert_eq!(replies[0]["error"]["code"], -32602);
            assert_eq!(replies[0]["error"]["message"], says);
        }
        let replies = run(
            Some(Role::Satellite),
            h,
            &[call("satellite_tick", json!({ "dry_run": 1 }))],
        );
        assert_eq!(
            replies[0]["error"]["message"],
            "satellite_tick: dry_run must be true or false, got 1"
        );
        // A pass name the tool does not run (the satellite's tick is its own tool).
        for pass in ["satellite", "sweep", "status"] {
            let (is_error, text) = tool(PM, h, "autopilot_pass", json!({ "pass": pass }));
            assert!(is_error);
            assert_eq!(
                text,
                "autopilot_pass: pass must be roadmap, review, merge or weekly"
            );
        }
        assert_eq!(w.files(), before);
        assert!(w.calls("fleet").is_empty());
    });
}

#[test]
fn the_status_read_answers_both_roles_and_writes_nothing() {
    with_world("status", |w, _, h| {
        let before = w.files();
        for role in [PM, Role::Satellite] {
            let (is_error, status) = tool(role, h, "autopilot_status", json!({}));
            assert!(!is_error, "{status}");
            assert_eq!(status["passes_running"], false);
            assert_eq!(status["running_jobs"], json!([]));
            assert_eq!(status["weekly_due"], true);
            assert_eq!(status["state"]["roadmaps_seen"], json!(["iannitram/demo"]));
            assert_eq!(status["state"]["pm_running"], Value::Null);
            assert_eq!(status["satellite"]["on"], false);
            assert_eq!(status["satellite"]["live"], false);
            assert_eq!(status["satellite"]["held"], json!([]));
        }
        assert_eq!(w.files(), before);
        // The other read tools still reach the read core.
        let replies = run(Some(PM), h, &[call("waiting", json!({}))]);
        assert_eq!(
            replies[0]["result"]["content"][0]["text"],
            json!({ "tool": "waiting" }).to_string()
        );
    });
}

#[test]
fn a_dry_run_of_each_pass_reports_and_changes_nothing() {
    with_world("dry", |w, seams, h| {
        w.satellite_on();
        let before = w.files();
        let dry = |pass: &str| {
            let (is_error, report) = tool(
                PM,
                h,
                "autopilot_pass",
                json!({ "pass": pass, "dry_run": true }),
            );
            assert!(!is_error, "{pass}: {report}");
            assert_eq!(report["pass"], pass);
            assert_eq!(report["dry_run"], true);
            assert_eq!(w.files(), before, "{pass}: a dry run writes nothing");
            report
        };
        assert_eq!(dry("review")["started"].as_array().map(Vec::len), Some(1));
        assert_eq!(dry("roadmap")["started"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            dry("merge")["merged"],
            json!([{ "repo": "iannitram/demo", "prs": [8], "closes": [12] }])
        );
        let weekly = dry("weekly");
        assert_eq!(weekly["started"].as_array().map(Vec::len), Some(1));
        assert_eq!(weekly["backup"], json!(["iannitram/demo"]));

        let (is_error, tick) = tool(
            Role::Satellite,
            h,
            "satellite_tick",
            json!({ "dry_run": true }),
        );
        assert!(!is_error, "{tick}");
        assert_eq!(tick["pass"], "satellite");
        assert_eq!(tick["dry_run"], true);
        assert_eq!(tick["lanes_started"], json!(["gemini"]));

        assert_eq!(w.files(), before);
        assert!(w.runs().is_empty(), "a dry run starts no job and no loop");
        assert!(w.calls("merge-pr").is_empty(), "a dry run merges nothing");
        assert!(seams.reads.lock().unwrap().is_empty());
        assert!(seams.backed_up.borrow().is_empty());
        assert!(!lock::held(&passes_lock_target(&w.data)));
        assert!(!lock::held(&satellite_lock_target(&w.data)));
    });
}

#[test]
fn each_poll_pass_runs_through_the_tool() {
    with_world("real", |w, seams, h| {
        let real = |pass: &str| {
            let (is_error, report) = tool(PM, h, "autopilot_pass", json!({ "pass": pass }));
            assert!(!is_error, "{pass}: {report}");
            assert_eq!(report["pass"], pass);
            assert_eq!(report["dry_run"], false);
            assert_eq!(report["errors"], json!([]));
            report
        };
        let review = real("review");
        assert_eq!(review["started"].as_array().map(Vec::len), Some(1));
        assert_eq!(w.state().reviews_started, ["iannitram/demo#7"]);
        // The job's launcher runs on beside the server; wait for it as the server does at
        // its end.
        h.finish();
        assert_eq!(w.runs().len(), 1);

        let roadmap = real("roadmap");
        assert_eq!(roadmap["started"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            w.state().pm_running.as_deref(),
            roadmap["started"][0].as_str()
        );
        h.finish();
        assert_eq!(w.runs().len(), 2);

        let weekly = real("weekly");
        assert_eq!(weekly["backup"], json!(["iannitram/demo"]));
        assert_eq!(
            weekly["backups"],
            json!([{ "repo": "iannitram/demo", "outcome": "refused", "note": "stub" }])
        );
        assert_eq!(*seams.backed_up.borrow(), ["iannitram/demo"]);
        assert_eq!(
            w.state().weekly_last_run,
            Some(autopilot_support::SUNDAY_AFTER_SIX)
        );
        // Last: the read after a merge replaces the repo's snapshot with the fake's answer.
        let merge = real("merge");
        assert_eq!(
            merge["merged"],
            json!([{ "repo": "iannitram/demo", "prs": [8], "closes": [12] }])
        );
        assert!(!w.calls("merge-pr").is_empty());
        assert_eq!(*seams.reads.lock().unwrap(), ["iannitram/demo"]);

        assert!(!lock::held(&passes_lock_target(&w.data)));
        h.finish();
    });
}

#[test]
fn a_satellite_tick_starts_its_lane_and_the_server_waits_for_the_loop() {
    with_world("tick", |w, _, h| {
        let (is_error, off) = tool(Role::Satellite, h, "satellite_tick", json!({}));
        assert!(!is_error, "{off}");
        assert_eq!(off["lanes_started"], json!([]));
        assert!(off["notes"][0].as_str().unwrap().contains("switch is off"));

        w.satellite_on();
        let (is_error, tick) = tool(Role::Satellite, h, "satellite_tick", json!({}));
        assert!(!is_error, "{tick}");
        assert_eq!(tick["lanes_started"], json!(["gemini"]));
        // The tick answers at once and the loop runs on. #432 (ruling 1): the server held the
        // satellite lock for the tick only and gave it back when the tick returned.
        assert!(!lock::held(&satellite_lock_target(&w.data)));
        let (_, status) = tool(Role::Satellite, h, "autopilot_status", json!({}));
        assert_eq!(status["satellite"]["on"], true);

        // What the server does when its client goes away: wait for the loop, report it.
        let end = h.finish().expect("a satellite ran here");
        assert_eq!(end.loops.len(), 1);
        assert_eq!(end.loops[0].lane, "gemini");
        assert_eq!(end.loops[0].exit_code, Some(0));
        assert!(!lock::held(&satellite_lock_target(&w.data)));
        assert_eq!(w.runs().len(), 1);
    });
}

#[test]
fn a_pass_is_an_error_result_while_another_run_holds_the_lock() {
    with_world("lock", |w, _, h| {
        w.satellite_on();
        let passes = FileLock::take(&passes_lock_target(&w.data), Duration::ZERO).unwrap();
        let satellite = FileLock::take(&satellite_lock_target(&w.data), Duration::ZERO).unwrap();
        let before = w.files();

        let (is_error, text) = tool(PM, h, "autopilot_pass", json!({ "pass": "review" }));
        assert!(is_error);
        assert!(
            text.as_str()
                .unwrap()
                .contains("another run holds the passes"),
            "{text}"
        );
        let (is_error, report) = tool(Role::Satellite, h, "satellite_tick", json!({}));
        assert!(is_error, "{report}");
        assert!(
            report["errors"][0]
                .as_str()
                .unwrap()
                .contains("another run holds the satellite"),
            "{report}"
        );
        assert!(w.runs().is_empty());
        assert_eq!(w.files(), before);

        let (_, status) = tool(PM, h, "autopilot_status", json!({}));
        assert_eq!(status["passes_running"], true);
        drop((passes, satellite));
        let (is_error, report) = tool(PM, h, "autopilot_pass", json!({ "pass": "review" }));
        assert!(!is_error, "{report}");
        h.finish();
    });
}

#[test]
fn a_pass_that_fails_is_an_error_result_holding_the_whole_report() {
    with_world("failing", |w, _, h| {
        w.break_the_fleet();
        let (is_error, report) = tool(PM, h, "autopilot_pass", json!({ "pass": "review" }));
        assert!(is_error);
        assert_eq!(report["pass"], "review");
        assert!(
            report["errors"][0]
                .as_str()
                .unwrap()
                .contains("budget allow"),
            "{report}"
        );
        assert!(w.runs().is_empty());
    });
}
