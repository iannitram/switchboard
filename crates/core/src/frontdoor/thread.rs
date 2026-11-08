//! FD-13: a chat is a thread with its handoff id, stored in local files: one JSON Lines file per
//! thread, `<data>/threads/<handoff_id>.jsonl`, each row in the job-log row shape (CORE-4), the
//! format fleet-state will carry later (`chat-sync`, After 1.0).

use crate::joblog::JobRow;
use crate::CoreError;
use std::path::{Path, PathBuf};

pub struct ThreadStore {
    dir: PathBuf,
}

impl ThreadStore {
    pub fn new(data_dir: &Path) -> Self {
        ThreadStore {
            dir: data_dir.join("threads"),
        }
    }

    /// Append one row to the thread's file (created on first write).
    pub fn append(&self, handoff_id: &str, row: &JobRow) -> Result<(), CoreError> {
        todo!("FD-13")
    }

    /// Every row of a thread, in file order; a thread with no file is empty.
    pub fn read(&self, handoff_id: &str) -> Result<Vec<JobRow>, CoreError> {
        todo!("FD-13")
    }

    /// Every thread's handoff id, sorted.
    pub fn list(&self) -> Result<Vec<String>, CoreError> {
        todo!("FD-13")
    }
}
