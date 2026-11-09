//! Locked tests for WAIT-W (the `waiting` command: the one list the Waiting view shows).
//! Written before the build (R28); the builder never edits this file.

use serde_json::json;
use std::path::PathBuf;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::{Issue, PullRequest, RepoSnapshot};
use switchboard_core::waiting::{assemble, from_snapshots, load, merge, Item, LocalItems, Source};
use switchboard_core::CoreError;

/// 2026-11-08T07:00:00Z
const NOW: i64 = 1_794_121_200;

fn data_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let d = std::env::temp_dir().join(format!("sb-waitw-{tag}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn issue(number: u64, title: &str, labels: &[&str]) -> Issue {
    Issue {
        number,
        title: title.to_string(),
        state: "open".to_string(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        ..Issue::default()
    }
}

fn snap(repo: &str, read_at: i64, issues: Vec<Issue>, pulls: Vec<PullRequest>) -> RepoSnapshot {
    RepoSnapshot {
        repo: repo.to_string(),
        read_at,
        issues,
        pulls,
        ..RepoSnapshot::default()
    }
}

/// alpha read an hour ago: issue #7 needs me; PR #12 reached Fable.
fn alpha() -> RepoSnapshot {
    let fable = PullRequest {
        number: 12,
        title: "Snapshot store".to_string(),
        state: "open".to_string(),
        labels: vec!["review:fable".to_string()],
        ..PullRequest::default()
    };
    snap(
        "iannitram/alpha",
        NOW - 3_600,
        vec![
            issue(7, "Pick the icon", &["needs-ian"]),
            issue(8, "Not mine", &[]),
        ],
        vec![fable],
    )
}

/// beta read ten minutes ago: issue #3 needs me.
fn beta() -> RepoSnapshot {
    snap(
        "iannitram/beta",
        NOW - 600,
        vec![issue(3, "Name the app", &["needs-ian"])],
        vec![],
    )
}

fn local(what: &str, since: i64, pool: &str) -> Item {
    Item {
        what: what.to_string(),
        why: "A pool edit to land through review (U5).".to_string(),
        source: Source::PoolEdit {
            pool: pool.to_string(),
        },
        link: "/x/pool-edits.jsonl".to_string(),
        action: "Open the edit".to_string(),
        since,
    }
}

fn locals() -> Vec<Item> {
    vec![
        local("Land the claude pool edit", NOW - 60, "claude"),
        local("Land the qwen pool edit", NOW - 7_200, "qwen"),
    ]
}

fn whats(items: &[Item]) -> Vec<&str> {
    items.iter().map(|i| i.what.as_str()).collect()
}

// --- assemble: the snapshot's items and the local ones, one list, newest first ---

#[test]
fn nothing_anywhere_is_no_items() {
    assert!(assemble(&[], &[]).is_empty());
    assert!(assemble(&[snap("iannitram/empty", NOW, vec![], vec![])], &[]).is_empty());
}

#[test]
fn snapshot_and_local_items_are_one_list_newest_first() {
    let items = assemble(&[alpha(), beta()], &locals());
    assert_eq!(
        whats(&items),
        vec![
            "Land the claude pool edit",          // NOW - 60
            "iannitram/beta#3: Name the app",     // NOW - 600
            "iannitram/alpha#12: Snapshot store", // NOW - 3600
            "iannitram/alpha#7: Pick the icon",   // NOW - 3600
            "Land the qwen pool edit",            // NOW - 7200
        ]
    );
}

#[test]
fn equal_times_are_ordered_by_what() {
    let items = assemble(&[], &[local("b edit", NOW, "b"), local("a edit", NOW, "a")]);
    assert_eq!(whats(&items), vec!["a edit", "b edit"]);
}

#[test]
fn assemble_is_the_merge_of_the_snapshot_items_and_the_local_items() {
    let snaps = vec![beta(), alpha()];
    let expected = merge(vec![from_snapshots(&snaps), locals()]);
    assert_eq!(assemble(&snaps, &locals()), expected);
}

#[test]
fn an_archived_repo_gives_nothing_and_the_local_items_stay() {
    let mut archived = alpha();
    archived.archived = true;
    let items = assemble(&[archived], &locals());
    assert_eq!(
        whats(&items),
        vec!["Land the claude pool edit", "Land the qwen pool edit"]
    );
}

#[test]
fn the_inputs_are_not_changed() {
    let snaps = vec![alpha(), beta()];
    let before = snaps.clone();
    let l = locals();
    let _ = assemble(&snaps, &l);
    assert_eq!(snaps, before);
    assert_eq!(l, locals());
}

// --- The JSON is the face's WaitingItemData (app/src/brocrew/WaitingItem.tsx, #60) ---

#[test]
fn an_item_has_exactly_the_face_field_names() {
    let items = assemble(&[alpha()], &[]);
    let v = serde_json::to_value(&items).unwrap();
    let mut keys: Vec<String> = v[1].as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["action", "link", "since", "source", "what", "why"]
    );
    assert_eq!(
        v[1],
        json!({
            "what": "iannitram/alpha#7: Pick the icon",
            "why": "Labelled needs-ian.",
            "source": { "NeedsMe": { "repo": "iannitram/alpha", "number": 7 } },
            "link": "https://github.com/iannitram/alpha/issues/7",
            "action": "Open issue",
            "since": NOW - 3_600
        })
    );
}

// --- load: the snapshot store and the local items in the app data folder ---

#[test]
fn load_with_an_empty_folder_is_no_items_and_writes_nothing() {
    let dir = data_dir("empty");
    assert_eq!(load(&dir).unwrap(), Vec::<Item>::new());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
}

#[test]
fn load_reads_the_store_and_the_local_items() {
    let dir = data_dir("full");
    let store = SnapshotStore::new(&dir);
    store.save(&alpha()).unwrap();
    store.save(&beta()).unwrap();
    let li = LocalItems::new(&dir);
    for item in locals() {
        li.add(item).unwrap();
    }
    assert_eq!(load(&dir).unwrap(), assemble(&[alpha(), beta()], &locals()));
}

#[test]
fn load_with_no_snapshot_still_shows_the_local_items() {
    let dir = data_dir("nosnap");
    LocalItems::new(&dir)
        .add(local("Land the claude pool edit", NOW - 60, "claude"))
        .unwrap();
    assert_eq!(
        whats(&load(&dir).unwrap()),
        vec!["Land the claude pool edit"]
    );
}

#[test]
fn load_with_no_local_items_shows_the_snapshot_items() {
    let dir = data_dir("nolocal");
    SnapshotStore::new(&dir).save(&beta()).unwrap();
    assert_eq!(
        whats(&load(&dir).unwrap()),
        vec!["iannitram/beta#3: Name the app"]
    );
}

#[test]
fn load_with_a_bad_local_file_is_its_parse_error() {
    let dir = data_dir("badlocal");
    SnapshotStore::new(&dir).save(&beta()).unwrap();
    let path = LocalItems::new(&dir).path().to_path_buf();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ not json").unwrap();
    match load(&dir).unwrap_err() {
        CoreError::Parse(msg) => {
            assert!(msg.starts_with(&format!("{}: ", path.display())), "{msg}")
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn load_with_a_bad_snapshot_file_is_its_parse_error() {
    let dir = data_dir("badsnap");
    let path = SnapshotStore::new(&dir).path_for("iannitram/bad");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "[1, 2").unwrap();
    match load(&dir).unwrap_err() {
        CoreError::Parse(msg) => {
            assert!(msg.starts_with(&format!("{}: ", path.display())), "{msg}")
        }
        other => panic!("got {other:?}"),
    }
}

// --- The shell registers the command; the code map says so ---

#[test]
fn the_shell_registers_the_waiting_command() {
    let main = include_str!("../../../app/src-tauri/src/main.rs");
    let handler = main
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("generate_handler! in main.rs");
    assert!(handler.contains("commands::waiting,"), "{handler}");

    let commands = include_str!("../../../app/src-tauri/src/commands.rs");
    assert!(
        commands.contains("pub fn waiting("),
        "commands.rs has no `pub fn waiting(`"
    );
    assert!(
        commands.contains("switchboard_core::waiting::load("),
        "commands.rs: the waiting command must call `switchboard_core::waiting::load(`"
    );
    assert!(
        commands.contains("FaceError::new(\"waiting\""),
        "commands.rs: the waiting command's errors must name `waiting`"
    );
}

#[test]
fn the_code_map_lists_the_waiting_command_present() {
    let map = include_str!("../../../docs/code-map.md");
    assert!(
        map.contains("- WAIT-W command (`assemble`, `load`, `waiting`): present."),
        "docs/code-map.md has no WAIT-W command line"
    );
}
