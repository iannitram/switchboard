//! The skeleton's own checks: the workspace builds, the fixtures are where the issues expect
//! them, and the pieces that are already real (the clock, the fleet script path) work. Each
//! issue adds its own test file beside this one.

use std::path::PathBuf;
use switchboard_core::clock::{Clock, FixedClock};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn fixed_clock_moves_only_when_told() {
    let c = FixedClock::at(1_000);
    assert_eq!(c.now(), 1_000);
    c.advance(600);
    assert_eq!(c.now(), 1_600);
}

#[test]
fn fleet_stub_and_its_answers_are_in_place() {
    let stub = fixtures().join("fleet-stub");
    assert!(stub.join("tools/fleet/fleet.py").is_file());
    for answer in ["budget.json", "actions.json"] {
        let text = std::fs::read_to_string(stub.join("answers").join(answer)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(v.is_object(), "{answer} is a JSON object");
    }
}

#[test]
fn ticket_title_names_the_signature() {
    assert_eq!(
        switchboard_core::it::ticket_title("disk-full"),
        "IT: disk-full"
    );
}
