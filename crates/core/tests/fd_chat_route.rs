//! Locked tests for FD-1-W (the `chat_route` command: the Chat composer's Send, #177). Written
//! before the build (R28); the builder never edits this file.
//!
//! The fixture fleet stub answers `actions` from its answers folder and exits 0 for
//! `budget allow`, or exits `FLEET_STUB_EXIT` for every command. The inline stub below answers
//! `actions` with `{}` and `budget allow` with `STUB_BUDGET_EXIT`, and logs every call.

use serde_json::{json, Value};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use switchboard_core::abilities::{AbilitiesIndex, FleetModels};
use switchboard_core::fleet::{FleetBridge, FleetErrorReason};
use switchboard_core::frontdoor::chat::{
    route_message, route_message_on, route_message_with, ChatPick, ChatRoute, FleetGate,
    DEFAULT_COMPLEXITY,
};
use switchboard_core::frontdoor::router::{self, BudgetGate, Complexity, Pick, Route, Step};
use switchboard_core::CoreError;

/// Logs its arguments to calls.log; `actions` prints `{}`, `budget` exits STUB_BUDGET_EXIT.
const INLINE_FLEET: &str = r#"import os, pathlib, sys
root = pathlib.Path(__file__).resolve().parent.parent.parent
with open(root / "calls.log", "a", encoding="utf-8", newline="\n") as fh:
    fh.write(" ".join(sys.argv[1:]) + "\n")
if sys.argv[1:2] == ["actions"]:
    sys.stdout.write("{}")
    sys.exit(0)
if sys.argv[1:2] == ["budget"]:
    sys.exit(int(os.environ.get("STUB_BUDGET_EXIT", "0")))
sys.exit(2)
"#;

fn fixture_stub() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fleet-stub")
}

fn fixture_fleet(exit: Option<&str>) -> FleetBridge {
    let b = FleetBridge::with_root(&fixture_stub());
    match exit {
        Some(code) => b.with_env("FLEET_STUB_EXIT", code),
        None => b,
    }
}

fn inline_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sb-fd1w-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tools").join("fleet")).unwrap();
    std::fs::write(
        root.join("tools").join("fleet").join("fleet.py"),
        INLINE_FLEET,
    )
    .unwrap();
    root
}

fn calls(root: &Path) -> String {
    std::fs::read_to_string(root.join("calls.log")).unwrap_or_default()
}

fn service(service: &str, model: &str) -> Pick {
    Pick::Service {
        service: service.to_string(),
        model: model.to_string(),
    }
}

fn wire(route: &Route) -> Value {
    serde_json::to_value(ChatRoute::from(route)).unwrap()
}

struct AllowAll;
impl BudgetGate for AllowAll {
    fn allows(&self, _service: &str) -> bool {
        true
    }
}

// --- ChatRoute: the face's `ChatRoute` type (#177: { step, kind, pick }) ---

#[test]
fn a_chat_route_has_exactly_the_face_field_names() {
    let v = wire(&Route {
        step: Step::BudgetRule,
        kind: None,
        pick: service("qwen", "qwen3-coder"),
    });
    let mut keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["kind", "pick", "step"]);
}

#[test]
fn the_step_is_the_line_the_chat_shows() {
    let cases = [
        (
            Step::Trigger {
                agent: "harmon".to_string(),
            },
            "trigger word harmon",
        ),
        (
            Step::ExplicitAsk {
                need: "web".to_string(),
            },
            "explicit ask web",
        ),
        (
            Step::Labeller {
                kind: "quick-lookup".to_string(),
            },
            "labeller",
        ),
        (Step::BudgetRule, "budget rule"),
    ];
    for (step, text) in cases {
        let r = Route {
            step: step.clone(),
            kind: None,
            pick: service("qwen", "qwen3-coder"),
        };
        assert_eq!(wire(&r)["step"], json!(text), "{step:?}");
        assert_eq!(ChatRoute::from(&r).step, text);
    }
}

#[test]
fn the_kind_is_copied_and_none_is_null() {
    let mut r = Route {
        step: Step::Labeller {
            kind: "quick-lookup".to_string(),
        },
        kind: Some("quick-lookup".to_string()),
        pick: service("gemini", "gemini-3-flash"),
    };
    assert_eq!(wire(&r)["kind"], json!("quick-lookup"));
    r.kind = None;
    assert_eq!(wire(&r)["kind"], Value::Null);
}

#[test]
fn a_service_pick_names_the_service_and_model() {
    let r = Route {
        step: Step::Labeller {
            kind: "quick-lookup".to_string(),
        },
        kind: Some("quick-lookup".to_string()),
        pick: service("gemini", "gemini-3-flash"),
    };
    assert_eq!(
        wire(&r),
        json!({
            "step": "labeller",
            "kind": "quick-lookup",
            "pick": { "to": "service", "service": "gemini", "model": "gemini-3-flash" }
        })
    );
    assert_eq!(
        ChatRoute::from(&r).pick,
        ChatPick::Service {
            service: "gemini".to_string(),
            model: "gemini-3-flash".to_string()
        }
    );
}

#[test]
fn a_claude_handoff_pick_carries_its_reason() {
    let r = Route {
        step: Step::BudgetRule,
        kind: None,
        pick: Pick::ClaudeHandoff("big with tools".to_string()),
    };
    assert_eq!(
        wire(&r)["pick"],
        json!({ "to": "claude-handoff", "reason": "big with tools" })
    );
    assert_eq!(
        ChatRoute::from(&r).pick,
        ChatPick::ClaudeHandoff {
            reason: "big with tools".to_string()
        }
    );
}

#[test]
fn a_wait_pick_carries_its_reason() {
    let r = Route {
        step: Step::ExplicitAsk {
            need: "web".to_string(),
        },
        kind: Some("web-search".to_string()),
        pick: Pick::Wait("waiting for Claude's budget".to_string()),
    };
    assert_eq!(
        wire(&r),
        json!({
            "step": "explicit ask web",
            "kind": "web-search",
            "pick": { "to": "wait", "reason": "waiting for Claude's budget" }
        })
    );
}

#[test]
fn converting_does_not_change_the_route() {
    let r = Route {
        step: Step::Trigger {
            agent: "harmon".to_string(),
        },
        kind: Some("code".to_string()),
        pick: Pick::ClaudeHandoff("harmon".to_string()),
    };
    let before = r.clone();
    let _ = ChatRoute::from(&r);
    assert_eq!(r, before);
}

#[test]
fn the_default_complexity_is_small() {
    assert_eq!(DEFAULT_COMPLEXITY, Complexity::Small);
}

// --- FleetGate: `fleet budget allow <service>` for the router (AUTO-1) ---

#[test]
fn the_gate_allows_a_pool_fleet_allows() {
    let fleet = fixture_fleet(None);
    let gate = FleetGate::new(&fleet);
    assert!(gate.allows("gemini"));
    assert_eq!(gate.into_error(), None);
}

#[test]
fn the_gate_refuses_a_braked_pool_without_an_error() {
    let fleet = fixture_fleet(Some("75"));
    let gate = FleetGate::new(&fleet);
    assert!(!gate.allows("gemini"));
    assert_eq!(gate.into_error(), None);
}

#[test]
fn a_fleet_failure_is_never_allowed_and_is_kept() {
    let fleet = fixture_fleet(Some("1"));
    let gate = FleetGate::new(&fleet);
    assert!(!gate.allows("gemini"));
    match gate.into_error() {
        Some(CoreError::Fleet(err)) => {
            assert_eq!(err.exit_code, Some(1));
            assert_eq!(err.reason, FleetErrorReason::Exit);
            assert!(err.command.contains(&"gemini".to_string()), "{err:?}");
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn the_gate_keeps_the_first_error_only() {
    let fleet = fixture_fleet(Some("1"));
    let gate = FleetGate::new(&fleet);
    assert!(!gate.allows("gemini"));
    assert!(!gate.allows("qwen"));
    match gate.into_error() {
        Some(CoreError::Fleet(err)) => {
            assert!(err.command.contains(&"gemini".to_string()), "{err:?}");
            assert!(!err.command.contains(&"qwen".to_string()), "{err:?}");
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn an_empty_service_is_refused_and_kept() {
    let root = inline_root("empty-lane");
    let fleet = FleetBridge::with_root(&root);
    let gate = FleetGate::new(&fleet);
    assert!(!gate.allows(""));
    assert_eq!(
        gate.into_error(),
        Some(CoreError::Refused(
            "budget: no lane to ask about".to_string()
        ))
    );
    assert_eq!(calls(&root), "");
}

#[test]
fn the_gate_asks_budget_allow_for_the_service() {
    let root = inline_root("asks");
    let fleet = FleetBridge::with_root(&root);
    let gate = FleetGate::new(&fleet);
    assert!(gate.allows("deepseek"));
    assert_eq!(calls(&root), "budget allow deepseek --json\n");
}

// --- route_message_with: the message, the index, the gate, the router ---

#[test]
fn an_empty_message_is_refused_without_a_call() {
    for message in ["", "   ", "\n\t "] {
        let root = inline_root("empty-message");
        let fleet = FleetBridge::with_root(&root);
        let called = Cell::new(false);
        let got = route_message_with(&fleet, message, Complexity::Small, |_, _, _, _| {
            called.set(true);
            Route {
                step: Step::BudgetRule,
                kind: None,
                pick: service("qwen", "qwen3-coder"),
            }
        });
        assert_eq!(
            got,
            Err(CoreError::Refused("chat: empty message".to_string())),
            "{message:?}"
        );
        assert!(!called.get(), "the router ran for {message:?}");
        assert_eq!(calls(&root), "", "fleet was called for {message:?}");
    }
}

#[test]
fn the_router_gets_the_message_the_loaded_index_and_the_complexity() {
    let fleet = fixture_fleet(None);
    let expected_index =
        AbilitiesIndex::load_with(&fleet, FleetModels::local_machine().as_deref()).unwrap();
    let seen: Cell<Option<(String, bool, Complexity)>> = Cell::new(None);
    let got = route_message_with(
        &fleet,
        "  look this up  ",
        Complexity::BigNoTools,
        |message, index, _gate, complexity| {
            seen.set(Some((
                message.to_string(),
                *index == expected_index,
                complexity,
            )));
            Route {
                step: Step::Labeller {
                    kind: "quick-lookup".to_string(),
                },
                kind: Some("quick-lookup".to_string()),
                pick: service("qwen", "qwen3-coder"),
            }
        },
    )
    .unwrap();
    assert_eq!(
        seen.take(),
        Some(("  look this up  ".to_string(), true, Complexity::BigNoTools))
    );
    assert_eq!(
        serde_json::to_value(got).unwrap(),
        json!({
            "step": "labeller",
            "kind": "quick-lookup",
            "pick": { "to": "service", "service": "qwen", "model": "qwen3-coder" }
        })
    );
}

/// A router that asks the gate about gemini and picks it when allowed.
fn gemini_or_wait(_m: &str, _i: &AbilitiesIndex, gate: &dyn BudgetGate, _c: Complexity) -> Route {
    let pick = if gate.allows("gemini") {
        service("gemini", "gemini-3-flash")
    } else {
        Pick::Wait("gemini is braked".to_string())
    };
    Route {
        step: Step::BudgetRule,
        kind: None,
        pick,
    }
}

#[test]
fn the_router_asks_fleet_through_the_gate() {
    let root = inline_root("gate-allows");
    let fleet = FleetBridge::with_root(&root);
    let got = route_message_with(&fleet, "hello", Complexity::Small, gemini_or_wait).unwrap();
    assert_eq!(
        got.pick,
        ChatPick::Service {
            service: "gemini".to_string(),
            model: "gemini-3-flash".to_string()
        }
    );
    assert_eq!(calls(&root), "actions --json\nbudget allow gemini --json\n");
}

#[test]
fn a_braked_pool_is_the_routers_answer_not_an_error() {
    let root = inline_root("gate-braked");
    let fleet = FleetBridge::with_root(&root).with_env("STUB_BUDGET_EXIT", "75");
    let got = route_message_with(&fleet, "hello", Complexity::Small, gemini_or_wait).unwrap();
    assert_eq!(
        serde_json::to_value(got).unwrap(),
        json!({
            "step": "budget rule",
            "kind": null,
            "pick": { "to": "wait", "reason": "gemini is braked" }
        })
    );
}

#[test]
fn a_fleet_failure_inside_the_gate_is_the_error() {
    let root = inline_root("gate-fails");
    let fleet = FleetBridge::with_root(&root).with_env("STUB_BUDGET_EXIT", "1");
    match route_message_with(&fleet, "hello", Complexity::Small, gemini_or_wait) {
        Err(CoreError::Fleet(err)) => {
            assert_eq!(err.exit_code, Some(1));
            assert!(err.command.contains(&"budget".to_string()), "{err:?}");
            assert!(err.command.contains(&"gemini".to_string()), "{err:?}");
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn a_failed_index_read_is_the_error_and_the_router_never_runs() {
    let fleet = fixture_fleet(Some("1"));
    let called = Cell::new(false);
    let got = route_message_with(&fleet, "hello", Complexity::Small, |_, _, _, _| {
        called.set(true);
        Route {
            step: Step::BudgetRule,
            kind: None,
            pick: service("qwen", "qwen3-coder"),
        }
    });
    match got {
        Err(CoreError::Fleet(err)) => {
            assert_eq!(err.exit_code, Some(1));
            assert!(err.command.contains(&"actions".to_string()), "{err:?}");
        }
        other => panic!("got {other:?}"),
    }
    assert!(!called.get());
}

// --- route_message: the real router (FD-1, #169) ---

#[test]
fn route_message_is_the_real_router_over_the_fleet() {
    // The fixture stub allows every pool, so the fleet gate answers as AllowAll does.
    let fleet = fixture_fleet(None);
    let index = AbilitiesIndex::load_with(&fleet, FleetModels::local_machine().as_deref()).unwrap();
    for message in ["hello there", "search the web for llama-swap"] {
        let got = route_message(&fleet, message, DEFAULT_COMPLEXITY).unwrap();
        let want = router::route(message, &index, &AllowAll, DEFAULT_COMPLEXITY, None);
        assert_eq!(got, ChatRoute::from(&want), "{message}");
    }
}

// --- route_message_on: qwen and the Claude API get their models from config.json (FD-3) ---

/// Answers `actions` with the `actions.json` next to it and exits 0 for `budget`.
const TABLE_FLEET: &str = r#"import pathlib, sys
here = pathlib.Path(__file__).resolve().parent
if sys.argv[1:2] == ["actions"]:
    sys.stdout.write((here / "actions.json").read_text(encoding="utf-8"))
sys.exit(0)
"#;

const STUB_QWEN_MODEL: &str = "qwen3.8-27b";
const STUB_CLAUDEAPI_MODEL: &str = "claude-sonnet-5-5";

/// A stub root with the action table `actions` and a config naming `STUB_QWEN_MODEL` for
/// `strix-halo` and `STUB_CLAUDEAPI_MODEL` as the Sonnet tier.
fn table_root(name: &str, actions: &Value) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sb-fd1w-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("tools").join("fleet");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("fleet.py"), TABLE_FLEET).unwrap();
    std::fs::write(dir.join("actions.json"), actions.to_string()).unwrap();
    let config = json!({
        "claudeapi_models": { "sonnet": STUB_CLAUDEAPI_MODEL },
        "machines": { "strix-halo": { "qwen_model": STUB_QWEN_MODEL } }
    });
    std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
    root
}

fn flagged_table() -> Value {
    json!({
        "stub-web": {
            "label": "Stub: a web lookup",
            "request": true,
            "needs": ["web"],
            "first": ["qwen"],
            "ok": { "qwen": true }
        },
        "stub-big": {
            "label": "Stub: a big request",
            "request": true,
            "needs": [],
            "first": ["claudeapi"],
            "claudeapi": true,
            "ok": { "claude": "sonnet" }
        }
    })
}

#[test]
fn a_qwen_first_kind_routes_to_qwen_with_this_machines_model() {
    let root = table_root("qwen-first", &flagged_table());
    let fleet = FleetBridge::with_root(&root);
    let got = route_message_on(
        &fleet,
        Some("strix-halo"),
        "search the web for llama-swap",
        Complexity::Small,
        |m, i, g, c| router::route(m, i, g, c, None),
    )
    .unwrap();
    assert_eq!(
        got,
        ChatRoute {
            step: "explicit ask web".to_string(),
            kind: Some("stub-web".to_string()),
            pick: ChatPick::Service {
                service: "qwen".to_string(),
                model: STUB_QWEN_MODEL.to_string(),
            },
        }
    );
}

#[test]
fn a_big_no_tools_message_routes_to_the_claude_api_sonnet_model() {
    let root = table_root("big-no-tools", &flagged_table());
    let fleet = FleetBridge::with_root(&root);
    let got = route_message_on(
        &fleet,
        Some("strix-halo"),
        "hello there",
        Complexity::BigNoTools,
        |m, i, g, c| router::route(m, i, g, c, None),
    )
    .unwrap();
    assert_eq!(
        got.pick,
        ChatPick::Service {
            service: "claudeapi".to_string(),
            model: STUB_CLAUDEAPI_MODEL.to_string(),
        }
    );
}

// --- The shell registers the command; the code map says so ---

fn shell(file: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../app/src-tauri/src")
            .join(file),
    )
    .unwrap()
}

#[test]
fn main_registers_the_chat_route_command() {
    let main = shell("main.rs");
    let start = main
        .find("tauri::generate_handler![")
        .expect("main.rs has generate_handler!");
    let end = start + main[start..].find(']').unwrap();
    let handler = &main[start..end];
    assert!(
        handler.contains("commands::chat_route,"),
        "generate_handler! lacks commands::chat_route: {handler}"
    );
}

#[test]
fn the_chat_route_command_is_a_thin_call_into_route_message() {
    let commands = shell("commands.rs");
    let at = commands
        .find("pub fn chat_route(")
        .expect("commands.rs has pub fn chat_route(");
    let before = &commands[..at];
    let attr = before.rfind("#[tauri::command]").expect("an attribute");
    assert!(
        !before[attr..].contains("fn "),
        "#[tauri::command] is not on chat_route"
    );
    let body_end = commands[at..]
        .find("\n}\n")
        .map_or(commands.len(), |i| at + i);
    let body = &commands[at..body_end];
    assert!(body.contains("message: String"), "{body}");
    assert!(body.contains("route_message("), "{body}");
    assert!(body.contains("estimate_complexity(&message)"), "{body}");
    assert!(body.contains("FaceError::new(\"chat_route\""), "{body}");
    assert!(body.contains("record_fleet_error("), "{body}");
}

#[test]
fn the_code_map_marks_the_command_present() {
    let map = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md"),
    )
    .unwrap();
    let section = map
        .split("### frontdoor")
        .nth(1)
        .and_then(|rest| rest.split("\n### ").next())
        .expect("a ### frontdoor section");
    assert!(
        section.contains(
            "- FD-1 chat_route command (`ChatRoute`, `FleetGate`, `route_message`, `estimate_complexity`, `chat_route`): present."
        ),
        "{section}"
    );
}
