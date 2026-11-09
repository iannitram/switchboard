//! Tests for FD-3 (`pick_for_kind` takes the first service in a kind's `first` list whose pool
//! is under budget, resetting or free services before the paid pools, the Claude subscription
//! handed off rather than called, and a wait with a reason when nothing is left). The index is
//! the stub's `answers/actions.json` plus two stub kinds that list `claude`; the budget gate is
//! a fixture built in code. No clock is read and no fleet is called.

use serde_json::{json, Value};
use std::path::PathBuf;
use switchboard_core::abilities::AbilitiesIndex;
use switchboard_core::frontdoor::router::{pick_for_kind, BudgetGate, Pick};

const GEMINI_MODEL: &str = "gemini-3.8-flash-medium";
const DEEPSEEK_MODEL: &str = "deepseek/deepseek-flash";
const CHATGPT_MODEL: &str = "gpt-6-luna";
const QWEN_MODEL: &str = "qwen3-27b";

/// A budget gate that brakes the listed services and allows every other one.
struct Braked(Vec<&'static str>);

impl BudgetGate for Braked {
    fn allows(&self, service: &str) -> bool {
        !self.0.contains(&service)
    }
}

fn all_allowed() -> Braked {
    Braked(vec![])
}

fn actions() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fleet-stub")
        .join("answers")
        .join("actions.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The fixture index plus `claude-then-gemini` (`first` [claude, gemini]) and `claude-only`
/// (`first` [claude]), the stub kinds the Claude subscription rows need.
fn index() -> AbilitiesIndex {
    let mut value = actions();
    let table = value.as_object_mut().unwrap();
    table.insert(
        "claude-then-gemini".to_string(),
        json!({
            "label": "Stub: Claude first, Gemini next",
            "request": true,
            "first": ["claude", "gemini"],
            "ok": { "claude": "sonnet", "gemini": { "model": GEMINI_MODEL, "effort": "-" } }
        }),
    );
    table.insert(
        "claude-only".to_string(),
        json!({
            "label": "Stub: Claude only",
            "request": true,
            "first": ["claude"],
            "ok": { "claude": "sonnet" }
        }),
    );
    AbilitiesIndex::from_json(&value).unwrap()
}

fn service(service: &str, model: &str) -> Pick {
    Pick::Service {
        service: service.to_string(),
        model: model.to_string(),
    }
}

fn pick(kind: &str, braked: &[&'static str]) -> Pick {
    pick_for_kind(&index(), kind, &Braked(braked.to_vec()))
}

/// The fixture index with `quick-lookup`'s qwen given a model row (the fixture's `qwen: true`
/// is a flag with no model, so on the fixture as it stands qwen is skipped; see
/// `a_service_without_a_model_row_is_skipped`).
fn index_with_qwen_model() -> AbilitiesIndex {
    let mut value = actions();
    value["quick-lookup"]["ok"]["qwen"] = json!({ "model": QWEN_MODEL, "effort": "-" });
    AbilitiesIndex::from_json(&value).unwrap()
}

#[test]
fn the_first_allowed_service_is_taken_with_its_model() {
    // quick-lookup: first [qwen, gemini].
    let idx = index_with_qwen_model();
    assert_eq!(
        pick_for_kind(&idx, "quick-lookup", &all_allowed()),
        service("qwen", QWEN_MODEL)
    );
    assert_eq!(
        pick_for_kind(&idx, "quick-lookup", &Braked(vec!["qwen"])),
        service("gemini", GEMINI_MODEL)
    );
}

#[test]
fn a_braked_first_service_falls_to_the_next_one() {
    // web-research: first [chatgpt, gemini].
    assert_eq!(pick("web-research", &[]), service("chatgpt", CHATGPT_MODEL));
    assert_eq!(
        pick("web-research", &["chatgpt"]),
        service("gemini", GEMINI_MODEL)
    );
}

#[test]
fn both_braked_with_no_paid_service_waits_for_budget() {
    assert_eq!(
        pick("web-research", &["chatgpt", "gemini"]),
        Pick::Wait("waiting for budget: web-research".to_string())
    );
}

#[test]
fn a_paid_pool_is_overflow_only() {
    // general-question: first [gemini, deepseek].
    assert_eq!(
        pick("general-question", &["gemini"]),
        service("deepseek", DEEPSEEK_MODEL)
    );
    assert_eq!(
        pick("general-question", &[]),
        service("gemini", GEMINI_MODEL)
    );
}

#[test]
fn a_resetting_service_listed_after_a_paid_pool_still_comes_first() {
    let mut value = actions();
    value.as_object_mut().unwrap().insert(
        "paid-listed-first".to_string(),
        json!({
            "label": "Stub: DeepSeek listed before Gemini",
            "request": true,
            "first": ["deepseek", "gemini"],
            "ok": {
                "deepseek": { "model": DEEPSEEK_MODEL, "effort": "-" },
                "gemini": { "model": GEMINI_MODEL, "effort": "-" }
            }
        }),
    );
    let idx = AbilitiesIndex::from_json(&value).unwrap();
    assert_eq!(
        pick_for_kind(&idx, "paid-listed-first", &all_allowed()),
        service("gemini", GEMINI_MODEL)
    );
    assert_eq!(
        pick_for_kind(&idx, "paid-listed-first", &Braked(vec!["gemini"])),
        service("deepseek", DEEPSEEK_MODEL)
    );
}

#[test]
fn the_claude_subscription_under_budget_is_a_handoff_naming_the_kind() {
    match pick("claude-then-gemini", &[]) {
        Pick::ClaudeHandoff(reason) => {
            assert!(reason.contains("claude-then-gemini"), "{reason}");
            assert!(!reason.contains('\n'), "{reason}");
        }
        other => panic!("expected a Claude handoff, got {other:?}"),
    }
}

#[test]
fn the_claude_subscription_braked_takes_the_kinds_next_service() {
    assert_eq!(
        pick("claude-then-gemini", &["claude"]),
        service("gemini", GEMINI_MODEL)
    );
}

#[test]
fn a_kind_listing_claude_with_nothing_left_waits_for_claudes_budget() {
    let wait = Pick::Wait("waiting for Claude's budget".to_string());
    assert_eq!(pick("claude-then-gemini", &["claude", "gemini"]), wait);
    assert_eq!(pick("claude-only", &["claude"]), wait);
}

#[test]
fn judgement_planning_goes_to_the_claude_api_or_waits() {
    // judgement-planning: first [claudeapi], a paid pool with no `ok` row of its own; the
    // fixture's `claudeapi: true` is a flag, so the stub gives it a model here.
    let mut value = actions();
    let entry = value["judgement-planning"].as_object_mut().unwrap();
    entry["ok"]["claudeapi"] = json!({ "model": "claude-sonnet", "effort": "-" });
    let idx = AbilitiesIndex::from_json(&value).unwrap();
    assert_eq!(
        pick_for_kind(&idx, "judgement-planning", &all_allowed()),
        service("claudeapi", "claude-sonnet")
    );
    assert_eq!(
        pick_for_kind(&idx, "judgement-planning", &Braked(vec!["claudeapi"])),
        Pick::Wait("waiting for budget: judgement-planning".to_string())
    );
}

#[test]
fn a_service_without_a_model_row_is_skipped() {
    // quick-lookup's qwen is `ok: true`, a flag with no model: gemini is taken over it.
    assert_eq!(pick("quick-lookup", &[]), service("gemini", GEMINI_MODEL));
    assert_eq!(
        pick("quick-lookup", &["gemini"]),
        Pick::Wait("waiting for budget: quick-lookup".to_string())
    );
}

#[test]
fn an_unknown_id_and_a_job_action_each_wait() {
    assert_eq!(
        pick("no-such-kind", &[]),
        Pick::Wait("unknown request kind no-such-kind".to_string())
    );
    assert_eq!(
        pick("small-code-change-exact-brief", &[]),
        Pick::Wait("unknown request kind small-code-change-exact-brief".to_string())
    );
}

#[test]
fn models_are_read_from_ok_and_empty_for_null_false_or_a_bare_string() {
    let idx = index();
    assert_eq!(idx.model("quick-lookup", "gemini"), Some(GEMINI_MODEL));
    assert_eq!(idx.model("quick-lookup", "claude"), None); // null
    assert_eq!(idx.model("quick-lookup", "qwen"), None); // true
    assert_eq!(idx.model("general-question", "qwen"), None); // false
    assert_eq!(idx.model("writing-editing", "claude"), None); // "sonnet"
    assert_eq!(idx.model("judgement-planning", "gemini"), None);
    assert_eq!(idx.model("no-such-kind", "gemini"), None);
    let rows = &idx.models["small-code-change-exact-brief"];
    assert_eq!(
        rows.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["chatgpt", "deepseek", "gemini"]
    );
    assert!(idx.models["judgement-planning"].is_empty());
}

#[test]
fn an_ok_that_is_not_an_object_is_a_parse_error_naming_its_id() {
    match AbilitiesIndex::from_json(&json!({ "odd-one": { "ok": "yes" } })) {
        Err(switchboard_core::CoreError::Parse(msg)) => assert!(msg.contains("odd-one"), "{msg}"),
        other => panic!("expected CoreError::Parse, got {other:?}"),
    }
}

#[test]
fn the_code_map_shows_fd_3_present() {
    let map = include_str!("../../../docs/code-map.md");
    let section: Vec<&str> = map
        .lines()
        .skip_while(|l| *l != "### frontdoor")
        .take_while(|l| !l.starts_with("### ") || *l == "### frontdoor")
        .collect();
    assert!(
        section.contains(&"- FD-3 pick for a kind (`pick_for_kind`): present."),
        "{section:?}"
    );
}
