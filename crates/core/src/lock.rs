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
//! #431: the lock file names its holder: a token no other holder has, the holder's process (id,
//! start time, and the machine, boot and pid namespace the id belongs to) and what it is doing. A holder refreshes and removes the file only while it
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
    /// Where `pid` means something (`own_place`): a process id read in another machine, boot
    /// or pid namespace (a container that shares the folder but not the process list) names
    /// some other process or none, so it says nothing about the holder.
    place: Option<String>,
    pid: Option<u32>,
    start: Option<String>,
    what: Option<String>,
}

impl Holder {
    /// A new holder in this process.
    fn new(what: Option<&str>) -> Holder {
        Holder {
            token: run_tag(),
            place: own_place(),
            pid: Some(std::process::id()),
            start: own_start(),
            what: what.map(one_line),
        }
    }

    fn text(&self) -> String {
        let mut text = format!("token={}\n", self.token);
        // Before the process id: a file cut short after `pid=` still has its place.
        if let Some(place) = &self.place {
            text.push_str(&format!("place={place}\n"));
        }
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
                Some(("place", v)) if !v.is_empty() => holder.place = Some(v.to_string()),
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
    /// process list that could not be read, is taken for alive (`loops::alive`). So is one
    /// recorded in another place (`own_place`: another machine sharing the folder, a boot
    /// before this one, another pid namespace), or with no place: its process id cannot be
    /// looked up here, and it is held until stale.
    fn gone(&self) -> bool {
        let Some(pid) = self.pid else {
            return false;
        };
        if self.place.is_none() || self.place != own_place() {
            return false;
        }
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

/// Where this process's id means something, read once: on Linux the boot and the pid
/// namespace (`/proc/sys/kernel/random/boot_id`, `/proc/self/ns/pid`), so a lock written in a
/// container with a process list of its own, or before a reboot, is never judged by this
/// process list; elsewhere the machine's name. `None` when it cannot be read: no lock is then
/// taken over before it is stale.
fn own_place() -> Option<String> {
    use std::sync::OnceLock;
    static PLACE: OnceLock<Option<String>> = OnceLock::new();
    PLACE.get_or_init(read_place).clone()
}

#[cfg(target_os = "linux")]
fn read_place() -> Option<String> {
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let ns = std::fs::read_link("/proc/self/ns/pid").ok()?;
    Some(one_line(&format!("{}/{}", boot.trim(), ns.display())))
}

#[cfg(windows)]
fn read_place() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .map(|name| one_line(name.trim()))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn read_place() -> Option<String> {
    let out = std::process::Command::new("uname")
        .arg("-n")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!name.is_empty()).then(|| one_line(&name))
}

#[cfg(not(any(unix, windows)))]
fn read_place() -> Option<String> {
    None
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
        let mut last_try = false;
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
                // A holder that ended since the last look (it let the lock go and exited while
                // the process list was read) is not a reason to give up: take it over and try
                // once more before saying the lock is held.
                if !last_try && holder_gone(&path) {
                    last_try = true;
                    take_over(&path);
                    continue;
                }
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

/// How many times the error path re-reads a lock file whose holder text is still empty or
/// partial (#443), `POLL` apart.
const SETTLE_READS: u32 = 5;

/// True when a lock file's text is not yet a whole holder: empty, or a new-format file cut
/// short before its process id (the holder writes it in one go, so a read can land mid-write).
/// A file from before #431 is one token line and is whole.
fn holder_is_partial(text: &str) -> bool {
    let holder = Holder::parse(text);
    holder.token.is_empty() || (text.contains("token=") && holder.pid.is_none())
}

/// The text of a lock file once its holder has written it: re-read up to `SETTLE_READS` times,
/// `POLL` apart, while it is partial (#443). `None` when the file is gone.
fn settled_text(path: &Path) -> Option<String> {
    settle(|| std::fs::read_to_string(path).ok())
}

/// `settled_text` over any reader, so the re-reads can be tested without timing.
fn settle(mut read: impl FnMut() -> Option<String>) -> Option<String> {
    let mut text = read()?;
    for _ in 0..SETTLE_READS {
        if !holder_is_partial(&text) {
            break;
        }
        std::thread::sleep(POLL);
        text = read()?;
    }
    Some(text)
}

/// The error a taker gets when `wait` ran out (#431): what holds the lock, when the file says,
/// and to wait for it or check on it. Removing the lock file is offered only when its holder
/// is provably gone; a live holder's never is.
fn busy(target: &Path, path: &Path, wait: Duration) -> CoreError {
    let waited = wait.as_secs_f64().ceil();
    let settled = settled_text(path);
    // A holder that has created the lock file and not yet written its text (#443): it is
    // starting, and naming it as "another run" would be wrong.
    if settled.as_deref().is_some_and(holder_is_partial) {
        return CoreError::Io(format!(
            "{} is locked by a run that is still starting (waited {waited} s for {}); wait a \
             moment and try again",
            target.display(),
            path.display()
        ));
    }
    let holder = settled.as_deref().map(Holder::parse).unwrap_or_default();
    let what = holder.what.as_deref().unwrap_or("another run");
    let text = match holder.pid {
        // Gone, and the file still names it: a holder that let the lock go and exited while
        // its process was looked up is not reported as one that died holding it.
        Some(pid)
            if holder.gone() && read_holder(path).is_some_and(|now| now.token == holder.token) =>
        {
            format!(
            "{} is locked by {what}, process {pid}, which has ended (waited {waited} s for {}); \
             try again, or remove that lock file",
            target.display(),
            path.display()
            )
        }
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

    /// A holder that ends between two looks of a waiting taker (it let the lock go and exited,
    /// or died) just before the wait runs out: the taker takes the lock, it does not report a
    /// holder that has ended.
    #[test]
    fn a_holder_that_ends_just_before_the_wait_runs_out_is_taken_over() {
        let Some(here) = own_place() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("sb-431-last-{}", run_tag()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("state.json");
        let path = lock_path(&target);
        // Held by a live process (this one, another token) when the taker first looks.
        let live = Holder::new(Some("a live run"));
        std::fs::write(&path, live.text()).unwrap();
        let dead = Holder {
            token: "dead".to_string(),
            place: Some(here),
            pid: Some(0x7fff_fff0),
            start: None,
            what: None,
        };
        let swap = {
            let path = path.clone();
            std::thread::spawn(move || {
                // After the look at 1 s, before the wait (1.5 s) runs out; the next look would
                // be at 2 s.
                std::thread::sleep(Duration::from_millis(1200));
                std::fs::write(&path, dead.text()).unwrap();
            })
        };
        let taken = FileLock::take(&target, Duration::from_millis(1500));
        swap.join().unwrap();
        assert!(taken.is_ok(), "{:?}", taken.err());
        drop(taken);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #443: a lock file whose holder has not written its text yet (empty, or cut short before
    /// its process id) is re-read, not reported as "another run"; text that arrives during the
    /// re-read is the one used.
    #[test]
    fn an_empty_holder_is_settled_not_called_another_run() {
        assert!(holder_is_partial(""));
        assert!(holder_is_partial("token=abc\n"));
        assert!(holder_is_partial("token=abc\npid="));
        assert!(!holder_is_partial("token=abc\npid=1234\n"));
        assert!(!holder_is_partial("abc"), "a pre-#431 holder is whole");

        let mut reads = vec!["", "", "token=abc\npid=1234\nholder=a run\n"].into_iter();
        let settled = settle(|| reads.next().map(str::to_string));
        assert_eq!(
            settled.as_deref(),
            Some("token=abc\npid=1234\nholder=a run\n")
        );

        let mut calls = 0;
        let never = settle(|| {
            calls += 1;
            Some(String::new())
        });
        assert_eq!(never.as_deref(), Some(""));
        assert_eq!(calls, 1 + SETTLE_READS);

        assert_eq!(settle(|| None), None);
    }

    /// #443: a taker that times out on a lock file whose holder has not written its text says
    /// the holder is still starting, not that another run holds the lock for ten minutes.
    #[test]
    fn a_busy_lock_with_an_empty_holder_says_it_is_starting() {
        let dir = std::env::temp_dir().join(format!("sb-443-empty-{}", run_tag()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("state.json");
        std::fs::write(lock_path(&target), "").unwrap();
        let err = FileLock::take(&target, Duration::from_millis(20))
            .expect_err("a held lock is not taken");
        let text = err.to_string();
        assert!(text.contains("still starting"), "{text}");
        assert!(!text.contains("another run"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A process id is looked up only where it was written: a holder in a container with a
    /// process list of its own, on another machine sharing the folder, or before a reboot is
    /// never found gone because this process list lacks its id; it is held until stale.
    #[test]
    fn a_holder_from_another_place_is_never_found_gone() {
        let Some(here) = own_place() else {
            return;
        };
        // No process has this id (above the kernel's pid limit on every system).
        let dead = |place: Option<&str>| Holder {
            token: "t".to_string(),
            place: place.map(str::to_string),
            pid: Some(0x7fff_fff0),
            start: None,
            what: None,
        };
        assert!(dead(Some(&here)).gone(), "a dead holder here is gone");
        assert!(!dead(Some("another-boot/pid:[1]")).gone());
        assert!(!dead(None).gone());
        let text = dead(Some(&here)).text();
        assert_eq!(Holder::parse(&text), dead(Some(&here)));
        // Cut short after `pid=`: its place is there before it.
        let cut = &text[..text.find("pid=").unwrap()];
        assert!(!Holder::parse(cut).gone());
    }
}
