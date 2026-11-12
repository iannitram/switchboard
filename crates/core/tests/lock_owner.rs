//! #431: a lock file carries its holder's token, process and what it is doing.
//!
//! - A holder frozen past the takeover time never touches nor removes the lock another run
//!   took over (`refresh`, `kept_fresh`, `FileLock::drop`); `keep_fresh` refreshes through
//!   `refresh`.
//! - A lock whose holder process is provably gone is taken over at once, not after
//!   `STALE_LOCK`; a holder that is alive, or whose liveness is unknown (an old lock file with
//!   no process in it), is waited for as before.
//! - Out of time, the error says what holds the lock and tells the user to wait or check;
//!   removing the lock file is mentioned only when its holder is provably gone.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};
use switchboard_core::lock::{self, lock_path, FileLock, STALE_LOCK};

const CHILD_TARGET: &str = "SB_LOCK_OWNER_CHILD_TARGET";
const CHILD_MODE: &str = "SB_LOCK_OWNER_CHILD_MODE";

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-431-lock-{tag}-{}", lock::run_tag()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn set_age(path: &Path, age: Duration) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

fn stale() -> Duration {
    STALE_LOCK + Duration::from_secs(60)
}

/// The run that froze holds `a`; its lock went stale and `b` took it over.
fn taken_over(target: &Path) -> (FileLock, FileLock, String) {
    let a = FileLock::take(target, Duration::ZERO).unwrap();
    set_age(&lock_path(target), stale());
    let b = FileLock::take(target, Duration::from_millis(500)).unwrap();
    let b_text = std::fs::read_to_string(lock_path(target)).unwrap();
    // `b` is fresh, but not so fresh that a touch would go unseen.
    set_age(&lock_path(target), Duration::from_secs(5 * 60));
    (a, b, b_text)
}

#[test]
fn a_frozen_holder_s_refresh_and_drop_leave_the_new_holder_s_lock_alone() {
    for round in 0..20 {
        let dir = scratch(&format!("frozen{round}"));
        let target = dir.join("state.json");
        let path = lock_path(&target);
        let (a, b, b_text) = taken_over(&target);
        let b_age = modified(&path);

        a.refresh();
        a.keep_fresh(|| ());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            b_text,
            "round {round}"
        );
        assert_eq!(
            modified(&path),
            b_age,
            "round {round}: the old holder touched it"
        );

        drop(a);
        assert!(
            path.exists(),
            "round {round}: the old holder removed the new lock"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), b_text);
        assert!(lock::held(&target));
        assert!(FileLock::take(&target, Duration::ZERO).is_err());

        drop(b);
        assert!(
            !path.exists(),
            "the new holder's drop still removes its own lock"
        );
    }
}

#[test]
fn a_frozen_holder_kept_fresh_by_a_thread_leaves_the_new_holder_s_lock_alone() {
    let dir = scratch("kept");
    let target = dir.join("state.json");
    let path = lock_path(&target);
    let (a, b, b_text) = taken_over(&target);
    let b_age = modified(&path);

    let fresh = a.kept_fresh(Duration::from_millis(5));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), b_text);
    assert_eq!(modified(&path), b_age, "the old holder's thread touched it");
    drop(fresh);
    assert!(path.exists(), "the old holder's drop removed the new lock");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), b_text);
    drop(b);
    assert!(!path.exists());
}

#[test]
fn a_holder_still_refreshes_its_own_lock() {
    let dir = scratch("own");
    let target = dir.join("state.json");
    let path = lock_path(&target);
    let held = FileLock::take(&target, Duration::ZERO).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    set_age(&path, stale());
    assert!(!lock::held(&target));
    held.refresh();
    assert!(lock::held(&target), "refresh made it fresh again");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        text,
        "a refresh keeps the holder's token"
    );
    drop(held);
    assert!(!path.exists());
}

/// An old-format lock file (one line, no process) is held until stale, then taken over.
#[test]
fn an_old_format_lock_is_held_until_stale() {
    let dir = scratch("old");
    let target = dir.join("state.json");
    let path = lock_path(&target);
    // The old format: `<pid>-<nanos>-<seq>`. The pid is never read from it.
    std::fs::write(&path, "999999999-1-0\n").unwrap();
    assert!(lock::held(&target));
    let err = FileLock::take(&target, Duration::from_millis(100)).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("try again"), "{text}");
    assert!(!text.contains("remove"), "{text}");
    set_age(&path, stale());
    assert!(!lock::held(&target));
    let held = FileLock::take(&target, Duration::from_millis(500)).unwrap();
    assert_ne!(std::fs::read_to_string(&path).unwrap(), "999999999-1-0\n");
    drop(held);
}

/// Not a test on its own: the process `a_lock_left_by_a_process_that_died_*` and
/// `a_lock_held_by_a_live_process_*` start, which takes the lock and either ends without
/// letting it go or holds it until told.
#[test]
fn lock_owner_child() {
    let (Ok(target), Ok(mode)) = (std::env::var(CHILD_TARGET), std::env::var(CHILD_MODE)) else {
        return;
    };
    let target = PathBuf::from(target);
    let held = FileLock::take_for(&target, Duration::from_secs(5), "a test run").unwrap();
    println!("held");
    if mode == "die" {
        // Ends with the lock file still there, as a run that was killed does.
        std::process::exit(0);
    }
    let go = target.with_extension("go");
    let give_up = Instant::now() + Duration::from_secs(30);
    while !go.exists() && Instant::now() < give_up {
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(held);
}

fn child(target: &Path, mode: &str) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lock_owner_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_TARGET, target)
        .env(CHILD_MODE, mode)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn a_lock_left_by_a_process_that_died_is_taken_over_at_once() {
    for round in 0..3 {
        let dir = scratch(&format!("died{round}"));
        let target = dir.join("thread.jsonl");
        let out = child(&target, "die").wait_with_output().unwrap();
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("held"),
            "round {round}: the child took the lock"
        );
        assert!(
            lock_path(&target).exists(),
            "the dead run left its lock file"
        );
        assert!(
            !lock::held(&target),
            "round {round}: a lock whose holder is gone is not held"
        );
        let started = Instant::now();
        let held = FileLock::take(&target, Duration::from_secs(5)).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "round {round}: took {:?}",
            started.elapsed()
        );
        drop(held);
        assert!(!lock_path(&target).exists());
    }
}

#[test]
fn a_lock_held_by_a_live_process_is_waited_for_and_named() {
    let dir = scratch("alive");
    let target = dir.join("thread.jsonl");
    let mut holder = child(&target, "hold");
    let pid = holder.id();
    let give_up = Instant::now() + Duration::from_secs(30);
    while !lock_path(&target).exists() && Instant::now() < give_up {
        std::thread::sleep(Duration::from_millis(10));
    }
    // The child writes its lock right after it makes the file.
    std::thread::sleep(Duration::from_millis(100));
    assert!(lock::held(&target));
    let err = FileLock::take(&target, Duration::from_millis(1500)).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("a test run"), "{text}");
    assert!(text.contains(&format!("process {pid}")), "{text}");
    assert!(text.contains("try again"), "{text}");
    assert!(
        !text.contains("remove"),
        "a live holder's lock is never offered for removal: {text}"
    );
    std::fs::write(target.with_extension("go"), "").unwrap();
    holder.wait().unwrap();
    assert!(!lock_path(&target).exists(), "the holder let it go");
    FileLock::take(&target, Duration::ZERO).unwrap();
}

/// What a lock is doing can change while it is held (the passes lock while the weekly backup
/// runs); the token stays, so the holder still lets it go.
#[test]
fn a_holder_can_say_what_it_is_doing_now() {
    let dir = scratch("note");
    let target = dir.join("autopilot.json");
    let held = FileLock::take_for(&target, Duration::ZERO, "the autopilot passes").unwrap();
    held.note("the weekly backup");
    let text = FileLock::take(&target, Duration::from_millis(50))
        .unwrap_err()
        .to_string();
    assert!(text.contains("the weekly backup"), "{text}");
    assert!(
        text.contains(&format!("process {}", std::process::id())),
        "{text}"
    );
    drop(held);
    assert!(!lock_path(&target).exists());
}
