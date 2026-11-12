//! CORE-2, IT-2 (#375): the fleet errors the app records, kept on disk in the app data folder
//! so the CLI and MCP can list them and the app's own list outlives a restart.
//!
//! One JSON record per line in `<data>/fleet-errors.jsonl`, oldest first, holding the fields
//! `FleetError` holds. A record is appended whole with one write while `lock::FileLock` is held,
//! so two app processes (or two threads) never interleave lines. The file is kept to the last
//! `FLEET_ERROR_LOG_CAP` lines, as the session log is. A line that does not read (cut short,
//! not UTF-8, not a record) is skipped and counted, never a panic. The app's list and the CLI's
//! both read through `load`, so the two faces agree.

use crate::fleet::{FleetError, FleetErrorReason, FLEET_ERROR_LOG_CAP};
use crate::lock::FileLock;
use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The file's name in the app data folder.
pub const FLEET_ERRORS_FILE: &str = "fleet-errors.jsonl";

/// How long `record` waits for another writer before it gives up. A record is a few
/// milliseconds' work, so a wait this long means a writer is stuck, and the error recording
/// is not worth holding up the action that failed.
pub const RECORD_LOCK_WAIT: Duration = Duration::from_secs(2);

/// One fleet error as a line of the file and as the CLI's and MCP's `--json` print it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetErrorRecord {
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub stderr: String,
    pub reason: ReasonRecord,
    #[serde(default)]
    pub undo_note: Option<String>,
    /// The error as the face shows it (`FleetError`'s text); written for a reader, not read back.
    #[serde(default, skip_deserializing)]
    pub message: String,
}

/// `FleetErrorReason` as a record: `{"kind": "exit"}`, `{"kind": "bad_json", "why": ..}`,
/// `{"kind": "spawn", "why": ..}`, `{"kind": "timeout", "secs": ..}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReasonRecord {
    Exit,
    BadJson { why: String },
    Spawn { why: String },
    Timeout { secs: u64 },
}

impl From<&FleetError> for FleetErrorRecord {
    fn from(err: &FleetError) -> Self {
        Self {
            command: err.command.clone(),
            exit_code: err.exit_code,
            stderr: err.stderr.clone(),
            reason: match &err.reason {
                FleetErrorReason::Exit => ReasonRecord::Exit,
                FleetErrorReason::BadJson(why) => ReasonRecord::BadJson { why: why.clone() },
                FleetErrorReason::Spawn(why) => ReasonRecord::Spawn { why: why.clone() },
                FleetErrorReason::Timeout(secs) => ReasonRecord::Timeout { secs: *secs },
            },
            undo_note: err.undo_note.clone(),
            message: err.to_string(),
        }
    }
}

impl From<FleetErrorRecord> for FleetError {
    fn from(rec: FleetErrorRecord) -> Self {
        Self {
            command: rec.command,
            exit_code: rec.exit_code,
            stderr: rec.stderr,
            reason: match rec.reason {
                ReasonRecord::Exit => FleetErrorReason::Exit,
                ReasonRecord::BadJson { why } => FleetErrorReason::BadJson(why),
                ReasonRecord::Spawn { why } => FleetErrorReason::Spawn(why),
                ReasonRecord::Timeout { secs } => FleetErrorReason::Timeout(secs),
            },
            undo_note: rec.undo_note,
        }
    }
}

/// What `load` found: the errors, newest first, and how many lines did not read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub errors: Vec<FleetError>,
    pub skipped: usize,
}

/// `<data>/fleet-errors.jsonl`.
pub fn path(data: &Path) -> PathBuf {
    data.join(FLEET_ERRORS_FILE)
}

/// Append `err` to the file in `data` (made if missing), then keep only the last
/// `FLEET_ERROR_LOG_CAP` lines. An error is the reason it was not kept; the caller says so and
/// carries on, since a failed recording must not fail the action that hit the fleet error.
pub fn record(data: &Path, err: &FleetError) -> Result<(), CoreError> {
    let io = |what: &Path, e: std::io::Error| CoreError::Io(format!("{}: {e}", what.display()));
    std::fs::create_dir_all(data).map_err(|e| io(data, e))?;
    let file_path = path(data);
    let _lock = FileLock::take(&file_path, RECORD_LOCK_WAIT)?;
    let mut line = serde_json::to_string(&FleetErrorRecord::from(err))
        .map_err(|e| CoreError::Parse(format!("fleet error record: {e}")))?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(&file_path)
        .map_err(|e| io(&file_path, e))?;
    // A line cut short (a crash mid-write) gets its end here, so this record starts a line.
    if ends_mid_line(&mut file).map_err(|e| io(&file_path, e))? {
        line.insert(0, '\n');
    }
    file.write_all(line.as_bytes())
        .and_then(|()| file.sync_data())
        .map_err(|e| io(&file_path, e))?;
    drop(file);
    trim(&file_path)
}

/// True when the file is not empty and its last byte is not a newline.
fn ends_mid_line(file: &mut std::fs::File) -> std::io::Result<bool> {
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(len - 1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

/// Keep the last `FLEET_ERROR_LOG_CAP` lines of the file, written whole (`lock::write_whole`).
/// Called with the lock held.
fn trim(file_path: &Path) -> Result<(), CoreError> {
    let io = |what: &Path, e: std::io::Error| CoreError::Io(format!("{}: {e}", what.display()));
    let bytes = std::fs::read(file_path).map_err(|e| io(file_path, e))?;
    let lines: Vec<&[u8]> = lines(&bytes).collect();
    if lines.len() <= FLEET_ERROR_LOG_CAP {
        return Ok(());
    }
    let mut kept = Vec::with_capacity(bytes.len());
    for line in &lines[lines.len() - FLEET_ERROR_LOG_CAP..] {
        kept.extend_from_slice(line);
        kept.push(b'\n');
    }
    // `write_whole` takes no lock of its own, so it is safe under the caller's held lock.
    crate::lock::write_whole(file_path, &kept)
}

/// The file's non-blank lines, split on bytes so a line that is not UTF-8 is still one line.
fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes
        .split(|b| *b == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
}

/// The recorded errors in `data`, newest first, at most `FLEET_ERROR_LOG_CAP`, and the count of
/// lines that did not read. No file is no errors. Only a file that exists and cannot be read is
/// an error.
pub fn load(data: &Path) -> Result<Recorded, CoreError> {
    let file_path = path(data);
    let bytes = match std::fs::read(&file_path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Recorded::default()),
        Err(e) => return Err(CoreError::Io(format!("{}: {e}", file_path.display()))),
    };
    let mut recorded = Recorded::default();
    for line in lines(&bytes) {
        match serde_json::from_slice::<FleetErrorRecord>(line) {
            Ok(rec) => recorded.errors.push(rec.into()),
            Err(_) => recorded.skipped += 1,
        }
    }
    recorded.errors.reverse();
    recorded.errors.truncate(FLEET_ERROR_LOG_CAP);
    Ok(recorded)
}
