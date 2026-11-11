//! BUD-2 (#416): pool edits saved at the same time, from several processes, each land whole
//! (config.json, a ledger row, the Waiting item) or are refused; none is lost. Also the lock the
//! save holds (`lock::FileLock`) and three message details from the #414 review.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::budget::{save_pool_edit, PoolEdit, POOL_EDITS_LOG};
use switchboard_core::clock::FixedClock;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::lock::{lock_path, FileLock, STALE_LOCK};
use switchboard_core::waiting::{LocalItems, Source};

/// How many processes edit at once, and how many edits each makes.
const CHILDREN: usize = 6;
const EDITS: usize = 3;

/// A fresh folder under the system temp folder.
fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-bud2-lock-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The pool child `i` edits.
fn pool(i: usize) -> String {
    format!("p{i}")
}

/// The cap child `i` sets on its edit `k`.
fn cap_for(i: usize, k: usize) -> f64 {
    (100 * (i + 1) + k + 1) as f64
}

/// A pipeline root under `base` with the fleet stub and one dollar-capped pool per child, and
/// an empty data folder.
fn dirs(base: &Path) -> (PathBuf, PathBuf) {
    let root = base.join("pipeline");
    let fleet = root.join("tools").join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::create_dir_all(root.join("answers")).unwrap();
    let stub = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fleet-stub");
    std::fs::copy(stub.join("tools/fleet/fleet.py"), fleet.join("fleet.py")).unwrap();
    std::fs::copy(
        stub.join("answers/budget.json"),
        root.join("answers/budget.json"),
    )
    .unwrap();
    let pools: serde_json::Map<String, Value> = (0..CHILDREN)
        .map(|i| {
            (
                pool(i),
                serde_json::json!({ "kind": "fixed-period", "cap_usd": 1 }),
            )
        })
        .collect();
    let config = serde_json::json!({ "pools": pools });
    std::fs::write(
        fleet.join("config.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();
    let data = base.join("data");
    std::fs::create_dir_all(&data).unwrap();
    (root, data)
}

fn cap(pool: &str, cap: f64) -> PoolEdit {
    PoolEdit {
        pool: pool.into(),
        cap: Some(cap),
        ..PoolEdit::default()
    }
}

/// The folder the parent of `pool_edit_child` hands it, and which child it is.
const CHILD_BASE: &str = "SWITCHBOARD_TEST_POOL_EDIT_CHILD_BASE";
const CHILD_INDEX: &str = "SWITCHBOARD_TEST_POOL_EDIT_CHILD_INDEX";

/// `EDITS` edits of one pool in a process of its own, for the test below. Does nothing in a
/// normal test run: only the parent sets `CHILD_BASE`. Waits for `<base>/go` so every child
/// starts together. Prints one `edit <pool> <cap> ok` or `edit <pool> <cap> refused <why>` line
/// per edit.
#[test]
fn pool_edit_child() {
    let Ok(base) = std::env::var(CHILD_BASE) else {
        return;
    };
    let base = PathBuf::from(base);
    let i: usize = std::env::var(CHILD_INDEX).unwrap().parse().unwrap();
    let root = base.join("pipeline");
    let data = base.join("data");
    let waited = std::time::Instant::now();
    while !base.join("go").exists() {
        if waited.elapsed() > std::time::Duration::from_secs(60) {
            println!("child gave up: no go file");
            return;
        }
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
    // A slow re-read keeps each save open long enough for the others to overlap it.
    let bridge = FleetBridge::new(&root).with_env("FLEET_STUB_SLEEP", "0.05");
    for k in 0..EDITS {
        let value = cap_for(i, k);
        match save_pool_edit(&bridge, &cap(&pool(i), value), &data, &FixedClock::at(5)) {
            Ok(_) => println!("edit {} {value} ok", pool(i)),
            Err(e) => println!(
                "edit {} {value} refused {}",
                pool(i),
                e.to_string().replace('\n', " ")
            ),
        }
    }
}

/// #416: `CHILDREN` processes each save `EDITS` cap edits of their own pool at the same moment.
/// What must hold: each edit is reported saved or refused; every pool's cap in config.json is
/// the last cap its process saved (nothing saved is lost); the ledger holds exactly one row per
/// saved edit; each pool with a saved edit has its Waiting item; no temp or lock file is left.
#[test]
fn pool_edits_from_several_processes_all_land_or_are_refused() {
    for round in 0..3 {
        let base = scratch(&format!("procs{round}"));
        let (root, data) = dirs(&base);
        let go = base.join("go");
        let children: Vec<_> = (0..CHILDREN)
            .map(|i| {
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "pool_edit_child", "--nocapture"])
                    .env(CHILD_BASE, &base)
                    .env(CHILD_INDEX, i.to_string())
                    .env_remove("FLEET_ROOT")
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::fs::write(&go, "").unwrap();
        let said: Vec<String> = children
            .into_iter()
            .map(|child| String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).into())
            .collect();

        let mut saved: Vec<(String, f64)> = Vec::new();
        for (i, text) in said.iter().enumerate() {
            let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("edit ")).collect();
            assert_eq!(lines.len(), EDITS, "round {round}, child {i}: {text}");
            for line in lines {
                let words: Vec<&str> = line.split_whitespace().collect();
                assert!(
                    words[3] == "ok" || words[3] == "refused",
                    "round {round}: {line}"
                );
                if words[3] == "ok" {
                    saved.push((words[1].to_string(), words[2].parse().unwrap()));
                }
            }
        }
        assert!(!saved.is_empty(), "round {round}: nothing saved: {said:?}");

        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("tools/fleet/config.json")).unwrap(),
        )
        .unwrap();
        for i in 0..CHILDREN {
            let want = saved
                .iter()
                .filter(|(p, _)| *p == pool(i))
                .map(|(_, c)| *c)
                .next_back()
                .unwrap_or(1.0);
            assert_eq!(
                config["pools"][pool(i)]["cap_usd"].as_f64(),
                Some(want),
                "round {round}: pool {} lost an edit: {said:?}",
                pool(i)
            );
        }

        let ledger = std::fs::read_to_string(data.join(POOL_EDITS_LOG)).unwrap_or_default();
        let mut rows: Vec<(String, f64)> = ledger
            .lines()
            .map(|l| {
                let row: Value = serde_json::from_str(l).unwrap();
                (
                    row["pool"].as_str().unwrap().to_string(),
                    row["set"]["cap_usd"].as_f64().unwrap(),
                )
            })
            .collect();
        let mut want = saved.clone();
        rows.sort_by(|a, b| a.partial_cmp(b).unwrap());
        want.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(rows, want, "round {round}: one ledger row per saved edit");

        let items = LocalItems::new(&data).load().unwrap();
        for i in 0..CHILDREN {
            let has_item = items
                .iter()
                .any(|item| item.source == Source::PoolEdit { pool: pool(i) });
            let has_save = saved.iter().any(|(p, _)| *p == pool(i));
            assert_eq!(
                has_item,
                has_save,
                "round {round}: Waiting item for {}",
                pool(i)
            );
        }

        let left: Vec<String> = [root.join("tools/fleet"), data.clone(), data.join("waiting")]
            .iter()
            .flat_map(|dir| std::fs::read_dir(dir).unwrap())
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("tmp") || name.contains("lock"))
            .collect();
        assert!(left.is_empty(), "round {round}: left behind: {left:?}");
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[test]
fn a_held_lock_is_waited_for_then_refused_with_what_to_do() {
    let base = scratch("held");
    let target = base.join("config.json");
    std::fs::write(&target, "{}").unwrap();
    let held = FileLock::take(&target, std::time::Duration::from_secs(1)).unwrap();
    let started = std::time::Instant::now();
    let err = FileLock::take(&target, std::time::Duration::from_millis(200)).unwrap_err();
    assert!(started.elapsed() >= std::time::Duration::from_millis(200));
    let text = err.to_string();
    assert!(text.contains("config.json"), "{text}");
    assert!(text.contains("try again"), "{text}");
    assert!(
        text.contains(&lock_path(&target).display().to_string()),
        "{text}"
    );
    drop(held);
    assert!(
        !lock_path(&target).exists(),
        "dropping the lock removes its file"
    );
    FileLock::take(&target, std::time::Duration::from_millis(200)).unwrap();
}

#[test]
fn a_lock_left_by_a_dead_run_is_taken_over() {
    let base = scratch("stale");
    let target = base.join("config.json");
    let lock = lock_path(&target);
    std::fs::write(&lock, "dead run\n").unwrap();
    let old = std::time::SystemTime::now() - STALE_LOCK - std::time::Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&lock)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let held = FileLock::take(&target, std::time::Duration::from_millis(200)).unwrap();
    assert_ne!(std::fs::read_to_string(&lock).unwrap(), "dead run\n");
    drop(held);
}

/// #416 review: several takers find the same dead run's lock at once. Only one may hold it at a
/// time; before, a taker that saw the stale file could move away the fresh lock another had
/// just made, and two held it (seen in most rounds with 8 takers).
#[test]
fn a_stale_lock_found_by_many_at_once_has_one_holder_at_a_time() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    const TAKERS: usize = 8;
    let base = scratch("stale-many");
    let target = base.join("config.json");
    for round in 0..100 {
        let lock = lock_path(&target);
        std::fs::write(&lock, "dead run\n").unwrap();
        let old = std::time::SystemTime::now() - STALE_LOCK - std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let holding = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(TAKERS));
        let takers: Vec<_> = (0..TAKERS)
            .map(|_| {
                let (target, holding, most, start) =
                    (target.clone(), holding.clone(), most.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    let held = FileLock::take(&target, std::time::Duration::from_secs(10)).unwrap();
                    most.fetch_max(holding.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_micros(200));
                    holding.fetch_sub(1, Ordering::SeqCst);
                    drop(held);
                })
            })
            .collect();
        for taker in takers {
            taker.join().unwrap();
        }
        assert_eq!(
            most.load(Ordering::SeqCst),
            1,
            "round {round}: two held the lock"
        );
        let left: Vec<_> = std::fs::read_dir(&base)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n != "config.json")
            .collect();
        assert!(left.is_empty(), "round {round}: left behind: {left:?}");
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// A pipeline root with one pool, `deepseek` (cap 10), and its data folder.
fn one_pool(tag: &str) -> (PathBuf, PathBuf) {
    let base = scratch(tag);
    let root = base.join("pipeline");
    let fleet = root.join("tools").join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::create_dir_all(root.join("answers")).unwrap();
    let stub = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fleet-stub");
    std::fs::copy(stub.join("tools/fleet/fleet.py"), fleet.join("fleet.py")).unwrap();
    std::fs::copy(
        stub.join("answers/budget.json"),
        root.join("answers/budget.json"),
    )
    .unwrap();
    std::fs::write(
        fleet.join("config.json"),
        "{\"pools\": {\"deepseek\": {\"kind\": \"fixed-period\", \"cap_usd\": 10}}}\n",
    )
    .unwrap();
    let data = base.join("data");
    std::fs::create_dir_all(&data).unwrap();
    (root, data)
}

/// #414 review: a cap of a millionth of a cent is no dollar amount; a cent is the least.
#[test]
fn a_cap_under_one_cent_is_refused_and_one_cent_is_saved() {
    for tiny in [1e-300, 1e-9, 0.009] {
        let (root, data) = one_pool("tiny");
        let err = save_pool_edit(
            &FleetBridge::new(&root),
            &cap("deepseek", tiny),
            &data,
            &FixedClock::at(1),
        )
        .unwrap_err();
        assert!(err.to_string().contains("at least 0.01"), "{tiny}: {err}");
        assert!(!data.join(POOL_EDITS_LOG).exists(), "{tiny}");
    }
    let (root, data) = one_pool("cent");
    save_pool_edit(
        &FleetBridge::new(&root),
        &cap("deepseek", 0.01),
        &data,
        &FixedClock::at(1),
    )
    .unwrap();
}

/// #414 review: an empty pool name said `no pool  in config.json` (two spaces).
#[test]
fn an_empty_pool_name_is_refused_in_plain_words() {
    let (root, data) = one_pool("empty");
    for edit in [cap("", 5.0), PoolEdit::default()] {
        let err =
            save_pool_edit(&FleetBridge::new(&root), &edit, &data, &FixedClock::at(1)).unwrap_err();
        let text = err.to_string();
        assert!(!text.contains("  "), "{text}");
        assert!(text.contains("no pool name"), "{text}");
    }
}

/// #414 review: when the Waiting item cannot be added and config.json cannot be put back, the
/// error said `file: file:`. Each part is named once, as a file error.
#[cfg(unix)]
#[test]
fn an_undo_that_fails_names_each_file_once() {
    use std::os::unix::fs::PermissionsExt;
    let (root, data) = one_pool("undo-text");
    // The Waiting item cannot be added: `waiting/local.json` is a folder.
    std::fs::create_dir_all(LocalItems::new(&data).path()).unwrap();
    // The re-read makes the config folder read-only, so config.json cannot be put back.
    let fleet = root.join("tools").join("fleet");
    std::fs::write(
        fleet.join("fleet.py"),
        "import os, sys\n\
         os.chmod(os.environ['CONFIG_DIR'], 0o555)\n\
         sys.stdout.write(open(os.environ['ANSWER']).read())\n",
    )
    .unwrap();
    let bridge = FleetBridge::new(&root)
        .with_env("CONFIG_DIR", fleet.to_str().unwrap())
        .with_env("ANSWER", root.join("answers/budget.json").to_str().unwrap());
    let err =
        save_pool_edit(&bridge, &cap("deepseek", 7.0), &data, &FixedClock::at(1)).unwrap_err();
    std::fs::set_permissions(&fleet, std::fs::Permissions::from_mode(0o755)).unwrap();
    let text = err.to_string();
    assert!(text.starts_with("file: "), "{text}");
    assert!(!text.contains("file: file:"), "{text}");
    assert!(!text.contains("check: file:"), "{text}");
    assert!(text.contains("local.json"), "{text}");
    assert!(text.contains("could not be fully taken back"), "{text}");
    assert!(text.contains("config.json"), "{text}");
}
