//! Tests for FD-1 (`route` stops at the first step that matches: a trigger word, an explicit
//! ask, the labeller, then the budget and complexity rule, and the `Route` says which step
//! picked). The index is the stub's `actions.json` plus one stub kind carrying the `code` need,
//! and the stub config's models; the budget gate is a fixture built in code.

use serde_json::{json, Value};
use std::path::PathBuf;
use switchboard_core::abilities::{AbilitiesIndex, FleetModels};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::frontdoor::router::{route, BudgetGate, Complexity, Pick, Route, Step};

const GEMINI_MODEL: &str = "gemini-3.8-flash-medium";
const QWEN_MODEL: &str = "qwen3.8-27b";
const CLAUDEAPI_MODEL: &str = "claude-sonnet-5-5";

struct Braked(Vec<&'static str>);

impl BudgetGate for Braked {
    fn allows(&self, service: &str) -> bool {
        !self.0.contains(&service)
    }
}

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fleet-stub")
}

fn index() -> AbilitiesIndex {
    index_with(true)
}

/// The stub table, with or without the stub request kind that carries `code`; without it only
/// a job action carries `code`, as in the real action table.
fn index_with(coding_kind: bool) -> AbilitiesIndex {
    let text = std::fs::read_to_string(stub().join("answers").join("actions.json")).unwrap();
    let mut value: Value = serde_json::from_str(&text).unwrap();
    if coding_kind {
        value.as_object_mut().unwrap().insert(
            "stub-coding".to_string(),
            json!({
                "label": "Stub: a coding kind",
                "request": true,
                "needs": ["code"],
                "first": ["gemini"],
                "ok": { "gemini": { "model": GEMINI_MODEL, "effort": "-" } }
            }),
        );
    }
    let models = FleetModels::read(&FleetBridge::with_root(&stub()), Some("strix-halo")).unwrap();
    AbilitiesIndex::from_json_with(&value, &models)
        .unwrap()
        .with_agent("Scout", "gemini", GEMINI_MODEL)
}

fn go(message: &str, braked: &[&'static str], complexity: Complexity) -> Route {
    route(message, &index(), &Braked(braked.to_vec()), complexity)
}

fn service(service: &str, model: &str) -> Pick {
    Pick::Service {
        service: service.to_string(),
        model: model.to_string(),
    }
}

#[test]
fn a_named_agent_routes_at_the_trigger_step_even_when_the_rules_would_label_it() {
    let r = go("scout, define entropy", &[], Complexity::Small);
    assert_eq!(
        r.step,
        Step::Trigger {
            agent: "scout".to_string()
        }
    );
    assert_eq!(r.kind, None);
    assert_eq!(r.pick, service("gemini", GEMINI_MODEL));
    // A whole word only.
    let r = go("define scouting", &[], Complexity::Small);
    assert_eq!(
        r.step,
        Step::Labeller {
            kind: "quick-lookup".to_string()
        }
    );
}

#[test]
fn harmon_and_the_therapist_are_a_claude_handoff() {
    for message in ["Harmon, what is up", "talk to the Therapist"] {
        let r = go(message, &[], Complexity::Small);
        assert!(matches!(r.step, Step::Trigger { .. }), "{message}");
        assert!(matches!(r.pick, Pick::ClaudeHandoff(_)), "{message}");
    }
    let r = go("harmon", &["claude"], Complexity::Small);
    assert_eq!(
        r.pick,
        Pick::Wait("waiting for Claude's budget".to_string())
    );
}

#[test]
fn a_braked_agent_pool_waits_with_the_reason() {
    let r = go("scout please", &["gemini"], Complexity::Small);
    assert_eq!(r.pick, Pick::Wait("waiting for budget: scout".to_string()));
}

#[test]
fn search_the_web_is_an_explicit_ask_for_web() {
    let r = go(
        "Please search the web for tide tables",
        &[],
        Complexity::Small,
    );
    assert_eq!(
        r.step,
        Step::ExplicitAsk {
            need: "web".to_string()
        }
    );
    assert_eq!(r.kind.as_deref(), Some("web-research"));
    assert!(matches!(
        r.pick,
        Pick::Service { .. } | Pick::ClaudeHandoff(_)
    ));
}

#[test]
fn write_code_is_an_explicit_ask_for_code() {
    let r = go("write code for a stack", &[], Complexity::Small);
    assert_eq!(
        r.step,
        Step::ExplicitAsk {
            need: "code".to_string()
        }
    );
    assert_eq!(r.kind.as_deref(), Some("stub-coding"));
    assert_eq!(r.pick, service("gemini", GEMINI_MODEL));
    let r = go("write code for a stack", &["gemini"], Complexity::Small);
    assert_eq!(
        r.pick,
        Pick::Wait("waiting for budget: stub-coding".to_string())
    );
}

#[test]
fn a_message_the_rules_label_routes_at_the_labeller() {
    let r = go("what is a monad", &[], Complexity::BigWithTools);
    assert_eq!(
        r.step,
        Step::Labeller {
            kind: "quick-lookup".to_string()
        }
    );
    assert_eq!(r.kind.as_deref(), Some("quick-lookup"));
    assert_eq!(r.pick, service("qwen", QWEN_MODEL));
    let r = go("what is a monad", &["qwen"], Complexity::Small);
    assert_eq!(r.pick, service("gemini", GEMINI_MODEL));
}

#[test]
fn a_message_nothing_matches_takes_the_budget_rule() {
    let r = go("hello there", &[], Complexity::Small);
    assert_eq!(r.step, Step::BudgetRule);
    assert_eq!(r.kind, None);
    assert_eq!(r.pick, service("qwen", QWEN_MODEL));

    let r = go("hello there", &[], Complexity::BigNoTools);
    assert_eq!(r.step, Step::BudgetRule);
    assert_eq!(r.pick, service("claudeapi", CLAUDEAPI_MODEL));

    let r = go("hello there", &[], Complexity::BigWithTools);
    assert_eq!(r.step, Step::BudgetRule);
    assert!(matches!(r.pick, Pick::ClaudeHandoff(_)));
}

#[test]
fn the_budget_rule_skips_braked_pools_and_waits_with_a_reason() {
    let r = go("hello there", &["qwen"], Complexity::Small);
    assert_eq!(r.pick, service("gemini", GEMINI_MODEL));
    let r = go("hello there", &["claudeapi"], Complexity::BigNoTools);
    assert_eq!(
        r.pick,
        Pick::Wait("waiting for budget: claudeapi".to_string())
    );
    let r = go("hello there", &["claude"], Complexity::BigWithTools);
    assert_eq!(
        r.pick,
        Pick::Wait("waiting for Claude's budget".to_string())
    );
}

fn go_real(message: &str, braked: &[&'static str]) -> Route {
    route(
        message,
        &index_with(false),
        &Braked(braked.to_vec()),
        Complexity::Small,
    )
}

#[test]
fn write_code_with_only_a_job_action_carrying_code_is_a_claude_handoff() {
    let r = go_real("write code to parse a csv", &[]);
    assert_eq!(
        r.step,
        Step::ExplicitAsk {
            need: "code".to_string()
        }
    );
    assert_eq!(r.kind, None);
    assert_eq!(
        r.pick,
        Pick::ClaudeHandoff("code: needs a Claude session with tools".to_string())
    );
    let r = go_real("write code to parse a csv", &["claude"]);
    assert_eq!(
        r.step,
        Step::ExplicitAsk {
            need: "code".to_string()
        }
    );
    assert_eq!(
        r.pick,
        Pick::Wait("waiting for Claude's budget".to_string())
    );
}

#[test]
fn ask_claude_is_an_explicit_ask_for_claude() {
    for message in ["Ask Claude: why is the sky blue", "open in Claude please"] {
        let r = go_real(message, &[]);
        assert_eq!(
            r.step,
            Step::ExplicitAsk {
                need: "claude".to_string()
            },
            "{message}"
        );
        assert_eq!(r.kind, None);
        assert_eq!(
            r.pick,
            Pick::ClaudeHandoff("claude: needs a Claude session with tools".to_string())
        );
    }
    let r = go_real("ask claude about it", &["claude"]);
    assert_eq!(
        r.pick,
        Pick::Wait("waiting for Claude's budget".to_string())
    );
}

#[test]
fn search_the_web_still_takes_the_request_kind_pick() {
    let r = go_real("search the web for x", &[]);
    assert_eq!(
        r.step,
        Step::ExplicitAsk {
            need: "web".to_string()
        }
    );
    assert_eq!(r.kind.as_deref(), Some("web-research"));
}

#[test]
fn an_ask_whose_need_nothing_carries_falls_through() {
    let index = AbilitiesIndex::default().with_ask("do a thing", "nothing-has-this");
    let r = route(
        "please do a thing",
        &index,
        &Braked(vec![]),
        Complexity::Small,
    );
    assert_ne!(
        r.step,
        Step::ExplicitAsk {
            need: "nothing-has-this".to_string()
        }
    );
    assert_eq!(r.step, Step::BudgetRule);
}
