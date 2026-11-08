//! Tests for IT-3 (an `it-triage` job for an open IT ticket carries `never_on` naming this
//! machine, and is not offered while the pipeline lacks `never_on`). The pipeline's answer is a
//! plain bool, so these tests need no fleet. `take_next` models `fleet run --next` for the one
//! rule under test (`never_on` holds at all hours; `not_on` holds only in quiet hours); the real
//! `fleet` is not called.

use serde_json::Value;
use switchboard_core::it::triage_job;

const THIS: &str = "machine-a";
const OTHER: &str = "machine-b";

/// Whether `machine` takes `job` from `fleet run --next` (a model of the one rule under test).
fn take_next(job: &Value, machine: &str, quiet_hours: bool) -> bool {
    let listed = |key: &str| {
        job.get(key)
            .and_then(Value::as_array)
            .is_some_and(|names| names.iter().any(|n| n.as_str() == Some(machine)))
    };
    if listed("never_on") {
        return false;
    }
    if quiet_hours && listed("not_on") {
        return false;
    }
    true
}

#[test]
fn the_job_carries_never_on_naming_this_machine() {
    let job = triage_job(42, THIS, true).expect("a job once the pipeline has never_on");
    assert_eq!(job["kind"], "it-triage");
    assert_eq!(job["ticket"], 42);
    assert_eq!(job["never_on"], serde_json::json!([THIS]));
}

#[test]
fn the_job_never_uses_not_on() {
    let job = triage_job(42, THIS, true).expect("a job once the pipeline has never_on");
    assert!(job.get("not_on").is_none());
}

#[test]
fn no_job_is_offered_while_the_pipeline_lacks_never_on() {
    assert!(triage_job(42, THIS, false).is_none());
}

#[test]
fn this_machine_does_not_take_its_own_triage_job_outside_quiet_hours() {
    let job = triage_job(42, THIS, true).expect("a job once the pipeline has never_on");
    assert!(!take_next(&job, THIS, false));
}

#[test]
fn another_machine_takes_the_triage_job() {
    let job = triage_job(42, THIS, true).expect("a job once the pipeline has never_on");
    assert!(take_next(&job, OTHER, false));
}
