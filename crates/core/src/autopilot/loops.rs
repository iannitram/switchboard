//! #432: the satellite's loops and holds on disk, `<data>/satellite/loops.json`, so every
//! process on this machine (the window, `switchboard autopilot satellite`, the MCP server, a
//! dry run) sees the same loops and the same holds.
//!
//! A loop is recorded when it starts, with its process id and the process that watches it (the
//! owner). A run that ended while its loop ran leaves the record behind; the next satellite tick
//! adopts that loop and watches it to its end (`ChildRunner::adopt`), so no second copy starts.
//! A lane whose loop ended with an error is held until a time on the clock, here too.
//!
//! Every change is a read, change and write under a `FileLock` on the file, written to a temp
//! file and renamed over it (#431's rule for state two processes write). A read takes no lock:
//! a dry run and the status read only read.

use crate::lock::{self, FileLock};
use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The file in the satellite folder.
pub const LOOPS_FILE: &str = "loops.json";

/// How long a change waits for another process's change: each is a read and a write.
const WRITE_WAIT: Duration = Duration::from_secs(10);

/// What `loops.json` holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoopsFile {
    /// When it was last changed, in Unix seconds.
    pub at: i64,
    pub loops: Vec<LoopRecord>,
    pub holds: Vec<Hold>,
}

/// One loop started and not yet seen to end.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoopRecord {
    pub lane: String,
    /// The loop's process.
    pub pid: u32,
    /// `process_token` of the loop when it started: a process id used again later by another
    /// process is not taken for the loop.
    pub pid_token: Option<String>,
    /// The process that watches it: the one that started it, or the one that adopted it.
    pub owner_pid: u32,
    pub owner_token: Option<String>,
    /// The command line it runs, program first.
    pub command: Vec<String>,
    /// When it started, in Unix seconds.
    pub started: i64,
}

/// A lane not to start before `until_ms` (Unix milliseconds).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hold {
    pub lane: String,
    pub until_ms: i64,
}

impl LoopRecord {
    /// Is the loop's process still running?
    pub fn running(&self) -> bool {
        alive(self.pid, self.pid_token.as_deref())
    }

    /// Is the process that watches it still running?
    pub fn owned(&self) -> bool {
        alive(self.owner_pid, self.owner_token.as_deref())
    }
}

impl LoopsFile {
    /// The lanes whose loop is running now.
    pub fn running(&self) -> Vec<String> {
        let mut lanes: Vec<String> = Vec::new();
        for record in self.loops.iter().filter(|record| record.running()) {
            if !lanes.contains(&record.lane) {
                lanes.push(record.lane.clone());
            }
        }
        lanes
    }

    /// The holds whose time has not passed at `now_ms`, each with the milliseconds left.
    pub fn holds_at(&self, now_ms: i64) -> Vec<(String, i64)> {
        self.holds
            .iter()
            .filter(|hold| hold.until_ms > now_ms)
            .map(|hold| (hold.lane.clone(), hold.until_ms - now_ms))
            .collect()
    }

    /// Hold `lane` until `until_ms`, in place of any hold it had; holds that are over go.
    pub fn hold(&mut self, lane: &str, until_ms: i64, now_ms: i64) {
        self.holds
            .retain(|hold| hold.lane != lane && hold.until_ms > now_ms);
        self.holds.push(Hold {
            lane: lane.to_string(),
            until_ms,
        });
    }

    /// Forget the loop of `lane` in process `pid`. Says whether there was one.
    pub fn forget(&mut self, lane: &str, pid: u32) -> bool {
        let before = self.loops.len();
        self.loops
            .retain(|record| !(record.lane == lane && record.pid == pid));
        self.loops.len() != before
    }
}

/// `<dir>/loops.json`.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(LOOPS_FILE)
}

/// What `<dir>/loops.json` holds; a missing or unreadable file is an empty one. Takes no lock.
pub fn read(dir: &Path) -> LoopsFile {
    std::fs::read_to_string(path(dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Change `<dir>/loops.json` under its lock: read it, let `change` change it, and write it back
/// (temp file and rename) when it changed. The answer is `change`'s.
pub fn update<T>(dir: &Path, change: impl FnOnce(&mut LoopsFile) -> T) -> Result<T, CoreError> {
    let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", path(dir).display()));
    std::fs::create_dir_all(dir).map_err(io)?;
    let target = path(dir);
    let _held = FileLock::take(&target, WRITE_WAIT)?;
    let mut file = read(dir);
    let before = file.clone();
    let answer = change(&mut file);
    if file != before {
        file.at = now_ms() / 1000;
        let text = serde_json::to_string_pretty(&file)
            .map_err(|e| CoreError::Parse(format!("{}: {e}", target.display())))?;
        let temp = lock::temp_path(&target, ".tmp");
        std::fs::write(&temp, text).map_err(io)?;
        if let Err(e) = std::fs::rename(&temp, &target) {
            let _ = std::fs::remove_file(&temp);
            return Err(io(e));
        }
    }
    Ok(answer)
}

/// Now, in Unix milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Is process `pid` running, and (with a token) the same process that had `token`?
pub fn alive(pid: u32, token: Option<&str>) -> bool {
    match process_token(pid) {
        Some(now) => token.map_or(true, |want| want == now),
        None => false,
    }
}

/// A word that names process `pid` while it runs (its start time), or `None` when no such
/// process runs (gone, or ended and not yet reaped). No extra dependency: Linux reads `/proc`,
/// another Unix asks `ps`, Windows asks `tasklist`.
pub fn process_token(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    token_of(pid)
}

#[cfg(target_os = "linux")]
fn token_of(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `pid (name) state ppid ...`: the name can hold spaces and brackets, so split after the
    // last `)`. Field 3 is the state, field 22 the start time.
    let rest = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    if matches!(fields.first(), None | Some(&"Z") | Some(&"X") | Some(&"x")) {
        return None;
    }
    fields.get(19).map(|start| start.to_string())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn token_of(pid: u32) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-o", "stat=,lstart=", "-p", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || text.is_empty() || text.starts_with('Z') {
        return None;
    }
    Some(
        text.split_once(char::is_whitespace)
            .map_or(String::new(), |(_, start)| start.trim().to_string()),
    )
}

#[cfg(windows)]
fn token_of(pid: u32) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .creation_flags(CREATE_NO_WINDOW)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    // `tasklist` gives no start time: a process id used again is taken for the same process.
    String::from_utf8_lossy(&out.stdout)
        .contains(&format!("\"{pid}\""))
        .then(|| "running".to_string())
}

#[cfg(not(any(unix, windows)))]
fn token_of(_pid: u32) -> Option<String> {
    None
}
