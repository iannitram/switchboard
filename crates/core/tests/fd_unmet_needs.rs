//! Tests for FD-12 (`unmet` names the request needs this machine's loadout cannot meet, in the
//! order given, and `held_job` builds the fleet job another machine pulls for them). The loadout
//! is a fixture built in code; no probe runs and no fleet is called.

use switchboard_core::it::{held_job, unmet, Loadout};

const THIS: &str = "machine-a";

/// A loadout with `gh` signed in, `codex` present but signed out, one model and one toolchain.
fn fixture() -> Loadout {
    Loadout {
        read_at: 0,
        timed_out: Vec::new(),
        clis: vec![("gh".to_string(), true), ("codex".to_string(), false)],
        models_loaded: vec!["llama-small".to_string()],
        toolchains: vec!["rust".to_string()],
        busy: false,
        recent_errors: Vec::new(),
    }
}

fn needs(tags: &[&str]) -> Vec<String> {
    tags.iter().map(|t| t.to_string()).collect()
}

#[test]
fn a_signed_in_cli_meets_its_need() {
    assert!(unmet(&needs(&["gh"]), &fixture()).is_empty());
}

#[test]
fn a_present_but_signed_out_cli_is_unmet() {
    assert_eq!(unmet(&needs(&["codex"]), &fixture()), needs(&["codex"]));
}

#[test]
fn a_loaded_model_and_a_toolchain_meet_their_needs() {
    assert!(unmet(&needs(&["llama-small", "rust"]), &fixture()).is_empty());
}

#[test]
fn unmet_lists_only_the_missing_needs_in_the_order_given() {
    let loadout = Loadout {
        clis: vec![("web".to_string(), true)],
        ..Loadout::default()
    };
    assert_eq!(unmet(&needs(&["web", "cuda"]), &loadout), needs(&["cuda"]));
    assert_eq!(
        unmet(&needs(&["cuda", "web", "gh"]), &loadout),
        needs(&["cuda", "gh"])
    );
}

#[test]
fn no_needs_gives_empty_unmet() {
    assert!(unmet(&[], &fixture()).is_empty());
    assert!(unmet(&[], &Loadout::default()).is_empty());
}

#[test]
fn held_job_is_none_when_nothing_is_unmet() {
    assert!(held_job("review", "look at PR 12", &[], THIS).is_none());
}

#[test]
fn held_job_carries_kind_request_unmet_needs_and_this_machine() {
    let job = held_job("review", "look at PR 12", &needs(&["cuda"]), THIS)
        .expect("a job when a need is unmet");
    assert_eq!(job["kind"], "review");
    assert_eq!(job["request"], "look at PR 12");
    assert_eq!(job["needs"], serde_json::json!(["cuda"]));
    assert_eq!(job["never_on"], serde_json::json!([THIS]));
}
