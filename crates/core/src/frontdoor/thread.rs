//! FD-13: a chat is a thread with its handoff id, stored in local files: one JSON Lines file per
//! thread, `<data>/threads/<handoff_id>.jsonl`, each row in the job-log row shape (CORE-4), the
//! format fleet-state will carry later (`chat-sync`, After 1.0).

use crate::joblog::JobRow;
use crate::lock::FileLock;
use crate::CoreError;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// How long a write waits for another writer of the same thread file (#419). A write under the
/// lock is one small file read and written, so a longer wait means a run died holding it.
pub const THREAD_LOCK_WAIT: Duration = Duration::from_secs(10);

/// Whether a thread row is a chat turn: its role is `user` or `assistant` (not a rating, an
/// implicit signal, a summary or a move).
pub fn is_turn(row: &JobRow) -> bool {
    matches!(row.role.as_deref(), Some("user") | Some("assistant"))
}

pub struct ThreadStore {
    dir: PathBuf,
}

/// A handoff id names one file under `threads/`, so it may not hold a path separator or `..`.
fn check_id(handoff_id: &str) -> Result<(), CoreError> {
    if handoff_id.is_empty()
        || handoff_id.contains('/')
        || handoff_id.contains('\\')
        || handoff_id.contains("..")
    {
        return Err(CoreError::Refused(format!(
            "handoff id is not a file name: {handoff_id:?}"
        )));
    }
    Ok(())
}

impl ThreadStore {
    pub fn new(data_dir: &Path) -> Self {
        ThreadStore {
            dir: data_dir.join("threads"),
        }
    }

    fn file(&self, handoff_id: &str) -> PathBuf {
        self.dir.join(format!("{handoff_id}.jsonl"))
    }

    /// Take the thread file's lock (`lock::FileLock`, #419): every write to a thread holds it,
    /// so a turn stored by one process and a rating rewritten by another (the window and the
    /// CLI or MCP) cannot lose each other's rows. The threads folder is made first: the lock
    /// file sits beside the thread file.
    fn locked(&self, handoff_id: &str) -> Result<FileLock, CoreError> {
        check_id(handoff_id)?;
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| CoreError::Io(format!("{}: {e}", self.dir.display())))?;
        FileLock::take(&self.file(handoff_id), THREAD_LOCK_WAIT)
    }

    /// Append `rows` to the thread's file (created on first write). The caller holds the lock.
    fn append_unlocked(&self, handoff_id: &str, rows: &[JobRow]) -> Result<(), CoreError> {
        let path = self.file(handoff_id);
        let mut text = String::new();
        for row in rows {
            text.push_str(
                &serde_json::to_string(row)
                    .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))?,
            );
            text.push('\n');
        }
        let io_err = |e: std::io::Error| CoreError::Io(format!("{}: {e}", path.display()));
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(io_err)?;
        file.write_all(text.as_bytes()).map_err(io_err)
    }

    /// Write `rows` as the whole thread file, through a temp file of its own beside it and a
    /// rename, so a reader never sees half a file. The caller holds the lock.
    fn rewrite_unlocked(&self, handoff_id: &str, rows: &[JobRow]) -> Result<(), CoreError> {
        let path = self.file(handoff_id);
        let mut text = String::new();
        for row in rows {
            text.push_str(
                &serde_json::to_string(row)
                    .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))?,
            );
            text.push('\n');
        }
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let tmp = path.with_extension(format!(
            "{}-{}.tmp",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let io_err = |e: std::io::Error| CoreError::Io(format!("{}: {e}", path.display()));
        std::fs::write(&tmp, text).map_err(io_err)?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            io_err(e)
        })
    }

    /// Append one row to the thread's file (created on first write), under the thread's lock.
    pub fn append(&self, handoff_id: &str, row: &JobRow) -> Result<(), CoreError> {
        self.append_all(handoff_id, std::slice::from_ref(row))
    }

    /// Append `rows` together, under one hold of the thread's lock: no other writer's row
    /// lands between them (a question and its answer stay next to each other, #419).
    pub fn append_all(&self, handoff_id: &str, rows: &[JobRow]) -> Result<(), CoreError> {
        let _lock = self.locked(handoff_id)?;
        self.append_unlocked(handoff_id, rows)
    }

    /// Append `row` when `wanted` says so of the thread's rows as they are now, under one hold of
    /// the thread's lock, so two writers cannot both append it. True when it was appended.
    pub fn append_if(
        &self,
        handoff_id: &str,
        row: &JobRow,
        wanted: impl FnOnce(&[JobRow]) -> bool,
    ) -> Result<bool, CoreError> {
        let _lock = self.locked(handoff_id)?;
        if !wanted(&self.read(handoff_id)?) {
            return Ok(false);
        }
        self.append_unlocked(handoff_id, std::slice::from_ref(row))?;
        Ok(true)
    }

    /// Put `row` in the thread in place of the row with the same id, or append it when no row
    /// has that id (or it has none). The read, the change and the write happen under the
    /// thread's lock (#419), so a row another writer appends meanwhile is not lost; the file is
    /// rewritten through a temp file and a rename (#372: a second rating of one turn replaces
    /// the first).
    pub fn upsert(&self, handoff_id: &str, row: &JobRow) -> Result<(), CoreError> {
        let _lock = self.locked(handoff_id)?;
        let mut rows = self.read(handoff_id)?;
        let at = row
            .id
            .as_ref()
            .and_then(|id| rows.iter().position(|old| old.id.as_ref() == Some(id)));
        let Some(at) = at else {
            return self.append_unlocked(handoff_id, std::slice::from_ref(row));
        };
        rows[at] = row.clone();
        self.rewrite_unlocked(handoff_id, &rows)
    }

    /// Change the row whose id is `row_id` to what `change` makes of it, under the thread's
    /// lock, so the row changed is the one in the file now, not one read earlier (#419). True
    /// when the thread has such a row; false, and nothing written, when it has none.
    pub fn change(
        &self,
        handoff_id: &str,
        row_id: &str,
        change: impl FnOnce(JobRow) -> JobRow,
    ) -> Result<bool, CoreError> {
        let _lock = self.locked(handoff_id)?;
        let mut rows = self.read(handoff_id)?;
        let Some(at) = rows
            .iter()
            .position(|old| old.id.as_deref() == Some(row_id))
        else {
            return Ok(false);
        };
        rows[at] = change(rows[at].clone());
        self.rewrite_unlocked(handoff_id, &rows)?;
        Ok(true)
    }

    /// Whether the thread has a file with at least one row.
    pub fn exists(&self, handoff_id: &str) -> Result<bool, CoreError> {
        Ok(!self.read(handoff_id)?.is_empty())
    }

    /// The thread's chat turns, oldest first: its rows whose role is `user` or `assistant`. A
    /// turn's index here is the turn index a rating names (FD-9).
    pub fn turns(&self, handoff_id: &str) -> Result<Vec<JobRow>, CoreError> {
        Ok(self.read(handoff_id)?.into_iter().filter(is_turn).collect())
    }

    /// Every row of a thread, in file order; a thread with no file is empty.
    pub fn read(&self, handoff_id: &str) -> Result<Vec<JobRow>, CoreError> {
        check_id(handoff_id)?;
        let path = self.file(handoff_id);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
        };
        text.lines()
            .enumerate()
            .map(|(i, line)| {
                serde_json::from_str::<JobRow>(line).map_err(|e| {
                    CoreError::Parse(format!("{}: line {}: {e}", path.display(), i + 1))
                })
            })
            .collect()
    }

    /// Every thread's handoff id, sorted.
    pub fn list(&self) -> Result<Vec<String>, CoreError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(CoreError::Io(format!("{}: {e}", self.dir.display()))),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| CoreError::Io(format!("{}: {e}", self.dir.display())))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Some(id) = name.strip_suffix(".jsonl") {
                if !id.is_empty() && entry.path().is_file() {
                    ids.push(id.to_string());
                }
            }
        }
        ids.sort();
        Ok(ids)
    }
}
