//! `autopilot`: the Project Manager, the Budget Manager and the satellite as code. The Budget
//! Manager is code; the Project Manager and the satellite run as routed jobs. On 1.0 this
//! machine is its own satellite.

pub mod classify;
pub mod driver;
pub mod instructions;
pub mod loops;
pub mod passes;
pub mod review;
pub mod run;
pub mod weekly;

pub use passes::{after_poll, AfterPoll};

use crate::fleet::FleetBridge;
use crate::CoreError;

/// AUTO-1: ask `fleet budget allow <lane>` before anything spends a pool; `false` (exit 75)
/// means not now. An empty lane is refused without a call; any other exit is the bridge's
/// `FleetError`, never `true`.
pub fn may_spend(fleet: &FleetBridge, lane: &str) -> Result<bool, CoreError> {
    if lane.is_empty() {
        return Err(CoreError::Refused("budget: no lane to ask about".into()));
    }
    let code = fleet
        .run_status(&["budget", "allow", lane])
        .map_err(CoreError::Fleet)?;
    match code {
        0 => Ok(true),
        75 => Ok(false),
        other => Err(CoreError::Refused(format!(
            "budget allow {lane}: exit {other}"
        ))),
    }
}
