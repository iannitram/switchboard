//! #419: every write to a thread file holds the file's lock (`lock::FileLock`), so writers in
//! different threads or processes (the window and the CLI or MCP) cannot lose each other's rows.
//! `ThreadStore::upsert` used to read the file, change it and rename a new one over it with no
//! lock: a row appended between its read and its rename was gone (93 of 200 in the report).

use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::joblog::JobRow;
use switchboard_core::lock::{lock_path, FileLock};
use switchboard_core::CoreError;

fn data_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-fd419-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn turn(id: &str) -> JobRow {
    JobRow {
        id: Some(id.to_string()),
        role: Some("assistant".to_string()),
        text: Some(format!("answer {id}")),
        ..Default::default()
    }
}

fn rating(rating: &str) -> JobRow {
    JobRow {
        id: Some("t#rating-1".to_string()),
        rating: Some(rating.to_string()),
        ..Default::default()
    }
}

const WRITERS: usize = 4;
const ROUNDS: usize = 50;

#[test]
fn turns_appended_while_a_rating_is_pressed_again_are_all_kept() {
    let dir = data_dir("stress");
    ThreadStore::new(&dir).append("t", &rating("up")).unwrap();
    let start = Arc::new(Barrier::new(WRITERS * 2));
    let mut running = Vec::new();
    for writer in 0..WRITERS {
        // Each writer has a store of its own, as two processes would.
        let (mine, go) = (dir.clone(), start.clone());
        running.push(std::thread::spawn(move || {
            let store = ThreadStore::new(&mine);
            go.wait();
            for n in 0..ROUNDS {
                store.append("t", &turn(&format!("w{writer}-{n}"))).unwrap();
            }
        }));
        let (dir, go) = (dir.clone(), start.clone());
        running.push(std::thread::spawn(move || {
            let store = ThreadStore::new(&dir);
            go.wait();
            for n in 0..ROUNDS {
                let press = if (n + writer) % 2 == 0 { "down" } else { "up" };
                store.upsert("t", &rating(press)).unwrap();
            }
        }));
    }
    for thread in running {
        thread.join().unwrap();
    }
    let rows = ThreadStore::new(&dir).read("t").unwrap();
    let ratings = rows.iter().filter(|r| r.rating.is_some()).count();
    assert_eq!(ratings, 1, "a second press replaces the first");
    let mut lost = Vec::new();
    for writer in 0..WRITERS {
        for n in 0..ROUNDS {
            let id = format!("w{writer}-{n}");
            if !rows.iter().any(|r| r.id.as_deref() == Some(id.as_str())) {
                lost.push(id);
            }
        }
    }
    assert!(lost.is_empty(), "{} turns lost: {lost:?}", lost.len());
    assert_eq!(rows.len(), WRITERS * ROUNDS + 1);
    // Each writer's own turns are still in the order it stored them.
    for writer in 0..WRITERS {
        let mine: Vec<usize> = rows
            .iter()
            .filter_map(|r| r.id.as_deref()?.strip_prefix(&format!("w{writer}-")))
            .map(|n| n.parse().unwrap())
            .collect();
        assert_eq!(mine, (0..ROUNDS).collect::<Vec<_>>());
    }
    assert!(
        !lock_path(&dir.join("threads").join("t.jsonl")).exists(),
        "the lock file goes when the write is done"
    );
    assert_eq!(ThreadStore::new(&dir).list().unwrap(), vec!["t"]);
}

#[test]
fn a_write_waits_for_the_lock_another_writer_holds() {
    let dir = data_dir("held");
    let store = ThreadStore::new(&dir);
    store.append("t", &turn("first")).unwrap();
    let file = dir.join("threads").join("t.jsonl");
    let held = FileLock::take(&file, std::time::Duration::from_secs(1)).unwrap();
    let writer = {
        let dir = dir.clone();
        std::thread::spawn(move || ThreadStore::new(&dir).append("t", &turn("second")))
    };
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert_eq!(
        store.read("t").unwrap().len(),
        1,
        "nothing is written while another writer holds the lock"
    );
    drop(held);
    writer.join().unwrap().unwrap();
    assert_eq!(store.read("t").unwrap().len(), 2);
}

#[test]
fn rows_appended_together_stay_together() {
    let dir = data_dir("pairs");
    let start = Arc::new(Barrier::new(WRITERS));
    let mut running = Vec::new();
    for writer in 0..WRITERS {
        let (dir, go) = (dir.clone(), start.clone());
        running.push(std::thread::spawn(move || {
            let store = ThreadStore::new(&dir);
            go.wait();
            for n in 0..ROUNDS {
                let pair = [
                    turn(&format!("q{writer}-{n}")),
                    turn(&format!("a{writer}-{n}")),
                ];
                store.append_all("t", &pair).unwrap();
            }
        }));
    }
    for thread in running {
        thread.join().unwrap();
    }
    let rows = ThreadStore::new(&dir).read("t").unwrap();
    assert_eq!(rows.len(), WRITERS * ROUNDS * 2);
    for pair in rows.chunks(2) {
        let (q, a) = (
            pair[0].id.as_deref().unwrap(),
            pair[1].id.as_deref().unwrap(),
        );
        assert_eq!(q.strip_prefix('q'), a.strip_prefix('a'), "{q} then {a}");
    }
}

#[test]
fn change_rewrites_the_row_in_the_file_now_and_says_when_there_is_none() {
    let dir = data_dir("change");
    let store = ThreadStore::new(&dir);
    store.append("t", &turn("a")).unwrap();
    store.append("t", &rating("down")).unwrap();
    let mark = |row: JobRow| JobRow {
        moved: Some(true),
        ..row
    };
    assert_eq!(store.change("t", "t#rating-1", mark), Ok(true));
    let rows = store.read("t").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].rating.as_deref(), Some("down"));
    assert_eq!(rows[1].moved, Some(true));
    assert_eq!(store.change("t", "t#rating-9", mark), Ok(false));
    assert_eq!(store.read("t").unwrap(), rows);
    assert!(matches!(
        store.change("../t", "x", mark),
        Err(CoreError::Refused(_))
    ));
}
