//! Tests for FD-3 (`pick_for_kind` takes the first service in a kind's `first` list whose pool
//! is under budget, resetting or free services before the paid pools, the Claude subscription
//! handed off rather than called, and a wait with a reason when nothing is left). The index is
//! the stub's `answers/actions.json`, unmodified, plus two stub kinds that list `claude`; the
//! models `actions.json` leaves as flags (`ok.qwen: true`, top-level `claudeapi: true`) come
//! from the stub's `tools/fleet/config.json` (`FleetModels`, the ruling on #130). The budget
//! gate is a fixture built in code. No clock is read.

use serde_json::{json, Value};
use std::ffi::OsStr;
use std::path::PathBuf;
use switchboard_core::abilities::{AbilitiesIndex, FleetModels};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::frontdoor::router::{pick_for_kind, BudgetGate, Pick};

const GEMINI_MODEL: &str = "gemini-3.8-flash-medium";
const DEEPSEEK_MODEL: &str = "deepseek/deepseek-flash";
const CHATGPT_MODEL: &str = "gpt-6-luna";
/// The machine the tests run as, and its `qwen_model` in the config fixture.
const MACHINE: &str = "strix-halo";
const QWEN_MODEL: &str = "qwen3.8-27b";
/// `claudeapi_models.sonnet` in the config fixture: what `ok.claude: "sonnet"` maps to.
const CLAUDEAPI_MODEL: &str = "claude-sonnet-5-5";

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

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fleet-stub")
}

/// The fixture `actions.json`, as it is on disk.
fn actions() -> Value {
    let path = stub().join("answers").join("actions.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The fixture `config.json`'s models for MACHINE, read the way the core reads them.
fn fleet_models() -> FleetModels {
    FleetModels::read(&FleetBridge::with_root(&stub()), Some(MACHINE)).unwrap()
}

/// `actions()` plus `claude-then-gemini` (`first` [claude, gemini]) and `claude-only`
/// (`first` [claude]), the stub kinds the Claude subscription rows need.
fn actions_with_claude_kinds() -> Value {
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
    value
}

/// The index the front door would hold on MACHINE: the fixture plus the config's models.
fn index() -> AbilitiesIndex {
    AbilitiesIndex::from_json_with(&actions_with_claude_kinds(), &fleet_models()).unwrap()
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

/// A fresh pipeline root with the stub `fleet.py` and nothing else (no `config.json`).
fn root_without_config(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sb-fd3-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let fleet = root.join("tools").join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::create_dir_all(root.join("answers")).unwrap();
    std::fs::copy(stub().join("tools/fleet/fleet.py"), fleet.join("fleet.py")).unwrap();
    std::fs::copy(
        stub().join("answers/actions.json"),
        root.join("answers").join("actions.json"),
    )
    .unwrap();
    root
}

// --- Done when: the picks over the unmodified fixture ---

#[test]
fn quick_lookup_takes_qwen_with_this_machines_model_then_gemini_when_qwen_is_braked() {
    // quick-lookup: first [qwen, gemini]; the fixture's `ok.qwen` is `true`, so the model is
    // the config's `machines.strix-halo.qwen_model`.
    assert_eq!(pick("quick-lookup", &[]), service("qwen", QWEN_MODEL));
    assert_eq!(
        pick("quick-lookup", &["qwen"]),
        service("gemini", GEMINI_MODEL)
    );
}

#[test]
fn judgement_planning_goes_to_the_claude_api_or_waits() {
    // judgement-planning: first [claudeapi]; the fixture's top-level `claudeapi: true` with
    // `ok.claude: "sonnet"` maps through the config's `claudeapi_models`.
    assert_eq!(
        pick("judgement-planning", &[]),
        service("claudeapi", CLAUDEAPI_MODEL)
    );
    assert_eq!(
        pick("judgement-planning", &["claudeapi"]),
        Pick::Wait("waiting for budget: judgement-planning".to_string())
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

// --- The flags without a config: no model, so the service is skipped ---

#[test]
fn without_the_fleet_config_qwen_and_the_claude_api_have_no_model_and_are_skipped() {
    // `from_json` (and `load`) carry the explicit rows only: quick-lookup falls to gemini and
    // judgement-planning can only wait.
    let idx = AbilitiesIndex::from_json(&actions()).unwrap();
    assert_eq!(idx.model("quick-lookup", "qwen"), None);
    assert_eq!(idx.model("judgement-planning", "claudeapi"), None);
    assert_eq!(
        pick_for_kind(&idx, "quick-lookup", &all_allowed()),
        service("gemini", GEMINI_MODEL)
    );
    assert_eq!(
        pick_for_kind(&idx, "judgement-planning", &all_allowed()),
        Pick::Wait("waiting for budget: judgement-planning".to_string())
    );
}

#[test]
fn a_root_with_no_config_json_reads_no_fleet_models_and_load_with_skips_those_services() {
    let root = root_without_config("missing");
    let fleet = FleetBridge::with_root(&root);
    assert_eq!(FleetModels::read(&fleet, Some(MACHINE)), None);
    let idx = AbilitiesIndex::load_with(&fleet, Some(MACHINE)).unwrap();
    assert_eq!(idx.model("quick-lookup", "qwen"), None);
    assert_eq!(idx.model("judgement-planning", "claudeapi"), None);
    assert_eq!(
        pick_for_kind(&idx, "quick-lookup", &all_allowed()),
        service("gemini", GEMINI_MODEL)
    );
    assert_eq!(
        pick_for_kind(&idx, "judgement-planning", &all_allowed()),
        Pick::Wait("waiting for budget: judgement-planning".to_string())
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_config_json_that_is_not_json_reads_as_none() {
    let root = root_without_config("badjson");
    std::fs::write(root.join("tools/fleet/config.json"), "{ not json").unwrap();
    assert_eq!(
        FleetModels::read(&FleetBridge::with_root(&root), Some(MACHINE)),
        None
    );
    let _ = std::fs::remove_dir_all(&root);
}

// --- FleetModels from the config fixture ---

#[test]
fn the_fleet_models_are_read_from_config_json_next_to_fleet_py() {
    assert_eq!(
        fleet_models(),
        FleetModels {
            qwen: Some(QWEN_MODEL.to_string()),
            claudeapi: [
                ("haiku", "claude-haiku-5-5"),
                ("sonnet", CLAUDEAPI_MODEL),
                ("opus", "claude-opus-5-5"),
            ]
            .into_iter()
            .map(|(tier, model)| (tier.to_string(), model.to_string()))
            .collect(),
        }
    );
}

#[test]
fn load_with_fills_the_flags_from_the_config_and_plain_load_does_not() {
    let fleet = FleetBridge::with_root(&stub());
    let with = AbilitiesIndex::load_with(&fleet, Some(MACHINE)).unwrap();
    assert_eq!(with.model("quick-lookup", "qwen"), Some(QWEN_MODEL));
    assert_eq!(
        with.model("judgement-planning", "claudeapi"),
        Some(CLAUDEAPI_MODEL)
    );
    assert_eq!(with.entries, AbilitiesIndex::load(&fleet).unwrap().entries);
    let plain = AbilitiesIndex::load(&fleet).unwrap();
    assert_eq!(plain.model("quick-lookup", "qwen"), None);
    assert_eq!(plain.model("judgement-planning", "claudeapi"), None);
}

#[test]
fn a_machine_with_no_qwen_model_gives_qwen_no_row_and_the_claude_api_keeps_its_model() {
    let fleet = FleetBridge::with_root(&stub());
    for machine in [Some("amd-handheld"), Some("no-such-machine"), None] {
        let models = FleetModels::read(&fleet, machine).unwrap();
        assert_eq!(models.qwen, None, "{machine:?}");
        let idx = AbilitiesIndex::from_json_with(&actions(), &models).unwrap();
        assert_eq!(
            pick_for_kind(&idx, "quick-lookup", &all_allowed()),
            service("gemini", GEMINI_MODEL),
            "{machine:?}"
        );
        assert_eq!(
            pick_for_kind(&idx, "judgement-planning", &all_allowed()),
            service("claudeapi", CLAUDEAPI_MODEL),
            "{machine:?}"
        );
    }
    let other = FleetModels::read(&fleet, Some("desktop-3090ti")).unwrap();
    assert_eq!(other.qwen.as_deref(), Some("qwen3-coder-30b-a3b-instruct"));
}

#[test]
fn a_claude_tier_the_config_does_not_map_or_a_missing_tier_gives_the_claude_api_no_model() {
    let table = json!({
        "unmapped-tier": {
            "label": "Stub", "request": true, "first": ["claudeapi"],
            "ok": { "claude": "sonnet-9" }, "claudeapi": true
        },
        "no-tier": {
            "label": "Stub", "request": true, "first": ["claudeapi"],
            "ok": { "claude": null }, "claudeapi": true
        },
        "flag-off": {
            "label": "Stub", "request": true, "first": ["claudeapi"],
            "ok": { "claude": "sonnet" }, "claudeapi": false
        },
        "explicit-row-wins": {
            "label": "Stub", "request": true, "first": ["claudeapi"],
            "ok": { "claude": "sonnet", "claudeapi": { "model": "claude-opus-5-5" } },
            "claudeapi": true
        }
    });
    let idx = AbilitiesIndex::from_json_with(&table, &fleet_models()).unwrap();
    for kind in ["unmapped-tier", "no-tier", "flag-off"] {
        assert_eq!(idx.model(kind, "claudeapi"), None, "{kind}");
        assert_eq!(
            pick_for_kind(&idx, kind, &all_allowed()),
            Pick::Wait(format!("waiting for budget: {kind}")),
            "{kind}"
        );
    }
    assert_eq!(
        idx.model("explicit-row-wins", "claudeapi"),
        Some("claude-opus-5-5")
    );
}

#[test]
fn a_bare_true_for_a_service_other_than_qwen_gives_no_model() {
    let table = json!({
        "gemini-flag": {
            "label": "Stub", "request": true, "first": ["gemini"],
            "ok": { "gemini": true, "qwen": true }
        }
    });
    let idx = AbilitiesIndex::from_json_with(&table, &fleet_models()).unwrap();
    assert_eq!(idx.model("gemini-flag", "gemini"), None);
    assert_eq!(idx.model("gemini-flag", "qwen"), Some(QWEN_MODEL));
    assert_eq!(
        pick_for_kind(&idx, "gemini-flag", &all_allowed()),
        Pick::Wait("waiting for budget: gemini-flag".to_string())
    );
}

#[test]
fn from_config_takes_only_string_models_and_tolerates_missing_keys() {
    assert_eq!(
        FleetModels::from_config(&json!({}), Some(MACHINE)),
        FleetModels::default()
    );
    let odd = json!({
        "claudeapi_models": { "sonnet": "claude-sonnet-5-5", "opus": 7 },
        "machines": { "strix-halo": { "qwen_model": 3 }, "box": { "qwen_model": "q" } }
    });
    let models = FleetModels::from_config(&odd, Some(MACHINE));
    assert_eq!(models.qwen, None);
    assert_eq!(
        models
            .claudeapi
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["sonnet"]
    );
    assert_eq!(
        FleetModels::from_config(&odd, Some("box")).qwen.as_deref(),
        Some("q")
    );
}

#[test]
fn the_machine_token_is_the_env_when_set_else_the_machine_file_else_none() {
    let dir = std::env::temp_dir().join(format!("sb-fd3-machine-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("machine");
    std::fs::write(&file, " strix-halo \n").unwrap();
    let blank = dir.join("blank");
    std::fs::write(&blank, "\n").unwrap();
    assert_eq!(
        FleetModels::resolve_machine(Some(OsStr::new(" desktop-3090ti ")), Some(&file)),
        Some("desktop-3090ti".to_string())
    );
    assert_eq!(
        FleetModels::resolve_machine(Some(OsStr::new("  ")), Some(&file)),
        Some("strix-halo".to_string())
    );
    assert_eq!(
        FleetModels::resolve_machine(None, Some(&file)),
        Some("strix-halo".to_string())
    );
    assert_eq!(FleetModels::resolve_machine(None, Some(&blank)), None);
    assert_eq!(
        FleetModels::resolve_machine(None, Some(&dir.join("absent"))),
        None
    );
    assert_eq!(FleetModels::resolve_machine(None, None), None);
    let _ = std::fs::remove_dir_all(&dir);
}

// --- The models map ---

#[test]
fn models_are_read_from_ok_and_the_flags_and_empty_for_null_false_or_a_bare_string() {
    let idx = index();
    assert_eq!(idx.model("quick-lookup", "gemini"), Some(GEMINI_MODEL));
    assert_eq!(idx.model("quick-lookup", "qwen"), Some(QWEN_MODEL)); // true + config
    assert_eq!(idx.model("quick-lookup", "claude"), None); // null
    assert_eq!(idx.model("general-question", "qwen"), None); // false
    assert_eq!(idx.model("writing-editing", "claude"), None); // "sonnet": the subscription
    assert_eq!(idx.model("writing-editing", "qwen"), None); // false

    // `claudeapi: true` with `ok.claude: "sonnet"`: the config's sonnet model.
    assert_eq!(
        idx.model("writing-editing", "claudeapi"),
        Some(CLAUDEAPI_MODEL)
    );
    assert_eq!(idx.model("judgement-planning", "gemini"), None);
    assert_eq!(idx.model("no-such-kind", "gemini"), None);
    // Three explicit rows plus both flags (`ok.claude: "haiku"`, `ok.qwen: true`).
    let rows = &idx.models["small-code-change-exact-brief"];
    assert_eq!(
        rows.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["chatgpt", "claudeapi", "deepseek", "gemini", "qwen"]
    );
    assert_eq!(rows["claudeapi"], "claude-haiku-5-5");
    assert_eq!(rows["qwen"], QWEN_MODEL);
    // Without the config the same entry holds the explicit rows only.
    let plain = AbilitiesIndex::from_json(&actions()).unwrap();
    assert_eq!(
        plain.models["small-code-change-exact-brief"]
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["chatgpt", "deepseek", "gemini"]
    );
    let rows = &idx.models["judgement-planning"];
    assert_eq!(
        rows.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["claudeapi"]
    );
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
