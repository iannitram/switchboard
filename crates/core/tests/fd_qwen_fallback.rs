//! #377 (FD-3, owner ruling 2026-11-11): an ask whose picked pool is braked or over budget
//! falls back to the local Qwen lane when that lane is up, and the reply says so plainly; it is
//! queued only when Qwen is down too; a pool in budget is unchanged; and one fallback does not
//! move a sticky chat (FD-5) off its model. #427 (owner rulings 2026-11-11): only a kind with a
//! Qwen row falls back, a wait for Claude's budget included; Qwen failing mid-answer holds the
//! ask for Claude; a thumbs down on a fallback answer holds the same ask, and thumbs on it never
//! teach routing. The route is the real router's over a stub fleet (`budget allow` from
//! `STUB_BRAKED`), the transport a fake: no service is called.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::frontdoor::adapters::ChatTransport;
use switchboard_core::frontdoor::answer::queued_asks;
use switchboard_core::frontdoor::ask::{ask_on, Asked, Seams};
use switchboard_core::frontdoor::chat::{qwen_fallback_on, ChatPick, ChatRoute};
use switchboard_core::frontdoor::labeller::SmallLabeller;
use switchboard_core::frontdoor::rating_log::rate_turn;
use switchboard_core::frontdoor::router::learned_pick;
use switchboard_core::frontdoor::sticky::open_chat;
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::CoreError;

const MACHINE: &str = "strix-halo";
const QWEN_URL: &str = "http://127.0.0.1:9/v1/models";
const ASK: &str = "compare tabs and spaces for me";
const REASON: &str =
    "nothing else is in budget (waiting for budget: general-question), so the local Qwen lane answers";

const FLEET: &str = r#"import os, pathlib, sys
if sys.argv[1:2] == ["actions"]:
    sys.stdout.write((pathlib.Path(__file__).parent / "actions.json").read_text())
    sys.exit(0)
if sys.argv[1:3] == ["budget", "allow"]:
    braked = os.environ.get("STUB_BRAKED", "").split(",")
    sys.exit(75 if sys.argv[3] in braked else 0)
if sys.argv[1:3] == ["budget", "--json"]:
    import json
    braked = os.environ.get("STUB_BRAKED", "").split(",")
    pools = ["claude", "claudeapi", "chatgpt", "deepseek", "gemini", "qwen"]
    sys.stdout.write(json.dumps({p: {"verdict": "brake" if p in braked else "on-pace",
                                     "reading_age_min": 5, "stale": False} for p in pools}))
    sys.exit(0)
sys.exit(2)
"#;

/// Names every message the kind it holds (FD-2's small model). By default a general question:
/// `first` is gemini then deepseek, and in this world it has a Qwen row.
struct Labels(&'static str);

impl SmallLabeller for Labels {
    fn label(&self, _message: &str, _kinds: &[String]) -> Option<String> {
        Some(self.0.to_string())
    }
}

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

/// The fake services; with `qwen_fails` set, the Qwen server fails mid-answer.
struct Fake(Seen, Arc<AtomicBool>);

impl ChatTransport for Fake {
    fn post_json(&self, url: &str, body: &Value) -> Result<Value, CoreError> {
        self.0.lock().unwrap().push((url.to_string(), body.clone()));
        if url.starts_with("http://127.0.0.1:9") {
            if self.1.load(Ordering::SeqCst) {
                return Err(CoreError::Io(
                    "qwen: connection reset mid-answer".to_string(),
                ));
            }
            return Ok(json!({"choices": [{"message": {"content": "qwen says hello"}}]}));
        }
        Ok(json!({"candidates": [{"content": {"parts": [{"text": "gemini says hello"}]}}]}))
    }
    fn post_json_headers(
        &self,
        url: &str,
        _headers: &[(String, String)],
        body: &Value,
    ) -> Result<Value, CoreError> {
        self.post_json(url, body)
    }
}

struct World {
    root: PathBuf,
    seen: Seen,
    probed: Arc<Mutex<Vec<String>>>,
    kind: Cell<&'static str>,
    qwen_fails: Arc<AtomicBool>,
}

impl World {
    fn new(name: &str) -> World {
        let root = std::env::temp_dir().join(format!("sb-fd377-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let tools = root.join("tools").join("fleet");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(tools.join("fleet.py"), FLEET).unwrap();
        let actions = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fleet-stub/answers/actions.json");
        let mut actions: Value =
            serde_json::from_str(&std::fs::read_to_string(actions).unwrap()).unwrap();
        // The fixture's general question with a Qwen row, and two kinds only Claude's
        // subscription answers: one with a Qwen row, one without.
        actions["general-question"]["ok"]["qwen"] = json!(true);
        actions["claude-plan"] = json!({
            "request": true, "first": ["claude"], "needs": [],
            "ok": { "claude": "opus", "qwen": true }
        });
        actions["claude-only"] = json!({
            "request": true, "first": ["claude"], "needs": [],
            "ok": { "claude": "opus", "qwen": false }
        });
        std::fs::write(tools.join("actions.json"), actions.to_string()).unwrap();
        let config = json!({ "machines": { MACHINE: {
            "qwen_capable": true,
            "qwen_model": "qwen3.8-27b",
            "qwen_server_url": QWEN_URL
        } } });
        std::fs::write(tools.join("config.json"), config.to_string()).unwrap();
        World {
            root,
            seen: Seen::default(),
            probed: Arc::default(),
            kind: Cell::new("general-question"),
            qwen_fails: Arc::default(),
        }
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn fleet(&self, braked: &str) -> FleetBridge {
        FleetBridge::with_root(&self.root).with_env("STUB_BRAKED", braked)
    }

    /// One ask with `braked` pools and the Qwen server up or down.
    fn ask(
        &self,
        braked: &str,
        qwen_is_up: bool,
        message: &str,
        thread: Option<&str>,
        dry_run: bool,
    ) -> Asked {
        let env = |name: &str| (name == "GEMINI_API_KEY").then(|| "test-key".to_string());
        let (seen, fails) = (self.seen.clone(), self.qwen_fails.clone());
        let transport =
            move || -> Box<dyn ChatTransport> { Box::new(Fake(seen.clone(), fails.clone())) };
        let probed = self.probed.clone();
        let qwen_up = move |url: &str| {
            probed.lock().unwrap().push(url.to_string());
            qwen_is_up
        };
        let labels = Labels(self.kind.get());
        let seams = Seams {
            machine: Some(MACHINE),
            small: Some(&labels),
            env: &env,
            transport: &transport,
            qwen_up: &qwen_up,
        };
        ask_on(
            &self.fleet(braked),
            &self.data(),
            message,
            thread,
            dry_run,
            &seams,
        )
        .unwrap()
    }

    fn urls(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.0.clone())
            .collect()
    }
}

fn qwen() -> ChatPick {
    ChatPick::Service {
        service: "qwen".to_string(),
        model: "qwen3.8-27b".to_string(),
    }
}

#[test]
fn a_braked_pool_with_qwen_up_goes_to_qwen_and_says_why() {
    let w = World::new("up");
    let asked = w.ask("gemini,deepseek", true, ASK, None, false);
    let reply = asked.reply;
    assert_eq!(reply.route.pick, qwen());
    assert_eq!(reply.route.kind.as_deref(), Some("general-question"));
    assert_eq!(reply.route.step, "labeller");
    assert_eq!(reply.fallback.as_deref(), Some(REASON));
    assert_eq!(reply.text.as_deref(), Some("qwen says hello"));
    assert_eq!(reply.service.as_deref(), Some("qwen"));
    assert_eq!(reply.queued, None);
    assert_eq!(reply.moved_from, None);
    assert_eq!(w.urls(), ["http://127.0.0.1:9/v1/chat/completions"]);
    assert_eq!(*w.probed.lock().unwrap(), [QWEN_URL]);

    // Both turns are stored; the answer's row says it was a fallback, and nothing is queued.
    let id = reply.handoff_id.unwrap();
    let rows = ThreadStore::new(&w.data()).read(&id).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].outcome, None);
    assert_eq!(rows[1].lane.as_deref(), Some("qwen"));
    assert_eq!(rows[1].fallback.as_deref(), Some(REASON));
    assert!(queued_asks(&w.data()).unwrap().is_empty());

    // The reason is on the wire for every face (the window, `--json`, MCP).
    let wire = serde_json::to_value(w.ask("gemini,deepseek", true, ASK, None, true)).unwrap();
    assert_eq!(
        wire,
        json!({
            "step": "labeller",
            "kind": "general-question",
            "pick": { "to": "service", "service": "qwen", "model": "qwen3.8-27b" },
            "fallback": REASON,
            "dry_run": true
        })
    );
    assert_eq!(w.urls().len(), 1, "a dry run sends nothing");
}

#[test]
fn a_braked_pool_with_qwen_down_is_queued() {
    let w = World::new("down");
    let reply = w.ask("gemini,deepseek", false, ASK, None, false).reply;
    assert_eq!(
        reply.route.pick,
        ChatPick::Wait {
            reason: "waiting for budget: general-question".to_string()
        }
    );
    assert_eq!(
        reply.queued.as_deref(),
        Some("waiting for budget: general-question")
    );
    assert_eq!(reply.fallback, None);
    assert_eq!(reply.text, None);
    assert!(w.urls().is_empty(), "nothing is sent");
    let queued = queued_asks(&w.data()).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].1.text.as_deref(), Some(ASK));

    // Qwen's own pool braked is Qwen down: its server is not even asked.
    let w = World::new("qwen-braked");
    let reply = w.ask("gemini,deepseek,qwen", true, ASK, None, false).reply;
    assert!(reply.queued.is_some());
    assert_eq!(reply.fallback, None);
    assert!(w.probed.lock().unwrap().is_empty());
    assert!(w.urls().is_empty());
}

#[test]
fn a_pool_in_budget_is_unchanged() {
    let w = World::new("fine");
    let reply = w.ask("", true, ASK, None, false).reply;
    assert_eq!(
        reply.route.pick,
        ChatPick::Service {
            service: "gemini".to_string(),
            model: "gemini-3.8-flash-medium".to_string()
        }
    );
    assert_eq!(reply.fallback, None);
    assert_eq!(reply.text.as_deref(), Some("gemini says hello"));
    assert!(
        w.probed.lock().unwrap().is_empty(),
        "Qwen is not asked about"
    );
    let id = reply.handoff_id.unwrap();
    let rows = ThreadStore::new(&w.data()).read(&id).unwrap();
    assert_eq!(rows[1].fallback, None);
    let wire = serde_json::to_value(&rows[1]).unwrap();
    assert!(wire.get("fallback").is_none(), "{wire}");
}

#[test]
fn one_fallback_does_not_move_a_sticky_chat() {
    let w = World::new("sticky");
    let first = w.ask("", true, ASK, None, false).reply;
    let id = first.handoff_id.unwrap();
    assert_eq!(first.service.as_deref(), Some("gemini"));

    // Every paid and resetting pool braked: Qwen answers this one turn.
    let second = w
        .ask("gemini,deepseek", true, "and for yaml", Some(&id), false)
        .reply;
    assert_eq!(second.route.pick, qwen());
    assert_eq!(second.fallback.as_deref(), Some(REASON));
    assert_eq!(second.text.as_deref(), Some("qwen says hello"));
    assert_eq!(second.moved_from, None, "a fallback is not a move");
    let store = ThreadStore::new(&w.data());
    let rows = store.read(&id).unwrap();
    assert!(
        rows.iter()
            .all(|r| !matches!(r.role.as_deref(), Some("move" | "summary"))),
        "no move and no summary is stored"
    );
    // Qwen was sent the thread's turns, as any turn in a chat is: no packet, no summary ask.
    let calls = w.seen.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert!(
        calls[1].0.starts_with("http://127.0.0.1:9"),
        "{}",
        calls[1].0
    );
    let contents: Vec<&str> = calls[1].1["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] != "system")
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    assert_eq!(contents, [ASK, "gemini says hello", "and for yaml"]);

    // The chat is still open on gemini, and with its pool back the next turn stays there.
    let open = open_chat(&store, &id).unwrap().unwrap();
    assert_eq!((open.service.as_str(), open.turn), ("gemini", 1));
    let third = w.ask("", true, "and for json", Some(&id), false).reply;
    assert_eq!(third.route.step, "sticky chat");
    assert_eq!(third.service.as_deref(), Some("gemini"));
    assert_eq!(third.fallback, None);
    assert_eq!(third.moved_from, None);
    assert_eq!(store.turns(&id).unwrap().len(), 6);
}

#[test]
fn a_new_chat_qwen_answered_as_a_fallback_is_open_on_no_model() {
    let w = World::new("new");
    let first = w.ask("gemini,deepseek", true, ASK, None, false).reply;
    let id = first.handoff_id.unwrap();
    assert_eq!(open_chat(&ThreadStore::new(&w.data()), &id).unwrap(), None);
    // With the pools back the next message is routed as a new chat's is: to gemini, no move.
    let next = w.ask("", true, "and for yaml", Some(&id), false).reply;
    assert_eq!(next.route.step, "labeller");
    assert_eq!(next.service.as_deref(), Some("gemini"));
    assert_eq!(next.moved_from, None);
}

fn waiting(reason: &str) -> ChatRoute {
    ChatRoute {
        step: "budget rule".to_string(),
        kind: None,
        pick: ChatPick::Wait {
            reason: reason.to_string(),
        },
    }
}

fn waiting_for(kind: &str, reason: &str) -> ChatRoute {
    ChatRoute {
        kind: Some(kind.to_string()),
        ..waiting(reason)
    }
}

#[test]
fn only_a_kind_with_a_qwen_row_falls_back() {
    let w = World::new("which");
    let up = |_: &str| true;
    let fallback = |route: &ChatRoute, machine: Option<&str>| {
        qwen_fallback_on(&w.fleet(""), machine, route, &up).unwrap()
    };
    let general = waiting_for("general-question", "waiting for budget: general-question");
    let taken = fallback(&general, Some(MACHINE)).unwrap();
    assert_eq!(taken.route.pick, qwen());
    assert_eq!(taken.route.step, "budget rule");
    assert_eq!(taken.waited, general, "the wait is kept for a hold");
    assert_eq!(taken.reason, REASON);
    // #427: a wait for Claude's budget falls back like a braked pool, for a kind with a Qwen row.
    let claude = waiting_for("claude-plan", "waiting for Claude's budget");
    assert_eq!(
        fallback(&claude, Some(MACHINE)).unwrap().reason,
        "nothing else is in budget (waiting for Claude's budget), so the local Qwen lane answers"
    );
    // A kind with no Qwen row, an unknown kind and a route with no kind are queued for Claude.
    for route in [
        waiting_for("claude-only", "waiting for Claude's budget"),
        waiting_for("writing-editing", "waiting for budget: writing-editing"),
        waiting_for("nope", "unknown request kind nope"),
        waiting("waiting for budget: claudeapi"),
        waiting("waiting for Claude's budget"),
    ] {
        assert_eq!(fallback(&route, Some(MACHINE)), None, "{route:?}");
    }
    // A route that is not a wait stands.
    let handoff = ChatRoute {
        pick: ChatPick::ClaudeHandoff {
            reason: "ask claude".to_string(),
            model: None,
        },
        ..general.clone()
    };
    assert_eq!(fallback(&handoff, Some(MACHINE)), None);
    // A machine with no Qwen model in the fleet config has no Qwen lane.
    assert_eq!(fallback(&general, Some("other")), None);
    assert_eq!(fallback(&general, None), None);
}

const HELD: &str =
    "Qwen failed mid-answer, so the ask is held for Claude (waiting for budget: general-question)";

#[test]
fn qwen_failing_mid_answer_holds_the_ask_for_claude() {
    let w = World::new("fails");
    w.qwen_fails.store(true, Ordering::SeqCst);
    let reply = w.ask("gemini,deepseek", true, ASK, None, false).reply;
    // As if Qwen were down: the wait is the route, the ask is queued, and the line says why.
    assert_eq!(
        reply.route.pick,
        ChatPick::Wait {
            reason: "waiting for budget: general-question".to_string()
        }
    );
    assert_eq!(reply.queued.as_deref(), Some(HELD));
    assert_eq!(reply.fallback, None);
    assert_eq!(reply.text, None, "no partial answer is kept");
    assert_eq!(reply.service, None);
    assert_eq!(w.urls(), ["http://127.0.0.1:9/v1/chat/completions"]);
    let id = reply.handoff_id.unwrap();
    let rows = ThreadStore::new(&w.data()).read(&id).unwrap();
    assert_eq!(rows.len(), 1, "the ask alone: {rows:?}");
    assert_eq!(rows[0].role.as_deref(), Some("user"));
    assert_eq!(rows[0].outcome.as_deref(), Some("queued"));
    let queued = queued_asks(&w.data()).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].0, id);
    assert_eq!(queued[0].1.text.as_deref(), Some(ASK));
    // The line is on the wire for every face.
    let wire = serde_json::to_value(w.ask("gemini,deepseek", true, ASK, Some(&id), false)).unwrap();
    assert_eq!(wire["queued"], HELD);
    // Sending stays manual: nothing more was sent than the two failed tries.
    assert_eq!(w.urls().len(), 2);
}

#[test]
fn waiting_for_claudes_budget_falls_back_for_a_kind_with_a_qwen_row() {
    let w = World::new("claude-wait");
    w.kind.set("claude-plan");
    let reply = w.ask("claude", true, "plan the release", None, false).reply;
    assert_eq!(reply.route.pick, qwen());
    assert_eq!(
        reply.fallback.as_deref(),
        Some("nothing else is in budget (waiting for Claude's budget), so the local Qwen lane answers")
    );
    assert_eq!(reply.text.as_deref(), Some("qwen says hello"));
}

#[test]
fn a_kind_with_no_qwen_row_or_no_kind_is_queued_for_claude() {
    for (kind, braked, message, reason) in [
        (
            "claude-only",
            "claude",
            "plan the release",
            "waiting for Claude's budget",
        ),
        (
            "writing-editing",
            "gemini,claudeapi",
            "edit my paragraph",
            "waiting for budget: writing-editing",
        ),
        // An unknown kind: the labeller's answer is not a kind, so the budget rule waits.
        ("nope", "claudeapi", ASK, "waiting for budget: claudeapi"),
    ] {
        let w = World::new(&format!("no-row-{kind}"));
        w.kind.set(kind);
        let reply = w.ask(braked, true, message, None, false).reply;
        assert_eq!(reply.queued.as_deref(), Some(reason), "{kind}");
        assert_eq!(reply.fallback, None, "{kind}");
        assert!(w.urls().is_empty(), "{kind}: nothing is sent");
        assert!(
            w.probed.lock().unwrap().is_empty(),
            "{kind}: Qwen is not asked"
        );
        assert_eq!(queued_asks(&w.data()).unwrap().len(), 1, "{kind}");
    }
}

#[test]
fn a_thumbs_down_on_a_fallback_answer_holds_the_same_ask_for_claude() {
    let w = World::new("down-on-fallback");
    let reply = w.ask("gemini,deepseek", true, ASK, None, false).reply;
    let id = reply.handoff_id.unwrap();
    let store = ThreadStore::new(&w.data());

    let row = rate_turn(&w.data(), &id, 1, "down").unwrap();
    assert_eq!(row.rating.as_deref(), Some("down"));
    assert_eq!(
        row.fallback.as_deref(),
        Some(REASON),
        "the rating says it rated a fallback"
    );
    let queued = queued_asks(&w.data()).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].0, id);
    assert_eq!(queued[0].1.text.as_deref(), Some(ASK));
    assert_eq!(queued[0].1.kind.as_deref(), Some("general-question"));
    // The turns: the ask, Qwen's answer, the same ask held.
    assert_eq!(store.turns(&id).unwrap().len(), 3);
    // A second press is one hold, and the fallback still moves nothing.
    rate_turn(&w.data(), &id, 1, "down").unwrap();
    assert_eq!(store.turns(&id).unwrap().len(), 3);
    assert_eq!(open_chat(&store, &id).unwrap(), None);
    assert!(store
        .read(&id)
        .unwrap()
        .iter()
        .all(|r| r.role.as_deref() != Some("move") && r.moved.is_none()));
    assert_eq!(w.urls().len(), 1, "sending the held ask stays manual");

    // A thumbs up on a fallback answer holds nothing.
    let w = World::new("up-on-fallback");
    let id = w
        .ask("gemini,deepseek", true, ASK, None, false)
        .reply
        .handoff_id
        .unwrap();
    let row = rate_turn(&w.data(), &id, 1, "up").unwrap();
    assert_eq!(row.fallback.as_deref(), Some(REASON));
    assert!(queued_asks(&w.data()).unwrap().is_empty());
}

#[test]
fn thumbs_on_fallback_answers_never_teach_routing() {
    let w = World::new("teach");
    let mut rows = Vec::new();
    for turn in 0..6 {
        let id = w
            .ask("gemini,deepseek", true, ASK, None, false)
            .reply
            .handoff_id
            .unwrap();
        rate_turn(&w.data(), &id, 1, "up").unwrap();
        let _ = turn;
        rows.extend(ThreadStore::new(&w.data()).read(&id).unwrap());
    }
    // Six thumbs up on Qwen for the kind: enough for FD-4, but every one rated a fallback.
    assert_eq!(rows.iter().filter(|r| r.rating.is_some()).count(), 6);
    assert_eq!(learned_pick("general-question", &rows), None);
    // The same rows as ordinary answers would teach it.
    let plain: Vec<_> = rows
        .into_iter()
        .map(|mut r| {
            r.fallback = None;
            r
        })
        .collect();
    assert_eq!(
        learned_pick("general-question", &plain),
        Some(("qwen".to_string(), "qwen3.8-27b".to_string()))
    );
}

// --- #428: a Claude handoff carries the model it goes to ---

#[test]
fn a_claude_handoff_route_names_the_kinds_claude_tier() {
    let w = World::new("claude-tier");
    w.kind.set("claude-plan");
    let asked = w.ask("", true, "plan the quarter", None, true);
    assert_eq!(
        asked.reply.route.pick,
        ChatPick::ClaudeHandoff {
            reason: "claude-plan: the Claude subscription is the kind's pick and under budget"
                .to_string(),
            model: Some("opus".to_string()),
        }
    );
}
