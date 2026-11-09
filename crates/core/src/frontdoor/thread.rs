//! FD-13: a chat is a thread with its handoff id, stored in local files: one JSON Lines file per
//! thread, `<data>/threads/<handoff_id>.jsonl`, each row in the job-log row shape (CORE-4), the
//! format fleet-state will carry later (`chat-sync`, After 1.0).

use crate::joblog::JobRow;
use crate::CoreError;
use std::io::Write;
use std::path::{Path, PathBuf};

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

    /// Append one row to the thread's file (created on first write).
    pub fn append(&self, handoff_id: &str, row: &JobRow) -> Result<(), CoreError> {
        check_id(handoff_id)?;
        let path = self.file(handoff_id);
        let mut line = serde_json::to_string(row)
            .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))?;
        line.push('\n');
        let io_err = |e: std::io::Error| CoreError::Io(format!("{}: {e}", path.display()));
        std::fs::create_dir_all(&self.dir).map_err(io_err)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(io_err)?;
        file.write_all(line.as_bytes()).map_err(io_err)
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
