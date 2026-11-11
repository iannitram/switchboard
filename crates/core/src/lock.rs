//! One writer at a time for a file the core reads, changes and writes back (#416), across
//! threads and processes, on Linux, macOS and Windows, with no extra dependency.
//!
//! The mechanism is the backup's (#363): each run has a tag no other run has (`run_tag`), so its
//! temp files never meet another run's. What a backup does not need and a read-modify-write
//! does is mutual exclusion; that is a lock file beside the target, made with `create_new`,
//! which the OS makes atomic on every platform, held for the whole change and removed when the
//! `FileLock` drops. A second taker waits, up to its bound, then gets an error that says what
//! holds the lock and what to do. A lock file older than `STALE_LOCK` was left by a run that
//! died, and is taken over.
//!
//! #431: the lock file names its holder: a token no other holder has, the holder's process (id
//! and start time) and what it is doing. A holder refreshes and removes the file only while it
//! still holds its token, so one frozen past the takeover time never touches the lock another
//! run took over. A lock whose holder's process is provably gone is taken over at once; one
//! whose holder may be alive (an old lock file with no process in it, a process list that could
//! not be read) waits for `STALE_LOCK` as before.

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

/// Write `text` to `target` whole (#431): to a temp file of this write's own (`temp_path`),
/// then renamed over `target`, so a reader finds the old file or the new one, never half of
/// one, and two writers at once never share a temp file. A failed write leaves `target` as it
/// was and removes the temp file.
pub fn write_whole(target: &Path, text: &[u8]) -> Result<(), CoreError> {
    let tmp = temp_path(target, ".tmp");
    let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", target.display()));
    if let Err(e) = std::fs::write(&tmp, text) {
        let _ = std::fs::remove_file(&tmp);
        return Err(io(e));
    }
    let mut tries = 0;
    loop {
        match std::fs::rename(&tmp, target) {
            Ok(()) => return Ok(()),
            // On Windows a file another rename is replacing refuses access for a moment.
            Err(e)
                if cfg!(windows)
                    && e.kind() == std::io::ErrorKind::PermissionDenied
                    && tries < 100 =>
            {
                tries += 1;
                std::thread::sleep(POLL);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(io(e));
            }
        }
    }
}

/// `<target>.switchboard-lock`.
pub fn lock_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(LOCK_SUFFIX);
    PathBuf::from(name)
}

/// What a lock file says about its holder (#431): a token no other holder has, its process
/// (the id and, where the system gives one, its start time, as `autopilot::loops` records a
/// loop's) and what it is doing. Written as `key=value` lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Holder {
    token: String,
    pid: Option<u32>,
    start: Option<String>,
    what: Option<String>,
}

impl Holder {
    /// A new holder in this process.
    fn new(what: Option<&str>) -> Holder {
        Holder {
            token: run_tag(),
            pid: Some(std::process::id()),
            start: own_start(),
            what: what.map(one_line),
        }
    }

    fn text(&self) -> String {
        let mut text = format!("token={}\n", self.token);
        if let Some(pid) = self.pid {
            text.push_str(&format!("pid={pid}\n"));
        }
        if let Some(start) = &self.start {
            text.push_str(&format!("start={start}\n"));
        }
        if let Some(what) = &self.what {
            text.push_str(&format!("holder={what}\n"));
        }
        text
    }

    /// A lock file's text. One written before #431 is one line, the token, with no process in
    /// it; that holder's liveness is unknown, so it is held until stale.
    fn parse(text: &str) -> Holder {
        if !text.lines().any(|line| line.starts_with("token=")) {
            return Holder {
                token: text.trim().to_string(),
                ..Holder::default()
            };
        }
        let mut holder = Holder::default();
        // Only whole lines: a file read while its holder writes it may end mid-line, and a
        // process id cut short (`pid=12` of `pid=1234`) would name another process.
        let whole = text.rfind('\n').map_or("", |end| &text[..end]);
        for line in whole.lines() {
            match line.split_once('=') {
                Some(("token", v)) => holder.token = v.to_string(),
                Some(("pid", v)) => holder.pid = v.trim().parse().ok(),
                Some(("start", v)) if !v.is_empty() => holder.start = Some(v.to_string()),
                Some(("holder", v)) if !v.is_empty() => holder.what = Some(v.to_string()),
                _ => {}
            }
        }
        holder
    }

    /// Is the holder's process provably gone: no such process runs, or its id now names
    /// another process (a start time that differs)? A holder with no process recorded, or a
    /// process list that could not be read, is taken for alive (`loops::alive`).
    fn gone(&self) -> bool {
        let Some(pid) = self.pid else {
            return false;
        };
        if pid == std::process::id() && self.start == own_start() {
            return false;
        }
        !crate::autopilot::loops::alive(pid, self.start.as_deref())
    }
}

/// This process's start time, read once (`loops::process_token`). Windows gives none
/// (`tasklist` has no start time), so none is recorded there.
fn own_start() -> Option<String> {
    use std::sync::OnceLock;
    static START: OnceLock<Option<String>> = OnceLock::new();
    START
        .get_or_init(|| {
            if cfg!(windows) {
                return None;
            }
            crate::autopilot::loops::process_token(std::process::id())
                .filter(|t| t != crate::autopilot::loops::UNKNOWN_TOKEN)
        })
        .clone()
}

fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// The holder a lock file names now; `None` when there is no file or it cannot be read.
fn read_holder(path: &Path) -> Option<Holder> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| Holder::parse(&text))
}

/// How often a waiting taker asks whether the holder's process is still running (on Windows
/// and macOS that runs `tasklist` or `ps`).
const LIVENESS_EVERY: Duration = Duration::from_secs(1);

/// A held lock on one target file; dropping it removes the lock file, if it is still this
/// holder's.
#[derive(Debug)]
pub struct FileLock {
    path: PathBuf,
    token: String,
}

impl FileLock {
    /// Take the lock on `target`, waiting up to `wait` while another thread or process holds
    /// it. The target's folder must exist. Out of time, the error names the lock file and what
    /// holds it, and says to wait and try again.
    pub fn take(target: &Path, wait: Duration) -> Result<FileLock, CoreError> {
        Self::taken(target, wait, None)
    }

    /// `take`, with what this holder is doing written in the lock file, for the error another
    /// taker gets ("the autopilot passes").
    pub fn take_for(target: &Path, wait: Duration, what: &str) -> Result<FileLock, CoreError> {
        Self::taken(target, wait, Some(what))
    }

    fn taken(target: &Path, wait: Duration, what: Option<&str>) -> Result<FileLock, CoreError> {
        let path = lock_path(target);
        let started = Instant::now();
        let mut looked: Option<Instant> = None;
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let holder = Holder::new(what);
                    // Who holds it: the token its own refresh and drop look for, and the
                    // process a waiting taker asks after.
                    let _ = file.write_all(holder.text().as_bytes());
                    return Ok(FileLock {
                        path,
                        token: holder.token,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                // On Windows a lock file being removed can refuse access for a moment.
                Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied => {}
                Err(e) => return Err(CoreError::Io(format!("{}: {e}", target.display()))),
            }
            let look = looked.map_or(true, |at| at.elapsed() >= LIVENESS_EVERY);
            if look {
                looked = Some(Instant::now());
            }
            if is_stale(&path) || (look && holder_gone(&path)) {
                take_over(&path);
                continue;
            }
            if started.elapsed() >= wait {
                return Err(busy(target, &path, wait));
            }
            std::thread::sleep(POLL);
        }
    }

    /// The lock file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Mark the lock file fresh, so a holder that works longer than `STALE_LOCK` (#385: a
    /// merge pass, a satellite loop) is not taken for a run that died. Only while the file is
    /// still this holder's (#431): a holder frozen past the takeover time never touches the
    /// lock another run took over.
    pub fn refresh(&self) {
        touch_if_mine(&self.path, &self.token);
    }

    /// Say what this holder is doing now (the passes lock while the weekly backup runs), for
    /// the error another taker gets. Only while the file is still this holder's.
    pub fn note(&self, what: &str) {
        use std::io::{Read, Seek};
        let Ok(mut file) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
        else {
            return;
        };
        let mut text = String::new();
        if file.read_to_string(&mut text).is_err() {
            return;
        }
        let mut holder = Holder::parse(&text);
        if holder.token != self.token {
            return;
        }
        holder.what = Some(one_line(what));
        // The same file, through the handle that read it: never another holder's.
        let _ = file
            .set_len(0)
            .and_then(|()| file.rewind())
            .and_then(|()| file.write_all(holder.text().as_bytes()));
    }

    /// Keep this lock fresh every `every` from a thread of its own until the `FreshLock` drops
    /// (#385 review: a lock held across calls, as an MCP server's satellite between ticks, which
    /// no one call can `keep_fresh`). The thread ends before the lock file is removed.
    pub fn kept_fresh(self, every: Duration) -> FreshLock {
        let (stop, wait) = std::sync::mpsc::channel::<()>();
        let (path, token) = (self.path.clone(), self.token.clone());
        let refresher = std::thread::spawn(move || {
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = wait.recv_timeout(every) {
                touch_if_mine(&path, &token);
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

/// Mark the lock file at `path` fresh when it holds `token`. The check and the touch go
/// through one open handle, so the file touched is the one whose token was read: when another
/// run has taken the lock over, either its token is read (nothing is touched) or the handle is
/// on the old file, which is no longer the lock. True when it was touched.
fn touch_if_mine(path: &Path, token: &str) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    else {
        return false;
    };
    let mut text = String::new();
    if file.read_to_string(&mut text).is_err() || Holder::parse(&text).token != token {
        return false;
    }
    file.set_modified(std::time::SystemTime::now()).is_ok()
}

/// How often `FileLock::keep_fresh` marks the lock file fresh.
pub const REFRESH_EVERY: Duration = Duration::from_secs(60);

/// Is the lock on `target` held now: its lock file is there, not stale, and its holder is not
/// provably gone. A read; it takes nothing.
pub fn held(target: &Path) -> bool {
    let path = lock_path(target);
    path.exists() && !is_stale(&path) && !holder_gone(&path)
}

impl Drop for FileLock {
    /// Removes the lock file only while it is this holder's (#431). It is marked fresh first,
    /// so no taker can find it stale in the moment before it goes.
    fn drop(&mut self) {
        if touch_if_mine(&self.path, &self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
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

/// True when the lock file at `path` names a holder whose process is provably gone (#431).
fn holder_gone(path: &Path) -> bool {
    read_holder(path).is_some_and(|holder| holder.gone())
}

/// The error a taker gets when `wait` ran out (#431): what holds the lock, when the file says,
/// and to wait for it or check on it. Removing the lock file is offered only when its holder
/// is provably gone; a live holder's never is.
fn busy(target: &Path, path: &Path, wait: Duration) -> CoreError {
    let waited = wait.as_secs_f64().ceil();
    let holder = read_holder(path).unwrap_or_default();
    let what = holder.what.as_deref().unwrap_or("another run");
    let text = match holder.pid {
        Some(pid) if holder.gone() => format!(
            "{} is locked by {what}, process {pid}, which has ended (waited {waited} s for {}); \
             try again, or remove that lock file",
            target.display(),
            path.display()
        ),
        Some(pid) => format!(
            "{} is being changed by {what}, process {pid} (waited {waited} s for {}); wait for \
             it to finish and try again, or check that process {pid} is still working",
            target.display(),
            path.display()
        ),
        None => format!(
            "{} is being changed by another run (waited {waited} s for {}); wait for it to \
             finish and try again. A lock left by a run that died is taken over by itself after \
             {} minutes",
            target.display(),
            path.display(),
            STALE_LOCK.as_secs() / 60
        ),
    };
    CoreError::Io(text)
}

/// Remove a dead run's lock file. Only one taker at a time may: it must first make
/// `<lock>.takeover` with `create_new`, then looks again and removes the lock only if it is
/// still stale or its holder still provably gone. Without that, a taker that saw the dead
/// file could remove a fresh lock another taker had just made, and two would hold it. A
/// takeover file left by a run that died mid-takeover is itself stale after `STALE_LOCK` and
/// removed; every other taker just looks again.
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
    if is_stale(path) || holder_gone(path) {
        let _ = std::fs::remove_file(path);
    }
    let _ = std::fs::remove_file(&guard);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lock file read while its holder writes it may end mid-line; a cut-short process id
    /// would name another process, so only whole lines count.
    #[test]
    fn a_lock_file_read_mid_write_names_no_process() {
        let whole = "token=abc\npid=1234\nstart=99\n";
        assert_eq!(Holder::parse(whole).pid, Some(1234));
        assert_eq!(Holder::parse(whole).start.as_deref(), Some("99"));
        let cut = Holder::parse("token=abc\npid=12");
        assert_eq!((cut.token.as_str(), cut.pid), ("abc", None));
        assert_eq!(Holder::parse("token=abc\npid=1234\nstart=9").start, None);
        assert!(!Holder::parse("").gone());
        assert!(!Holder::parse("tok").gone());
    }
}
