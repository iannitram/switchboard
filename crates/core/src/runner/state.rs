//! The run state of one unit in one workflow: where it is, how often each step failed, and what
//! the last call did. It is saved after every call, so a restart resumes at the same step.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::lock::write_whole;
use crate::CoreError;

/// What `on_fail` decided once a step's retries were used up. The run stops here until a person
/// or the named step takes it on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "via", rename_all = "snake_case")]
pub enum Halt {
    /// Hand the unit to another step; holds the spec's `escalate_via` text.
    Escalate(String),
    Park,
    AskOwner,
    Fail,
}

/// What the last call did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Last {
    /// Moved to `to_state_ok`.
    Done,
    /// Exit 75: nothing changed.
    NotNow,
    /// A failure with retries left; the next call runs the same step again.
    Retry,
    /// A failure that halted the run.
    Halted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunState {
    pub spec: String,
    pub unit: String,
    pub state: String,
    /// Failures so far per step id; a step that succeeds starts again from zero.
    pub failures: BTreeMap<String, u64>,
    /// The step the next call must run again after a failure that may be retried.
    pub retry_step: Option<String>,
    pub halt: Option<Halt>,
    pub last_step: Option<String>,
    pub last: Option<Last>,
    /// Why the last call ended as it did ("not now", the failed check, the exit code).
    pub note: String,
}

impl RunState {
    pub fn new(spec: &str, unit: &str, state: &str) -> Self {
        Self {
            spec: spec.to_string(),
            unit: unit.to_string(),
            state: state.to_string(),
            failures: BTreeMap::new(),
            retry_step: None,
            halt: None,
            last_step: None,
            last: None,
            note: String::new(),
        }
    }
}

/// Run states kept as one JSON file per spec and unit under one folder.
#[derive(Debug, Clone)]
pub struct StateStore {
    dir: PathBuf,
}

impl StateStore {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    fn file(&self, spec: &str, unit: &str) -> PathBuf {
        let safe = |text: &str| -> String {
            text.chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect()
        };
        self.dir
            .join(format!("{}--{}.json", safe(spec), safe(unit)))
    }

    /// The saved state, or `None` when this unit has not started. A file that does not parse is an
    /// error: a run is never restarted from a guess.
    pub fn load(&self, spec: &str, unit: &str) -> Result<Option<RunState>, CoreError> {
        let path = self.file(spec, unit);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))
    }

    pub fn save(&self, state: &RunState) -> Result<(), CoreError> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| CoreError::Io(format!("{}: {e}", self.dir.display())))?;
        let text = serde_json::to_string_pretty(state)
            .map_err(|e| CoreError::Parse(format!("run state: {e}")))?;
        write_whole(&self.file(&state.spec, &state.unit), text.as_bytes())
    }
}
