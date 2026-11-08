//! Locked tests for issue 2 (the abilities index: request kinds, needs, first lists). Qwen never
//! edits this file. The fixture is the stub's `answers/actions.json`.

use serde_json::{json, Value};
use std::path::PathBuf;
use switchboard_core::abilities::{AbilitiesIndex, Ability};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::CoreError;

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fleet-stub")
}

fn actions() -> Value {
    let text = std::fs::read_to_string(stub().join("answers").join("actions.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn index() -> AbilitiesIndex {
    AbilitiesIndex::from_json(&actions()).unwrap()
}

fn ids(list: &[&Ability]) -> Vec<String> {
    list.iter().map(|a| a.id.clone()).collect()
}

#[test]
fn every_entry_is_read_sorted_by_id() {
    let idx = index();
    let got: Vec<&str> = idx.entries.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
        got,
        vec![
            "general-question",
            "judgement-planning",
            "quick-lookup",
            "small-code-change-exact-brief",
            "web-research",
            "writing-editing",
        ]
    );
}

#[test]
fn a_request_kind_carries_its_label_needs_first_and_writes() {
    let idx = index();
    assert_eq!(
        idx.get("quick-lookup"),
        Some(&Ability {
            id: "quick-lookup".into(),
            label: "Quick lookup (a fact, a definition, a one-line answer)".into(),
            needs: vec![],
            request: true,
            first: vec!["qwen".into(), "gemini".into()],
            writes: false,
        })
    );
    assert_eq!(idx.get("web-research").unwrap().needs, vec!["web"]);
}

#[test]
fn a_job_action_has_request_false_and_an_empty_first_list() {
    let a = index()
        .get("small-code-change-exact-brief")
        .cloned()
        .unwrap();
    assert_eq!(a.label, "Small code change from an exact brief");
    assert_eq!(a.needs, vec!["code", "repo-write", "tools"]);
    assert!(!a.request);
    assert!(a.first.is_empty());
    assert!(a.writes);
}

#[test]
fn request_kinds_lists_only_request_entries_in_id_order() {
    let idx = index();
    assert_eq!(
        ids(&idx.request_kinds()),
        vec![
            "general-question",
            "judgement-planning",
            "quick-lookup",
            "web-research",
            "writing-editing",
        ]
    );
}

#[test]
fn get_finds_an_entry_by_id_and_none_otherwise() {
    let idx = index();
    assert_eq!(
        idx.get("web-research").unwrap().label,
        "Web research (search, sources)"
    );
    assert_eq!(idx.get("no-such-kind"), None);
}

#[test]
fn kinds_needing_lists_request_kinds_whose_needs_hold_the_tag() {
    let idx = index();
    assert_eq!(ids(&idx.kinds_needing("web")), vec!["web-research"]);
    assert_eq!(
        ids(&idx.kinds_needing("judgement")),
        vec!["judgement-planning"]
    );
    // `code` is only on a job action, never a request kind.
    assert!(idx.kinds_needing("code").is_empty());
    assert!(idx.kinds_needing("").is_empty());
}

#[test]
fn absent_or_null_keys_take_their_defaults() {
    let v = json!({ "bare": {}, "nulls": { "label": null, "needs": null, "first": null } });
    let idx = AbilitiesIndex::from_json(&v).unwrap();
    let bare = Ability {
        id: "bare".into(),
        label: String::new(),
        needs: vec![],
        request: false,
        first: vec![],
        writes: false,
    };
    assert_eq!(idx.get("bare"), Some(&bare));
    assert_eq!(idx.get("nulls").unwrap().label, "");
}

#[test]
fn input_that_is_not_an_object_is_a_parse_error() {
    assert!(matches!(
        AbilitiesIndex::from_json(&json!([1, 2])),
        Err(CoreError::Parse(_))
    ));
}

#[test]
fn an_entry_of_the_wrong_shape_is_a_parse_error_naming_its_id() {
    for bad in [
        json!({ "odd-one": "not an object" }),
        json!({ "odd-one": { "needs": "web" } }),
        json!({ "odd-one": { "first": ["qwen", 3] } }),
        json!({ "odd-one": { "label": 3 } }),
        json!({ "odd-one": { "request": "yes" } }),
    ] {
        match AbilitiesIndex::from_json(&bad) {
            Err(CoreError::Parse(msg)) => assert!(msg.contains("odd-one"), "{msg}"),
            other => panic!("expected CoreError::Parse for {bad}, got {other:?}"),
        }
    }
}

#[test]
fn from_json_does_not_change_its_input() {
    let v = actions();
    let before = v.clone();
    let _ = AbilitiesIndex::from_json(&v);
    assert_eq!(v, before);
}

#[test]
fn load_reads_fleet_actions_through_the_bridge() {
    let fleet = FleetBridge::with_root(&stub());
    assert_eq!(AbilitiesIndex::load(&fleet), Ok(index()));
}

#[test]
fn load_turns_a_fleet_failure_into_a_core_fleet_error() {
    let fleet = FleetBridge::with_root(&stub()).with_env("FLEET_STUB_EXIT", "4");
    match AbilitiesIndex::load(&fleet) {
        Err(CoreError::Fleet(e)) => assert_eq!(e.exit_code, Some(4)),
        other => panic!("expected CoreError::Fleet, got {other:?}"),
    }
}

#[test]
fn the_code_map_shows_core_abilities_present() {
    let map = include_str!("../../../docs/code-map.md");
    let line = map
        .lines()
        .skip_while(|l| *l != "### core-abilities")
        .nth(1)
        .unwrap();
    assert!(line.contains("· present."), "{line}");
}
