//! Locked tests for #50 (WAIT-1: items from local sources and one merged list). Written before
//! the build (R28); the builder never edits this file.

use std::path::{Path, PathBuf};
use switchboard_core::rules::GuardedAction;
use switchboard_core::sky::priority::PriorityCopy;
use switchboard_core::waiting::{
    guarded_item, handoff_item, merge, refused_backup_item, stale_priority_item, Item, LocalItems,
    Source,
};
use switchboard_core::CoreError;

const DAY: i64 = 86_400;
/// 2026-11-08T07:00:00Z
const NOW: i64 = 1_794_121_200;

fn data_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-wait1-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn item(what: &str, since: i64, source: Source) -> Item {
    Item {
        what: what.to_string(),
        why: "because".to_string(),
        source,
        link: "https://example.invalid".to_string(),
        action: "Open".to_string(),
        since,
    }
}

fn pool(p: &str) -> Source {
    Source::PoolEdit {
        pool: p.to_string(),
    }
}

fn whats(items: &[Item]) -> Vec<&str> {
    items.iter().map(|i| i.what.as_str()).collect()
}

// ---- the item for each local source ----

#[test]
fn a_handoff_not_yet_opened() {
    assert_eq!(
        handoff_item("h-42", "Plan the backup restore", NOW),
        Item {
            what: "Open the Claude handoff: Plan the backup restore".to_string(),
            why: "A chat was handed to Claude and is not opened yet (FD-8).".to_string(),
            source: Source::Handoff {
                handoff_id: "h-42".to_string()
            },
            link: "handoff:h-42".to_string(),
            action: "Open in Claude".to_string(),
            since: NOW,
        }
    );
}

#[test]
fn a_guarded_action_from_the_cli_or_mcp() {
    let delete = GuardedAction::DeleteRepo {
        repo: "iannitram/old-thing".to_string(),
    };
    let text = "Delete iannitram/old-thing: confirm in the app";
    assert_eq!(
        guarded_item(&delete, text, NOW),
        Item {
            what: text.to_string(),
            why: "Asked for from the CLI or MCP; only the app can do it, after I confirm (CORE-3)."
                .to_string(),
            source: Source::GuardedAction {
                text: text.to_string()
            },
            link: "https://github.com/iannitram/old-thing".to_string(),
            action: "Confirm in the app".to_string(),
            since: NOW,
        }
    );
}

#[test]
fn a_guarded_action_links_to_its_repo_or_pr() {
    let restore = GuardedAction::RestoreOverLive {
        repo: "iannitram/wavcraft".to_string(),
    };
    assert_eq!(
        guarded_item(&restore, "x", NOW).link,
        "https://github.com/iannitram/wavcraft"
    );
    let merge_pr = GuardedAction::Merge {
        repo: "iannitram/switchboard".to_string(),
        pr: 61,
    };
    assert_eq!(
        guarded_item(&merge_pr, "x", NOW).link,
        "https://github.com/iannitram/switchboard/pull/61"
    );
}

#[test]
fn a_refused_backup() {
    let dir = Path::new("/home/me/Nextcloud/backups/github/iannitram/wavcraft");
    assert_eq!(
        refused_backup_item("iannitram/wavcraft", "8 issues closed by no PR", dir, NOW),
        Item {
            what: "Check the iannitram/wavcraft backup".to_string(),
            why: "The new backup was refused and the old one kept: 8 issues closed by no PR"
                .to_string(),
            source: Source::RefusedBackup {
                repo: "iannitram/wavcraft".to_string()
            },
            link: dir.display().to_string(),
            action: "Open the backup folder".to_string(),
            since: NOW,
        }
    );
}

#[test]
fn a_stale_priority_copy() {
    let copy = PriorityCopy {
        updated_at: NOW - 10 * DAY,
        order: vec!["iannitram/switchboard".to_string()],
    };
    let path = Path::new("/home/me/dev/fleet-state/priority.json");
    assert_eq!(
        stale_priority_item(&copy, path, NOW),
        Some(Item {
            what: "Refresh the priority copy".to_string(),
            why: "The priority copy is 10 days old; the sky does not trust it after 7 days."
                .to_string(),
            source: Source::StalePriority,
            link: path.display().to_string(),
            action: "Refresh in Claude".to_string(),
            since: NOW - 3 * DAY,
        })
    );
}

#[test]
fn a_fresh_priority_copy_gives_no_item() {
    let copy = PriorityCopy {
        updated_at: NOW - 7 * DAY,
        order: vec![],
    };
    assert_eq!(stale_priority_item(&copy, Path::new("p.json"), NOW), None);
    let just_stale = stale_priority_item(&copy, Path::new("p.json"), NOW + 1).unwrap();
    assert_eq!(just_stale.since, NOW);
    assert_eq!(
        just_stale.why,
        "The priority copy is 7 days old; the sky does not trust it after 7 days."
    );
}

// ---- the local store ----

#[test]
fn the_store_lives_in_the_data_folder() {
    let d = data_dir("path");
    assert_eq!(
        LocalItems::new(&d).path(),
        d.join("waiting").join("local.json").as_path()
    );
}

#[test]
fn a_missing_store_loads_empty_and_is_not_created() {
    let d = data_dir("missing");
    let store = LocalItems::new(&d);
    assert_eq!(store.load(), Ok(vec![]));
    assert_eq!(store.remove(&pool("claude")), Ok(false));
    assert!(!store.path().exists());
}

#[test]
fn added_items_load_back_in_the_order_added() {
    let d = data_dir("add");
    let store = LocalItems::new(&d);
    let a = handoff_item("h-1", "First ask", NOW);
    let b = item("Land the deepseek pool edit", NOW + 5, pool("deepseek"));
    store.add(a.clone()).unwrap();
    store.add(b.clone()).unwrap();
    assert_eq!(store.load().unwrap(), vec![a.clone(), b.clone()]);
    // A second store over the same folder reads the same file.
    assert_eq!(LocalItems::new(&d).load().unwrap(), vec![a, b]);
}

#[test]
fn the_file_is_a_pretty_json_array_ending_in_a_newline() {
    let d = data_dir("file");
    let store = LocalItems::new(&d);
    let a = item("A", NOW, pool("claude"));
    store.add(a.clone()).unwrap();
    let text = std::fs::read_to_string(store.path()).unwrap();
    assert!(text.starts_with("[\n  {"), "{text}");
    assert!(text.ends_with("]\n"), "{text}");
    let back: Vec<Item> = serde_json::from_str(&text).unwrap();
    assert_eq!(back, vec![a]);
    assert!(
        !store.path().with_extension("json.tmp").exists(),
        "the temp file is renamed away"
    );
}

#[test]
fn adding_an_item_with_the_same_source_replaces_it_in_place() {
    let d = data_dir("replace");
    let store = LocalItems::new(&d);
    store.add(item("old", NOW, pool("deepseek"))).unwrap();
    store.add(item("other", NOW, pool("claude"))).unwrap();
    let newer = item("new", NOW + 60, pool("deepseek"));
    store.add(newer.clone()).unwrap();
    let got = store.load().unwrap();
    assert_eq!(whats(&got), vec!["new", "other"]);
    assert_eq!(got[0], newer);
}

#[test]
fn remove_drops_every_item_with_that_source() {
    let d = data_dir("remove");
    let store = LocalItems::new(&d);
    let h = Source::Handoff {
        handoff_id: "h-9".to_string(),
    };
    store.add(item("keep", NOW, pool("claude"))).unwrap();
    store.add(item("drop", NOW, h.clone())).unwrap();
    assert_eq!(store.remove(&h), Ok(true));
    assert_eq!(whats(&store.load().unwrap()), vec!["keep"]);
    assert_eq!(store.remove(&h), Ok(false));
    assert_eq!(whats(&store.load().unwrap()), vec!["keep"]);
}

#[test]
fn every_source_kind_round_trips_through_the_file() {
    let d = data_dir("kinds");
    let store = LocalItems::new(&d);
    let all = vec![
        item("1", 1, Source::CalDavTask { uid: "u1".into() }),
        item(
            "2",
            2,
            Source::NeedsMe {
                repo: "o/r".into(),
                number: 3,
            },
        ),
        item(
            "3",
            3,
            Source::ReviewFail {
                repo: "o/r".into(),
                pr: 4,
            },
        ),
        item(
            "4",
            4,
            Source::LadderAtFable {
                repo: "o/r".into(),
                pr: 5,
            },
        ),
        item(
            "5",
            5,
            Source::Handoff {
                handoff_id: "h".into(),
            },
        ),
        item("6", 6, Source::GuardedAction { text: "t".into() }),
        item("7", 7, Source::ItTicket { number: 8 }),
        item("8", 8, Source::RefusedBackup { repo: "o/r".into() }),
        item("9", 9, Source::StalePriority),
        item("10", 10, pool("gemini")),
    ];
    for i in &all {
        store.add(i.clone()).unwrap();
    }
    assert_eq!(store.load().unwrap(), all);
}

#[test]
fn a_store_that_does_not_parse_is_a_parse_error_naming_the_file() {
    let d = data_dir("bad");
    let store = LocalItems::new(&d);
    std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    std::fs::write(store.path(), "not json").unwrap();
    match store.load() {
        Err(CoreError::Parse(msg)) => assert!(
            msg.starts_with(&format!("{}: ", store.path().display())),
            "{msg}"
        ),
        other => panic!("expected a parse error, got {other:?}"),
    }
    assert!(matches!(
        store.add(item("x", NOW, pool("claude"))),
        Err(CoreError::Parse(_))
    ));
    assert_eq!(
        std::fs::read_to_string(store.path()).unwrap(),
        "not json",
        "a bad file is never overwritten"
    );
}

// ---- one merged list ----

#[test]
fn merge_of_nothing_is_empty() {
    assert!(merge(vec![]).is_empty());
    assert!(merge(vec![vec![], vec![]]).is_empty());
}

#[test]
fn merge_puts_every_source_in_one_list_newest_first() {
    let snapshot_items = vec![item("b", 100, pool("1")), item("d", 300, pool("2"))];
    let local = vec![item("a", 200, pool("3"))];
    let priority = vec![item("c", 50, Source::StalePriority)];
    let got = merge(vec![snapshot_items, local, priority]);
    assert_eq!(whats(&got), vec!["d", "a", "b", "c"]);
}

#[test]
fn merge_breaks_a_tie_in_since_by_what() {
    let got = merge(vec![
        vec![item("Zebra", 100, pool("1")), item("apple", 100, pool("2"))],
        vec![item("Mango", 100, pool("3")), item("Newer", 101, pool("4"))],
    ]);
    // Byte order: capitals sort before lowercase.
    assert_eq!(whats(&got), vec!["Newer", "Mango", "Zebra", "apple"]);
}

#[test]
fn merge_keeps_every_item_even_a_full_duplicate() {
    let x = item("same", 100, pool("1"));
    let got = merge(vec![vec![x.clone()], vec![x.clone()]]);
    assert_eq!(got, vec![x.clone(), x]);
}

#[test]
fn the_code_map_marks_the_local_items_and_the_merge_present() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md");
    let map = std::fs::read_to_string(path).unwrap();
    assert!(map.contains("- WAIT-1 local items and the merge (`LocalItems`, `merge`): present."));
}
