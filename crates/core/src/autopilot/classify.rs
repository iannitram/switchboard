//! AUTO-9: a job's failure is classed by the done card's `outcome` and `failure_mode`, never by
//! the exit code: `fleet errors classify --json` decides, the core reads its answer.

use crate::fleet::FleetBridge;
use crate::CoreError;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureClass {
    Passed,
    /// The fleet's (rate limit, lock timeout, push failed...): an IT ticket (IT-2).
    Fleet {
        signature: String,
    },
    /// The code's: the ladder climbs; out of ladder, a bug (IT-4).
    Coding,
    /// The brief's or the job's: nothing filed, the Project Manager fixes it.
    Author,
}

/// Read one answer of `fleet errors classify --json` for a card.
pub fn from_json(value: &Value) -> Result<FailureClass, CoreError> {
    todo!("AUTO-9")
}

pub fn classify(fleet: &FleetBridge, card: &Path) -> Result<FailureClass, CoreError> {
    todo!("AUTO-9")
}
