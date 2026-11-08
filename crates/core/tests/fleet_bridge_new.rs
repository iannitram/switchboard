//! R25 review probe for issue 1: `FleetBridge::new` reads `FLEET_ROOT` from the environment.
//! Its own test file (its own process) and a single test, so setting the variable cannot race
//! the locked tests in `fleet_bridge.rs`.

use std::path::Path;
use switchboard_core::fleet::{FleetBridge, FLEET_ROOT_ENV};

#[test]
fn new_takes_fleet_root_from_the_environment_when_non_empty() {
    let p = Path::new("/srv/pipeline");
    std::env::remove_var(FLEET_ROOT_ENV);
    assert_eq!(FleetBridge::new(p).root(), p);
    std::env::set_var(FLEET_ROOT_ENV, "");
    assert_eq!(FleetBridge::new(p).root(), p);
    std::env::set_var(FLEET_ROOT_ENV, "/srv/other");
    assert_eq!(FleetBridge::new(p).root(), Path::new("/srv/other"));
    std::env::remove_var(FLEET_ROOT_ENV);
}
