//! CORE-1, CORE-2: the fleet bridge. The core wraps the pipeline repo's Python `fleet` tools
//! rather than porting them: it runs `<python> <root>/tools/fleet/fleet.py <cmd> ... --json`
//! and parses the result. Only commands with `--json`; one that lacks it gets `--json` added
//! in a pipeline repo change, never text parsing.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// The environment variable that points the bridge at another root (tests: a stub `fleet.py`
/// under `tests/fixtures/fleet-stub/`).
pub const FLEET_ROOT_ENV: &str = "FLEET_ROOT";

/// CORE-2: a fleet command that failed or printed JSON the core can't parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetError {
    /// The full command line, program first.
    pub command: Vec<String>,
    /// The exit code; `None` when the process could not start or was killed.
    pub exit_code: Option<i32>,
    /// Its stderr (all of it; the face shows the first lines).
    pub stderr: String,
    pub reason: FleetErrorReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FleetErrorReason {
    /// The process exited non-zero.
    Exit,
    /// The process exited 0 but stdout was not JSON. Holds the parser's message.
    BadJson(String),
    /// The process could not be started. Holds the OS error text.
    Spawn(String),
}

pub struct FleetBridge {
    root: PathBuf,
}

impl FleetBridge {
    /// A bridge over the pipeline repo at `pipeline_root`, unless `FLEET_ROOT` is set, which
    /// wins.
    pub fn new(pipeline_root: &Path) -> Self {
        todo!("CORE-1")
    }

    /// `<root>/tools/fleet/fleet.py`.
    pub fn script(&self) -> PathBuf {
        self.root.join("tools").join("fleet").join("fleet.py")
    }

    /// Run `fleet.py <args...> --json` with the platform's Python launcher and parse stdout.
    pub fn run_json(&self, args: &[&str]) -> Result<Value, FleetError> {
        todo!("CORE-1, CORE-2")
    }

    /// Run a fleet command whose answer is its exit code (`budget allow <lane>`: 0 go,
    /// 75 not now). Any other code is a `FleetError`.
    pub fn run_status(&self, args: &[&str]) -> Result<i32, FleetError> {
        todo!("AUTO-1")
    }
}

/// CORE-2: every fleet error the core has recorded this session, newest last, for the view
/// and the IT region (IT-2).
#[derive(Debug, Default)]
pub struct FleetErrorLog {
    pub errors: Vec<FleetError>,
}

impl FleetErrorLog {
    pub fn record(&mut self, err: FleetError) {
        todo!("CORE-2")
    }
}
