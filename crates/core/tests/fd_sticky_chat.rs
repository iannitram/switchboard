//! #378 (FD-5, FD-6, FD-7, FD-10): a chat open on a model stays on it until I say "ask Claude",
//! name an agent, give a thumbs down, or the model writes `[[handoff: <reason>]]`. A move sends
//! the incoming model the packet (the rolling summary the outgoing model rewrote, checked for
//! links, the last turns and the links), never the full transcript; a thumbs down that moved
//! the chat is marked `moved`. The fleet is a stub that answers `actions` with `{}` and
//! `budget allow` from `STUB_BRAKED`; the chat transport is a fake. No service is called.

use serde_json::{json, Value};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use switchboard_core::abilities::AbilitiesIndex;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::frontdoor::adapters::ChatTransport;
use switchboard_core::frontdoor::answer::{answer_route, AnswerContext, ChatReply};
use switchboard_core::frontdoor::chat::{route_in_thread_on, ChatPick, ChatRoute};
use switchboard_core::frontdoor::rating_log::log_rating;
use switchboard_core::frontdoor::router::{BudgetGate, Complexity, Pick, Route, Step};
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::CoreError;

const FLEET: &str = r#"import os, sys
if sys.argv[1:2] == ["actions"]:
    sys.stdout.write("{}")
    sys.exit(0)
if sys.argv[1:3] == ["budget", "allow"]:
    braked = os.environ.get("STUB_BRAKED", "").split(",")
    sys.exit(75 if sys.argv[3] in braked else 0)
sys.exit(2)
"#;

fn root(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-sticky-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("tools").join("fleet")).unwrap();
    std::fs::write(d.join("tools").join("fleet").join("fleet.py"), FLEET).unwrap();
    d
}

fn fleet(root: &Path, braked: &str) -> FleetBridge {
    FleetBridge::with_root(root).with_env("STUB_BRAKED", braked)
}

// --- the fake chat services ---

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

/// Answers every call with the next of `replies` (the last one again when they run out).
struct Fake {
    seen: Seen,
    replies: Arc<Mutex<Vec<String>>>,
}

impl ChatTransport for Fake {
    fn post_json(&self, url: &str, body: &Value) -> Result<Value, CoreError> {
        self.seen
            .lock()
            .unwrap()
            .push((url.to_string(), body.clone()));
        let mut replies = self.replies.lock().unwrap();
        let text = if replies.len() > 1 {
            replies.remove(0)
        } else {
            replies[0].clone()
        };
        Ok(json!({"choices": [{"message": {"content": text}}]}))
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

struct Chat {
    data: PathBuf,
    seen: Seen,
    replies: Arc<Mutex<Vec<String>>>,
}

impl Chat {
    fn new(data: &Path) -> Self {
        Chat {
            data: data.to_path_buf(),
            seen: Seen::default(),
            replies: Arc::new(Mutex::new(vec!["ok".to_string()])),
        }
    }

    fn replies(&self, replies: &[&str]) {
        *self.replies.lock().unwrap() = replies.iter().map(|r| r.to_string()).collect();
    }

    fn send(&self, route: ChatRoute, message: &str, id: Option<&str>) -> ChatReply {
        let env = |name: &str| (name == "DEEPSEEK_API_KEY").then(|| "test-key".to_string());
        let (seen, replies) = (self.seen.clone(), self.replies.clone());
        let transport = move || -> Box<dyn ChatTransport> {
            Box::new(Fake {
                seen: seen.clone(),
                replies: replies.clone(),
            })
        };
        let ctx = AnswerContext {
            data_dir: &self.data,
            qwen_base_url: Some("http://127.0.0.1:8080"),
            env: &env,
            transport: &transport,
        };
        answer_route(route, message, id, &ctx).unwrap()
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.seen.lock().unwrap().clone()
    }
}

fn to(service: &str, model: &str) -> ChatRoute {
    ChatRoute {
        step: "labeller".to_string(),
        kind: Some("quick-lookup".to_string()),
        pick: ChatPick::Service {
            service: service.to_string(),
            model: model.to_string(),
        },
    }
}

fn qwen() -> ChatRoute {
    to("qwen", "qwen-small")
}

/// The system prompt and the non-system contents of one sent request.
fn sent(body: &Value) -> (String, Vec<String>) {
    let messages = body["messages"].as_array().unwrap();
    let system = messages
        .iter()
        .find(|m| m["role"] == "system")
        .map(|m| m["content"].as_str().unwrap().to_string())
        .unwrap_or_default();
    let rest = messages
        .iter()
        .filter(|m| m["role"] != "system")
        .map(|m| m["content"].as_str().unwrap().to_string())
        .collect();
    (system, rest)
}

/// A thread `t` that qwen answered once: "what is a monad".
fn opened(data: &Path, chat: &Chat) {
    chat.replies(&["A monad is a monoid."]);
    chat.send(qwen(), "what is a monad", Some("t"));
    assert!(data.join("threads").join("t.jsonl").exists());
}

// --- routing (FD-5) ---

/// Routes in thread `t`; answers the route and whether the router was asked.
fn route_in(
    root: &Path,
    braked: &str,
    data: &Path,
    id: Option<&str>,
    message: &str,
    picked: Pick,
) -> (ChatRoute, bool, Vec<bool>) {
    let asked = Cell::new(false);
    let qwen_allowed = std::cell::RefCell::new(Vec::new());
    let route = route_in_thread_on(
        &fleet(root, braked),
        None,
        data,
        id,
        message,
        Complexity::BigWithTools,
        |_: &str, _: &AbilitiesIndex, gate: &dyn BudgetGate, _: Complexity| {
            asked.set(true);
            qwen_allowed.borrow_mut().push(gate.allows("qwen"));
            Route {
                step: Step::BudgetRule,
                kind: None,
                pick: picked.clone(),
            }
        },
    )
    .unwrap();
    let allowed = qwen_allowed.into_inner();
    (route, asked.get(), allowed)
}

fn handoff() -> Pick {
    Pick::ClaudeHandoff("big with tools: the Claude subscription".to_string())
}

const FOLLOW_UP: &str =
    "design a plan step by step for migrating our database and refactor the architecture";

#[test]
fn a_follow_up_in_an_open_chat_stays_on_its_model() {
    let root = root("follow-up");
    let data = root.join("data");
    let chat = Chat::new(&data);
    opened(&data, &chat);
    let (route, asked, _) = route_in(&root, "", &data, Some("t"), FOLLOW_UP, handoff());
    assert!(!asked, "the router is not asked while the chat is sticky");
    assert_eq!(route.step, "sticky chat");
    assert_eq!(route.kind.as_deref(), Some("quick-lookup"));
    assert_eq!(
        route.pick,
        ChatPick::Service {
            service: "qwen".to_string(),
            model: "qwen-small".to_string()
        }
    );
    // And the turn is answered in the thread, on qwen, with the earlier turns.
    chat.replies(&["Here is a plan."]);
    let reply = chat.send(route, FOLLOW_UP, Some("t"));
    assert_eq!(reply.moved_from, None);
    assert_eq!(ThreadStore::new(&data).turns("t").unwrap().len(), 4);
    let (_, contents) = sent(&chat.calls().last().unwrap().1);
    assert_eq!(
        contents,
        ["what is a monad", "A monad is a monoid.", FOLLOW_UP]
    );
}

#[test]
fn a_new_chat_or_one_with_no_answer_is_routed() {
    let root = root("new");
    let data = root.join("data");
    let (_, asked, _) = route_in(&root, "", &data, None, FOLLOW_UP, handoff());
    assert!(asked);
    let (route, asked, _) = route_in(&root, "", &data, Some("empty"), FOLLOW_UP, handoff());
    assert!(asked);
    assert_eq!(route.step, "budget rule");
}

#[test]
fn ask_claude_moves_the_chat() {
    let root = root("ask");
    let data = root.join("data");
    opened(&data, &Chat::new(&data));
    let (route, asked, qwen_allowed) = route_in(
        &root,
        "",
        &data,
        Some("t"),
        "please ask Claude about it",
        handoff(),
    );
    assert!(asked);
    assert!(matches!(route.pick, ChatPick::ClaudeHandoff { .. }));
    // Not a thumbs down or a marker: the open model is not shut out.
    assert_eq!(qwen_allowed, [true]);
}

#[test]
fn naming_an_agent_moves_the_chat() {
    let root = root("agent");
    let data = root.join("data");
    opened(&data, &Chat::new(&data));
    let (_, asked, _) = route_in(
        &root,
        "",
        &data,
        Some("t"),
        "what would Harmon say",
        handoff(),
    );
    assert!(asked);
}

#[test]
fn a_thumbs_down_moves_the_chat_off_its_model() {
    let root = root("down");
    let data = root.join("data");
    opened(&data, &Chat::new(&data));
    log_rating(&data, "t", 1, "down", "qwen", "qwen-small", None).unwrap();
    let (_, asked, qwen_allowed) = route_in(
        &root,
        "",
        &data,
        Some("t"),
        "and what is a functor",
        handoff(),
    );
    assert!(asked);
    assert_eq!(
        qwen_allowed,
        [false],
        "the open model may not take the turn"
    );
}

#[test]
fn a_thumbs_up_does_not_move_it() {
    let root = root("up");
    let data = root.join("data");
    opened(&data, &Chat::new(&data));
    log_rating(&data, "t", 1, "up", "qwen", "qwen-small", None).unwrap();
    let (route, asked, _) = route_in(&root, "", &data, Some("t"), "and a functor", handoff());
    assert!(!asked);
    assert_eq!(route.step, "sticky chat");
}

#[test]
fn the_models_handoff_marker_moves_the_chat_off_it() {
    let root = root("marker");
    let data = root.join("data");
    let chat = Chat::new(&data);
    chat.replies(&["I cannot run code. [[handoff: needs tools]]"]);
    let reply = chat.send(qwen(), "run my tests", Some("t"));
    assert_eq!(reply.handoff.as_deref(), Some("needs tools"));
    let (_, asked, qwen_allowed) = route_in(&root, "", &data, Some("t"), "go on", handoff());
    assert!(asked);
    assert_eq!(qwen_allowed, [false]);
}

#[test]
fn a_braked_pool_is_routed_again() {
    let root = root("braked");
    let data = root.join("data");
    opened(&data, &Chat::new(&data));
    let (_, asked, _) = route_in(&root, "qwen", &data, Some("t"), "and a functor", handoff());
    assert!(asked);
}

// --- the move (FD-6, FD-7, FD-10) ---

#[test]
fn a_move_sends_the_packet_not_the_transcript() {
    let root = root("move");
    let data = root.join("data");
    let chat = Chat::new(&data);
    opened(&data, &chat);
    chat.replies(&["See https://example.invalid/notes.md for more."]);
    chat.send(qwen(), "where can I read more", Some("t"));
    log_rating(&data, "t", 3, "down", "qwen", "qwen-small", None).unwrap();
    let before = chat.calls().len();

    // qwen writes the summary (naming the link), then deepseek answers.
    chat.replies(&[
        "Monads; notes at https://example.invalid/notes.md",
        "A functor maps.",
    ]);
    let reply = chat.send(
        to("deepseek", "deepseek-chat"),
        "what is a functor",
        Some("t"),
    );
    assert_eq!(reply.text.as_deref(), Some("A functor maps."));
    assert_eq!(reply.moved_from.as_deref(), Some("qwen qwen-small"));
    let calls = chat.calls()[before..].to_vec();
    assert_eq!(calls.len(), 2, "one summary ask, one answer: {calls:?}");
    assert!(
        calls[0].0.starts_with("http://127.0.0.1:8080"),
        "qwen wrote it"
    );
    assert!(calls[1].0.contains("deepseek"), "{}", calls[1].0);
    let (system, contents) = sent(&calls[1].1);
    assert_eq!(contents, ["what is a functor"], "never the full transcript");
    assert!(system.contains("Summary\nMonads; notes at https://example.invalid/notes.md"));
    assert!(system.contains("Last turns\nuser: what is a monad\nassistant: A monad is a monoid."));
    assert!(system.contains("Links\nhttps://example.invalid/notes.md"));

    // The summary and the move are stored, the down rating is marked moved.
    let rows = ThreadStore::new(&data).read("t").unwrap();
    let summary: Vec<_> = rows
        .iter()
        .filter(|r| r.role.as_deref() == Some("summary"))
        .collect();
    assert_eq!(summary.len(), 1);
    assert_eq!(
        summary[0].text.as_deref(),
        Some("Monads; notes at https://example.invalid/notes.md")
    );
    let rating = rows
        .iter()
        .find(|r| r.id.as_deref() == Some("t#rating-3"))
        .unwrap();
    assert_eq!(rating.rating.as_deref(), Some("down"));
    assert_eq!(rating.moved, Some(true));
    // Turn indexes are unchanged by the summary and move rows: turn 5 is deepseek's.
    let turns = ThreadStore::new(&data).turns("t").unwrap();
    assert_eq!(turns.len(), 6);
    assert_eq!(turns[5].lane.as_deref(), Some("deepseek"));

    // The next turn stays on deepseek with the packet and the turns since the move only.
    let (route, asked, _) = route_in(&root, "", &data, Some("t"), "and a monoid", handoff());
    assert!(!asked);
    chat.replies(&["A monoid combines."]);
    chat.send(route, "and a monoid", Some("t"));
    let (system, contents) = sent(&chat.calls().last().unwrap().1);
    assert!(system.contains("Summary\nMonads"), "{system}");
    assert_eq!(
        contents,
        ["what is a functor", "A functor maps.", "and a monoid"]
    );
}

#[test]
fn a_summary_that_drops_a_link_is_asked_for_once_more_and_never_stacked() {
    let root = root("recheck");
    let data = root.join("data");
    let chat = Chat::new(&data);
    chat.replies(&["Read file:///srv/a.txt first."]);
    chat.send(qwen(), "what should I read", Some("t"));
    chat.replies(&[
        "Reading advice.",
        "Reading advice: file:///srv/a.txt",
        "Moved.",
    ]);
    chat.send(to("deepseek", "deepseek-chat"), "thanks", Some("t"));
    let calls = chat.calls();
    assert_eq!(
        calls.len(),
        4,
        "the answer, two summary asks, the new answer"
    );
    let (_, again) = sent(&calls[2].1);
    assert!(
        again.last().unwrap().contains("file:///srv/a.txt"),
        "{again:?}"
    );

    // A second move rewrites the one summary row.
    chat.replies(&["Newer summary file:///srv/a.txt", "Back."]);
    chat.send(qwen(), "back to you", Some("t"));
    let rows = ThreadStore::new(&data).read("t").unwrap();
    let summaries: Vec<_> = rows
        .iter()
        .filter(|r| r.role.as_deref() == Some("summary"))
        .map(|r| r.text.clone().unwrap())
        .collect();
    assert_eq!(summaries, ["Newer summary file:///srv/a.txt"]);
}

#[test]
fn a_claude_handoff_in_an_open_chat_carries_the_packet() {
    let root = root("claude");
    let data = root.join("data");
    let chat = Chat::new(&data);
    opened(&data, &chat);
    chat.replies(&["We talked about monads."]);
    let route = ChatRoute {
        step: "explicit ask claude".to_string(),
        kind: None,
        pick: ChatPick::ClaudeHandoff {
            reason: "ask claude".to_string(),
        },
    };
    let reply = chat.send(route, "ask Claude to refactor it", Some("t"));
    assert_eq!(reply.handoff_id.as_deref(), Some("t"));
    assert_eq!(reply.moved_from.as_deref(), Some("qwen qwen-small"));
    let value = serde_json::to_value(&reply).unwrap();
    let packet = reply.packet.expect("the packet for Open in Claude");
    assert_eq!(value["packet"], packet.as_str());
    assert_eq!(value["moved_from"], "qwen qwen-small");
    assert!(
        packet.starts_with("Summary\nWe talked about monads."),
        "{packet}"
    );
    assert!(packet.contains("user: what is a monad"));
    assert!(
        packet.ends_with("Ask\nask Claude to refactor it"),
        "{packet}"
    );
    // Only the summary was asked of a model; no turn is stored for Claude.
    assert_eq!(chat.calls().len(), 2);
    assert_eq!(ThreadStore::new(&data).turns("t").unwrap().len(), 2);
}
