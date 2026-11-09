//! Issue 130 (FD-3): `pick_for_kind` over the stub's `answers/actions.json` and a fixture budget
//! gate, plus stub kinds for the Claude subscription; and `Ability.models` read from `ok`.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;
use switchboard_core::abilities::AbilitiesIndex;
use switchboard_core::frontdoor::router::{pick_for_kind, BudgetGate, Pick};

fn actions() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fleet-stub")
        .join("answers")
        .join("actions.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn index() -> AbilitiesIndex {
    AbilitiesIndex::from_json(&actions()).unwrap()
}

/// The fixture budget gate: every service is allowed except the braked ones.
struct Braked(BTreeSet<String>);

impl BudgetGate for Braked {
    fn allows(&self, service: &str) -> bool {
        !self.0.contains(service)
    }
}

fn braked(services: &[&str]) -> Braked {
    Braked(services.iter().map(|s| s.to_string()).collect())
}

fn model(index: &AbilitiesIndex, kind: &str, service: &str) -> String {
    index
        .get(kind)
        .unwrap()
        .models
        .get(service)
        .cloned()
        .unwrap_or_else(|| panic!("the fixture has no {service} model for {kind}"))
}

fn service(service: &str, model: &str) -> Pick {
    Pick::Service {
        service: service.into(),
        model: model.into(),
    }
}

/// Stub kinds over the fixture's services, for the Claude subscription cases.
fn stub_index() -> AbilitiesIndex {
    let gemini = json!({ "model": "gemini-3.8-flash-medium", "effort": "-" });
    let deepseek = json!({ "model": "deepseek/deepseek-flash", "effort": "-" });
    AbilitiesIndex::from_json(&json!({
        "claude-then-gemini": {
            "label": "Stub", "request": true, "first": ["claude", "gemini"],
            "ok": { "claude": "sonnet", "gemini": gemini }
        },
        "claude-only": {
            "label": "Stub", "request": true, "first": ["claude"],
            "ok": { "claude": "sonnet" }
        },
        "deepseek-then-gemini": {
            "label": "Stub", "request": true, "first": ["deepseek", "gemini"],
            "ok": { "deepseek": deepseek, "gemini": gemini }
        }
    }))
    .unwrap()
}

#[test]
fn quick_lookup_takes_qwen_then_gemini_when_qwen_is_braked() {
    let idx = index();
    let qwen = model(&idx, "quick-lookup", "qwen");
    assert_eq!(
        pick_for_kind(&idx, "quick-lookup", &braked(&[])),
        service("qwen", &qwen)
    );
    assert_eq!(
        pick_for_kind(&idx, "quick-lookup", &braked(&["qwen"])),
        service("gemini", "gemini-3.8-flash-medium")
    );
}

#[test]
fn web_research_falls_to_gemini_then_waits_with_no_paid_service() {
    let idx = index();
    assert_eq!(
        pick_for_kind(&idx, "web-research", &braked(&["chatgpt"])),
        service("gemini", "gemini-3.8-flash-medium")
    );
    assert_eq!(
        pick_for_kind(&idx, "web-research", &braked(&["chatgpt", "gemini"])),
        Pick::Wait("waiting for budget: web-research".into())
    );
}

#[test]
fn a_paid_pool_is_only_overflow() {
    let idx = index();
    assert_eq!(
        pick_for_kind(&idx, "general-question", &braked(&["gemini"])),
        service("deepseek", "deepseek/deepseek-flash")
    );
    assert_eq!(
        pick_for_kind(&idx, "general-question", &braked(&[])),
        service("gemini", "gemini-3.8-flash-medium")
    );
    // deepseek listed first still waits for pass two while gemini is allowed.
    let stub = stub_index();
    assert_eq!(
        pick_for_kind(&stub, "deepseek-then-gemini", &braked(&[])),
        service("gemini", "gemini-3.8-flash-medium")
    );
    assert_eq!(
        pick_for_kind(&stub, "deepseek-then-gemini", &braked(&["gemini"])),
        service("deepseek", "deepseek/deepseek-flash")
    );
}

#[test]
fn the_claude_subscription_is_handed_off_never_called() {
    let stub = stub_index();
    match pick_for_kind(&stub, "claude-then-gemini", &braked(&[])) {
        Pick::ClaudeHandoff(why) => {
            assert!(why.contains("claude-then-gemini"), "{why}");
            assert!(!why.contains('\n'), "{why}");
        }
        other => panic!("expected a ClaudeHandoff, got {other:?}"),
    }
    assert_eq!(
        pick_for_kind(&stub, "claude-then-gemini", &braked(&["claude"])),
        service("gemini", "gemini-3.8-flash-medium")
    );
    assert_eq!(
        pick_for_kind(&stub, "claude-then-gemini", &braked(&["claude", "gemini"])),
        Pick::Wait("waiting for Claude's budget".into())
    );
    assert_eq!(
        pick_for_kind(&stub, "claude-only", &braked(&["claude"])),
        Pick::Wait("waiting for Claude's budget".into())
    );
}

#[test]
fn judgement_planning_takes_claudeapi_or_waits() {
    let idx = index();
    let claudeapi = model(&idx, "judgement-planning", "claudeapi");
    assert_eq!(
        pick_for_kind(&idx, "judgement-planning", &braked(&[])),
        service("claudeapi", &claudeapi)
    );
    assert!(matches!(
        pick_for_kind(&idx, "judgement-planning", &braked(&["claudeapi"])),
        Pick::Wait(_)
    ));
}

#[test]
fn an_unknown_id_or_a_job_action_waits() {
    let idx = index();
    assert_eq!(
        pick_for_kind(&idx, "no-such-kind", &braked(&[])),
        Pick::Wait("unknown request kind no-such-kind".into())
    );
    assert_eq!(
        pick_for_kind(&idx, "small-code-change-exact-brief", &braked(&[])),
        Pick::Wait("unknown request kind small-code-change-exact-brief".into())
    );
}

#[test]
fn ability_models_come_from_ok_objects_only() {
    let idx = index();
    let web = &idx.get("web-research").unwrap().models;
    assert_eq!(web.get("chatgpt").map(String::as_str), Some("gpt-6-luna"));
    assert_eq!(
        web.get("gemini").map(String::as_str),
        Some("gemini-3.8-flash-medium")
    );
    // null and false entries have no row.
    assert_eq!(web.len(), 2);
    let general = &idx.get("general-question").unwrap().models;
    assert!(!general.contains_key("qwen"));
    assert!(!general.contains_key("claude"));
    // a bare string has no row either.
    assert!(!idx
        .get("writing-editing")
        .unwrap()
        .models
        .contains_key("claude"));
    let none = AbilitiesIndex::from_json(&json!({ "x": { "label": "X" } })).unwrap();
    assert!(none.get("x").unwrap().models.is_empty());
}
