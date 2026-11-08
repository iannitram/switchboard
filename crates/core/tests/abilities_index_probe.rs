//! R28 probe for issue 2: behaviour the issue states that the locked tests in
//! `abilities_index.rs` do not cover. Added in the review; the locked file is unchanged.

use serde_json::json;
use switchboard_core::abilities::{AbilitiesIndex, Ability};
use switchboard_core::CoreError;

#[test]
fn null_request_and_writes_take_their_defaults() {
    let v = json!({
        "x": { "label": null, "needs": null, "first": null, "request": null, "writes": null }
    });
    let idx = AbilitiesIndex::from_json(&v).unwrap();
    assert_eq!(
        idx.get("x"),
        Some(&Ability {
            id: "x".into(),
            label: String::new(),
            needs: vec![],
            request: false,
            first: vec![],
            writes: false,
        })
    );
}

#[test]
fn a_top_level_null_or_string_is_a_parse_error() {
    for bad in [json!(null), json!("text")] {
        assert!(
            matches!(AbilitiesIndex::from_json(&bad), Err(CoreError::Parse(_))),
            "expected CoreError::Parse for {bad}"
        );
    }
}

#[test]
fn a_needs_list_with_a_non_string_is_a_parse_error_naming_its_id() {
    match AbilitiesIndex::from_json(&json!({ "bad": { "needs": [1] } })) {
        Err(CoreError::Parse(msg)) => assert!(msg.contains("bad"), "{msg}"),
        other => panic!("expected CoreError::Parse, got {other:?}"),
    }
}

#[test]
fn a_job_action_that_needs_web_is_not_listed_under_web() {
    let v = json!({
        "job-web": { "label": "Job", "needs": ["web"], "writes": false },
        "req-web": { "label": "Ask", "needs": ["web"], "request": true }
    });
    let idx = AbilitiesIndex::from_json(&v).unwrap();
    let got: Vec<&str> = idx
        .kinds_needing("web")
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(got, vec!["req-web"]);
}
