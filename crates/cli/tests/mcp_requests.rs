//! #347: the MCP tools that file and turn down an archive or restore request. The
//! project-manager role holds `request_archive`, `request_restore` and `drop_request`; they
//! only add or remove a Waiting on you item. No MCP tool, under any role, can confirm one: the
//! typed confirm stays with the owner in the app. The real `CoreWrites` runs over a temp app
//! data folder with no pipeline checkout and a merge that panics, so anything beyond the
//! Waiting list would fail the test.

// The binary crate has no library target, so the module is compiled in directly.
#[path = "../src/mcp.rs"]
#[allow(dead_code)]
mod mcp;

use std::path::{Path, PathBuf};

use mcp::{serve, CoreWrites, ReadCore};
use serde_json::{json, Value};
use switchboard_core::clock::FixedClock;
use switchboard_core::confirm::HeldRecords;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::RepoSnapshot;
use switchboard_core::rules::Role;
use switchboard_core::waiting::{Item, LocalItems, Source};

const NOW: i64 = 1_794_146_400;
const ARCHIVE: &str = "I was asked from MCP to archive o/switchboard";
const RESTORE: &str = "I was asked from MCP to restore over o/switchboard";

struct FakeRead;

impl ReadCore for FakeRead {
    fn call(&self, _: &str, _: &Value) -> Result<Value, String> {
        Ok(json!([]))
    }
}

/// A fresh app data folder that knows `o/switchboard`, under a plain (not job-worktree) folder.
fn data(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-mcp-ask-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let snap = RepoSnapshot {
        repo: "o/switchboard".to_string(),
        read_at: 2000,
        ..Default::default()
    };
    SnapshotStore::new(&dir).save(&snap).unwrap();
    dir
}

fn call(id: u64, name: &str, arguments: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": arguments } })
        .to_string()
}

/// Send each line to the server over the real write core and parse every reply.
fn run(role: Option<Role>, data: &Path, lines: &[String]) -> Vec<Value> {
    let merge = |_: &str, _: u64| -> Result<(), String> { panic!("no request tool merges") };
    let writes = CoreWrites {
        merge: &merge,
        fleet: None,
        data: Some(data.to_path_buf()),
        clock: &FixedClock::at(NOW),
    };
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut out: Vec<u8> = Vec::new();
    serve(input.as_bytes(), &mut out, &FakeRead, role, &writes).expect("serve");
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("each reply is one JSON line"))
        .collect()
}

fn tools(role: Option<Role>, data: &Path) -> Vec<Value> {
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} });
    run(role, data, &[list.to_string()])[0]["result"]["tools"]
        .as_array()
        .expect("tools array")
        .clone()
}

fn items(data: &Path) -> Vec<Item> {
    LocalItems::new(data).load().unwrap()
}

/// The texts of the requests waiting now.
fn waiting(data: &Path) -> Vec<String> {
    items(data)
        .into_iter()
        .map(|item| match item.source {
            Source::GuardedAction { text } => text,
            other => panic!("{other:?}"),
        })
        .collect()
}

fn text(reply: &Value) -> (&str, bool) {
    (
        reply["result"]["content"][0]["text"].as_str().unwrap(),
        reply["result"]["isError"].as_bool().unwrap(),
    )
}

/// Every file under `dir`, relative, sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(next) = todo.pop() {
        for entry in std::fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                todo.push(path);
            } else {
                found.push(path.strip_prefix(dir).unwrap().display().to_string());
            }
        }
    }
    found.sort();
    found
}

const ROLES: [Option<Role>; 4] = [
    None,
    Some(Role::ProjectManager),
    Some(Role::BudgetManager),
    Some(Role::Satellite),
];

#[test]
fn only_the_project_manager_holds_the_request_tools() {
    let dir = data("list");
    for role in ROLES {
        let names: Vec<String> = tools(role.clone(), &dir)
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        let held: Vec<&str> = names
            .iter()
            .map(String::as_str)
            .filter(|n| ["request_archive", "request_restore", "drop_request"].contains(n))
            .collect();
        if role == Some(Role::ProjectManager) {
            assert_eq!(held, ["request_archive", "request_restore", "drop_request"]);
        } else {
            assert_eq!(held, Vec::<&str>::new(), "{role:?}");
            for (id, tool) in ["request_archive", "request_restore", "drop_request"]
                .iter()
                .enumerate()
            {
                let args = json!({ "repo": "switchboard", "kind": "archive" });
                let reply = &run(role.clone(), &dir, &[call(id as u64, tool, args)])[0];
                assert_eq!(reply["error"]["message"], format!("unknown tool: {tool}"));
            }
        }
    }
    assert_eq!(waiting(&dir), Vec::<String>::new());
}

#[test]
fn a_request_only_files_a_waiting_item_that_links_to_the_repo() {
    let dir = data("file");
    let before = files(&dir);
    let replies = run(
        Some(Role::ProjectManager),
        &dir,
        &[
            call(1, "request_archive", json!({ "repo": "switchboard" })),
            call(2, "request_restore", json!({ "repo": "o/switchboard" })),
            // Asking again keeps one item.
            call(3, "request_archive", json!({ "repo": "o/switchboard" })),
        ],
    );
    assert_eq!(
        text(&replies[0]),
        (
            "archive o/switchboard: needs a confirm in the app; the request is in Waiting on you",
            false
        )
    );
    assert_eq!(
        text(&replies[1]),
        (
            "restore over o/switchboard: needs a confirm in the app; the request is in Waiting on you",
            false
        )
    );
    assert!(!text(&replies[2]).1);
    assert_eq!(waiting(&dir), [ARCHIVE, RESTORE]);
    for item in items(&dir) {
        assert_eq!(item.link, "https://github.com/o/switchboard");
        assert_eq!(item.action, "Confirm in the app");
        assert_eq!(item.since, NOW);
    }
    // The Waiting list is the only thing written.
    let mut after = files(&dir);
    after.retain(|f| !before.contains(f));
    assert_eq!(after, ["waiting/local.json"]);
}

#[test]
fn a_repo_switchboard_does_not_know_files_nothing() {
    let dir = data("unknown");
    let replies = run(
        Some(Role::ProjectManager),
        &dir,
        &[
            call(1, "request_archive", json!({ "repo": "nope" })),
            call(2, "request_restore", json!({ "repo": "o/gone" })),
            call(3, "request_restore", json!({})),
            call(4, "request_archive", json!({ "repo": 7 })),
        ],
    );
    assert_eq!(
        text(&replies[0]),
        ("request_archive: no such repo: nope", true)
    );
    assert_eq!(
        text(&replies[1]),
        ("request_restore: no such repo: o/gone", true)
    );
    assert!(replies[2]["error"]["message"].is_string());
    assert!(replies[3]["error"]["message"].is_string());
    assert_eq!(waiting(&dir), Vec::<String>::new());
}

#[test]
fn no_mcp_tool_can_confirm_under_any_role() {
    for (n, role) in ROLES.into_iter().enumerate() {
        let dir = data(&format!("confirm-{n}"));
        // A request is waiting, as the CLI or MCP files it.
        run(
            Some(Role::ProjectManager),
            &dir,
            &[
                call(1, "request_archive", json!({ "repo": "switchboard" })),
                call(2, "request_restore", json!({ "repo": "switchboard" })),
            ],
        );
        let before = files(&dir);
        let list = tools(role.clone(), &dir);
        // No tool is named for a confirm and none takes the name typed or a confirmed flag.
        for tool in &list {
            let name = tool["name"].as_str().unwrap();
            assert!(!name.contains("confirm"), "{role:?} lists {name}");
            let props = tool["inputSchema"]["properties"].as_object().unwrap();
            for key in ["typed", "confirmed", "confirm", "confirmed_in_app"] {
                assert!(!props.contains_key(key), "{role:?} {name} takes {key}");
            }
        }
        // The app's command and its likely spellings are not tools.
        let said = json!({ "text": ARCHIVE, "repo": "o/switchboard", "typed": "o/switchboard", "confirmed": true });
        for (id, name) in [
            "confirm_guarded",
            "confirm",
            "confirm_request",
            "archive",
            "delete",
            "restore",
        ]
        .iter()
        .enumerate()
        {
            let reply = &run(role.clone(), &dir, &[call(id as u64, name, said.clone())])[0];
            assert_eq!(
                reply["error"]["message"],
                format!("unknown tool: {name}"),
                "{role:?}"
            );
        }
        // Every tool the role does hold, called with the name typed and `confirmed: true`,
        // leaves both requests waiting and writes nothing beyond the Waiting list.
        for (id, tool) in list.iter().enumerate() {
            let name = tool["name"].as_str().unwrap();
            if [
                "merge",
                "drop_request",
                "set_pool_cap",
                "pool_edit",
                "budget_refresh",
            ]
            .contains(&name)
            {
                continue;
            }
            let mut args = said.clone();
            args["repo"] = json!("switchboard");
            run(role.clone(), &dir, &[call(id as u64, name, args)]);
        }
        assert_eq!(waiting(&dir), [ARCHIVE, RESTORE], "{role:?}");
        assert_eq!(files(&dir), before, "{role:?}");
        assert_eq!(HeldRecords::new(&dir).get(RESTORE).unwrap(), None);
    }
}

#[test]
fn drop_request_takes_the_request_out_and_does_nothing_else() {
    let dir = data("drop");
    let pm = Some(Role::ProjectManager);
    run(
        pm.clone(),
        &dir,
        &[
            call(1, "request_archive", json!({ "repo": "switchboard" })),
            call(2, "request_restore", json!({ "repo": "switchboard" })),
        ],
    );
    let before = files(&dir);
    let replies = run(
        pm.clone(),
        &dir,
        &[call(
            1,
            "drop_request",
            json!({ "kind": "archive", "repo": "switchboard" }),
        )],
    );
    assert_eq!(
        text(&replies[0]),
        (
            "the request to archive o/switchboard was dropped; nothing was archived, pushed or queued",
            false
        )
    );
    assert_eq!(waiting(&dir), [RESTORE]);
    assert_eq!(files(&dir), before);

    // What is not waiting is refused, as is a kind that is neither, and nothing changes.
    let replies = run(
        pm,
        &dir,
        &[
            call(
                1,
                "drop_request",
                json!({ "kind": "archive", "repo": "o/switchboard" }),
            ),
            call(
                2,
                "drop_request",
                json!({ "kind": "restore", "repo": "o/gone" }),
            ),
            call(
                3,
                "drop_request",
                json!({ "kind": "merge", "repo": "switchboard" }),
            ),
            call(
                4,
                "drop_request",
                json!({ "kind": "restore", "repo": "nope" }),
            ),
        ],
    );
    assert_eq!(
        text(&replies[0]),
        (
            "archive o/switchboard: no such request is waiting on you; nothing was dropped",
            true
        )
    );
    assert_eq!(
        text(&replies[1]),
        (
            "restore over o/gone: no such request is waiting on you; nothing was dropped",
            true
        )
    );
    assert_eq!(
        replies[2]["error"]["message"],
        "drop_request needs kind (archive or restore) and repo (string)"
    );
    assert_eq!(
        text(&replies[3]),
        ("drop_request: no such repo: nope", true)
    );
    assert_eq!(waiting(&dir), [RESTORE]);
}
