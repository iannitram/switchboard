//! `autopilot`: the Project Manager, the Budget Manager and the satellite as code. The Budget
//! Manager is code; the Project Manager and the satellite run as routed jobs. On 1.0 this
//! machine is its own satellite.

pub mod classify;
pub mod driver;
pub mod instructions;
pub mod review;
pub mod weekly;

use crate::fleet::FleetBridge;
use crate::CoreError;

/// AUTO-1: ask `fleet budget allow <lane>` before anything spends a pool; `false` (exit 75)
/// means not now.
pub fn may_spend(fleet: &FleetBridge, lane: &str) -> Result<bool, CoreError> {
    todo!("AUTO-1")
}
