//! One writer at a time for a file the core reads, changes and writes back (#416), across
//! threads and processes, on Linux, macOS and Windows, with no extra dependency.
//!
//! The mechanism is the backup's (#363): each run has a tag no other run has (`run_tag`), so its
//! temp files never meet another run's. What a backup does not need and a read-modify-write
//! does is mutual exclusion; that is a lock file beside the target, made with `create_new`,
//! which the OS makes atomic on every platform, held for the whole change and removed when the
//! `FileLock` drops. A second taker waits, up to its bound, then gets an error that says what to
//! do. A lock file older than `STALE_LOCK` was left by a run that died, and is taken over.

use crate::CoreError;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What a lock file's name adds to its target's.
pub const LOCK_SUFFIX: &str = ".switchboard-lock";

/// A lock file this old was left by a run that died. No change the core makes under a lock
/// takes this long: the slowest, a pool edit, re-reads the budget within the fleet time limit
/// (`fleet::FLEET_TIMEOUT_SECS`, two minutes).
pub const STALE_LOCK: Duration = Duration::from_secs(10 * 60);

/// How often a waiting taker looks again.
const POLL: Duration = Duration::from_millis(10);

/// A tag no other run has: the process, the clock and a counter. The counter keeps two runs in
/// one process apart when the clock does not tick between them (the backup's `run_tag`, #363).
pub fn run_tag() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{}-{nanos}-{seq}", std::process::id())
}

/// `<target><suffix>-<run tag>`: a sibling file name no other run uses.
pub fn temp_path(target: &Path, suffix: &str) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(format!("{suffix}-{}", run_tag()));
    PathBuf::from(name)
}

/// `<target>.switchboard-lock`.
pub fn lock_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(LOCK_SUFFIX);
    PathBuf::from(name)
}

/// A held lock on one target file; dropping it removes the lock file.
#[derive(Debug)]
pub struct FileLock {
    path: PathBuf,
}

impl FileLock {
    /// Take the lock on `target`, waiting up to `wait` while another thread or process holds
    /// it. The target's folder must exist. Out of time, the error names the lock file and says
    /// to try again, or to remove the file when nothing else is running.
    pub fn take(target: &Path, wait: Duration) -> Result<FileLock, CoreError> {
        let path = lock_path(target);
        let started = Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    // Who holds it, for a person who finds it; the lock is the file itself.
                    let _ = writeln!(file, "{}", run_tag());
                    return Ok(FileLock { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                // On Windows a lock file being removed can refuse access for a moment.
                Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied => {}
                Err(e) => return Err(CoreError::Io(format!("{}: {e}", target.display()))),
            }
            if is_stale(&path) {
                take_over(&path);
                continue;
            }
            if started.elapsed() >= wait {
                return Err(CoreError::Io(format!(
                    "{} is being changed by another run (waited {} s for {}); try again, \
                     or remove the lock file if nothing else is running",
                    target.display(),
                    wait.as_secs_f64().ceil(),
                    path.display()
                )));
            }
            std::thread::sleep(POLL);
        }
    }

    /// The lock file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write the lock file again, so a holder that works longer than `STALE_LOCK` (#385: a
    /// merge pass, a satellite loop) is not taken for a run that died.
    pub fn refresh(&self) {
        rewrite(&self.path);
    }

    /// Keep this lock fresh every `every` from a thread of its own until the `FreshLock` drops
    /// (#385 review: a lock held across calls, as an MCP server's satellite between ticks, which
    /// no one call can `keep_fresh`). The thread ends before the lock file is removed.
    pub fn kept_fresh(self, every: Duration) -> FreshLock {
        let (stop, wait) = std::sync::mpsc::channel::<()>();
        let path = self.path.clone();
        let refresher = std::thread::spawn(move || {
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = wait.recv_timeout(every) {
                rewrite(&path);
            }
        });
        FreshLock {
            stop: Some(stop),
            refresher: Some(refresher),
            _lock: self,
        }
    }

    /// Run `work` with the lock refreshed every `REFRESH_EVERY` until it ends.
    pub fn keep_fresh<T>(&self, work: impl FnOnce() -> T) -> T {
        let (done, wait) = std::sync::mpsc::channel::<()>();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                    wait.recv_timeout(REFRESH_EVERY)
                {
                    self.refresh();
                }
            });
            let out = work();
            drop(done);
            out
        })
    }
}

/// A `FileLock` a thread keeps fresh (`FileLock::kept_fresh`); dropping it stops the thread,
/// then removes the lock file.
#[derive(Debug)]
pub struct FreshLock {
    stop: Option<std::sync::mpsc::Sender<()>>,
    refresher: Option<std::thread::JoinHandle<()>>,
    _lock: FileLock,
}

impl Drop for FreshLock {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(refresher) = self.refresher.take() {
            let _ = refresher.join();
        }
    }
}

/// Write a held lock file again, so its age starts over.
fn rewrite(path: &Path) {
    let _ = std::fs::write(path, format!("{}\n", run_tag()));
}

/// How often `FileLock::keep_fresh` writes the lock file again.
pub const REFRESH_EVERY: Duration = Duration::from_secs(60);

/// Is the lock on `target` held now: its lock file is there and not stale. A read; it takes
/// nothing.
pub fn held(target: &Path) -> bool {
    let path = lock_path(target);
    path.exists() && !is_stale(&path)
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// True when the lock file at `path` is older than `STALE_LOCK`.
fn is_stale(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age > STALE_LOCK)
}

/// Remove a dead run's lock file. Only one taker at a time may: it must first make
/// `<lock>.takeover` with `create_new`, then looks again and removes the lock only if it is
/// still stale. Without that, a taker that saw the stale file could remove a fresh lock another
/// taker had just made, and two would hold it. A takeover file left by a run that died mid-
/// takeover is itself stale after `STALE_LOCK` and removed; every other taker just looks again.
fn take_over(path: &Path) {
    let mut name = path.as_os_str().to_owned();
    name.push(".takeover");
    let guard = PathBuf::from(name);
    if std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&guard)
        .is_err()
    {
        if is_stale(&guard) {
            let _ = std::fs::remove_file(&guard);
        }
        return;
    }
    if is_stale(path) {
        let _ = std::fs::remove_file(path);
    }
    let _ = std::fs::remove_file(&guard);
}
