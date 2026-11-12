//! #431: state files two processes write (the window and a headless run at once).
//!
//! - `<data>/waiting/local.json`: every change is a read, change and write under the file's
//!   lock, so a writer that loaded before another's write never puts an old item back nor
//!   loses a new one.
//! - Snapshots save through a temp file of each save's own, so two saves of one repo's
//!   snapshot never share one.
//! - `autopilot.json` is written whole (temp file and rename): a read never sees half of it.
//!
//! Each writer has a store of its own, as two processes would.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use switchboard_core::autopilot::passes::{AutopilotState, STATE_FILE};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::RepoSnapshot;
use switchboard_core::lock::{self, lock_path};
use switchboard_core::waiting::{Item, LocalItems, Source};

fn data_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-431-{tag}-{}", lock::run_tag()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn item(text: &str, why: &str) -> Item {
    Item {
        what: text.to_string(),
        why: why.to_string(),
        source: Source::GuardedAction {
            text: text.to_string(),
        },
        link: String::new(),
        action: "Confirm in the app".to_string(),
        since: 1,
    }
}

/// Files in `dir` other than `keep`: temp or lock files left behind.
fn left_over(dir: &std::path::Path, keep: &[&str]) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !keep.contains(&n.as_str()))
        .collect()
}

#[test]
fn a_waiting_change_waits_for_the_lock_another_writer_holds() {
    let data = data_dir("local-held");
    let local = LocalItems::new(&data);
    local.add(item("first", "")).unwrap();
    let held = lock::FileLock::take(local.path(), std::time::Duration::from_secs(1)).unwrap();
    let writer = {
        let data = data.clone();
        std::thread::spawn(move || LocalItems::new(&data).add(item("second", "")))
    };
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert_eq!(
        local.load().unwrap().len(),
        1,
        "nothing written while the lock is held"
    );
    drop(held);
    writer.join().unwrap().unwrap();
    assert_eq!(local.load().unwrap().len(), 2);
    assert!(!lock_path(local.path()).exists());
}

fn snapshot(repo: &str, read_at: i64, size: usize) -> RepoSnapshot {
    RepoSnapshot {
        repo: repo.to_string(),
        read_at,
        labels: (0..size).map(|n| format!("label-{read_at}-{n}")).collect(),
        ..Default::default()
    }
}

/// Before: every save of a repo's snapshot wrote `<file>.json.tmp`, so two saves at once
/// renamed each other's temp file away (a save failed) or wrote into one file together.
#[test]
fn saves_of_one_repo_s_snapshot_at_once_each_land_whole() {
    let data = data_dir("snap");
    const SAVERS: usize = 6;
    const SAVES: usize = 60;
    let start = Arc::new(Barrier::new(SAVERS));
    let running: Vec<_> = (0..SAVERS)
        .map(|saver| {
            let (data, go) = (data.clone(), start.clone());
            std::thread::spawn(move || {
                let store = SnapshotStore::new(&data);
                go.wait();
                let mut failed = Vec::new();
                for n in 0..SAVES {
                    let at = (saver * 1000 + n) as i64;
                    if let Err(e) = store.save(&snapshot("owner/demo", at, 50 + saver * 40)) {
                        failed.push(e.to_string());
                    }
                }
                failed
            })
        })
        .collect();
    let failed: Vec<String> = running
        .into_iter()
        .flat_map(|t| t.join().unwrap())
        .collect();
    assert!(
        failed.is_empty(),
        "{} saves failed: {:?}",
        failed.len(),
        &failed[..1]
    );
    let store = SnapshotStore::new(&data);
    let snap = store.load("owner/demo").unwrap().expect("a snapshot");
    // Whole: one save's labels, all of them.
    let at = snap.read_at;
    assert_eq!(
        snap,
        snapshot("owner/demo", at, 50 + (at as usize / 1000) * 40)
    );
    let (_, bad) = store.load_each().unwrap();
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(
        left_over(&data.join("snapshot"), &["owner__demo.json"]),
        Vec::<String>::new()
    );
}

/// Before: `autopilot.json` was written in place, so the status read could find it empty or
/// cut short while a pass saved it.
#[test]
fn autopilot_state_is_never_read_half_written() {
    let data = data_dir("state");
    let state = |n: usize| AutopilotState {
        reviews_started: (0..200 + (n % 2) * 800)
            .map(|i| format!("owner/demo#{i}-{n}"))
            .collect(),
        ..Default::default()
    };
    state(0).save(&data).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let reader = {
        let (data, stop) = (data.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut bad = Vec::new();
            let mut reads = 0;
            while !stop.load(Ordering::SeqCst) {
                reads += 1;
                if let Err(e) = AutopilotState::load(&data) {
                    bad.push(e.to_string());
                }
            }
            (reads, bad)
        })
    };
    for n in 0..300 {
        state(n).save(&data).unwrap();
    }
    stop.store(true, Ordering::SeqCst);
    let (reads, bad) = reader.join().unwrap();
    assert!(reads > 0);
    assert!(
        bad.is_empty(),
        "{} of {reads} reads saw a half-written file: {:?}",
        bad.len(),
        &bad[..1]
    );
    assert_eq!(AutopilotState::load(&data).unwrap(), state(299));
    assert_eq!(left_over(&data, &[STATE_FILE]), Vec::<String>::new());
}
