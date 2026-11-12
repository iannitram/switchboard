//! #377 (FD-3): with every pool braked the route is a wait, and the ask is queued: its user turn
//! is stored in the thread marked `queued`, the reply carries the thread and says why, a queued
//! ask is not sent to a model as history later, and `queued_asks` lists the ones still waiting.
//! The transport is a fake; no service is called.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use switchboard_core::frontdoor::adapters::ChatTransport;
use switchboard_core::frontdoor::answer::{answer_route, queued_asks, AnswerContext, ChatReply};
use switchboard_core::frontdoor::chat::{ChatPick, ChatRoute};
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::CoreError;

type Seen = Arc<Mutex<Vec<Value>>>;

struct Fake(Seen);

impl ChatTransport for Fake {
    fn post_json(&self, _url: &str, body: &Value) -> Result<Value, CoreError> {
        self.0.lock().unwrap().push(body.clone());
        Ok(json!({"choices": [{"message": {"content": "A monad is a monoid."}}]}))
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

fn data_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-queued-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn wait() -> ChatRoute {
    ChatRoute {
        step: "labeller".to_string(),
        kind: Some("quick-lookup".to_string()),
        pick: ChatPick::Wait {
            reason: "waiting for budget: quick-lookup".to_string(),
        },
    }
}

fn qwen() -> ChatRoute {
    ChatRoute {
        pick: ChatPick::Service {
            service: "qwen".to_string(),
            model: "qwen-small".to_string(),
        },
        ..wait()
    }
}

fn answer(
    dir: &Path,
    route: ChatRoute,
    message: &str,
    id: Option<&str>,
    seen: &Seen,
) -> Result<ChatReply, CoreError> {
    let env = |_: &str| None;
    let seen = seen.clone();
    let transport = move || -> Box<dyn ChatTransport> { Box::new(Fake(seen.clone())) };
    let ctx = AnswerContext {
        data_dir: dir,
        qwen_base_url: Some("http://127.0.0.1:8080"),
        env: &env,
        transport: &transport,
    };
    answer_route(route, message, id, &ctx)
}

#[test]
fn a_wait_queues_the_ask_in_a_new_thread_and_says_so() {
    let dir = data_dir("new");
    let seen = Seen::default();
    let reply = answer(&dir, wait(), "what is a monad", None, &seen).unwrap();
    let id = reply
        .handoff_id
        .clone()
        .expect("the thread that holds the ask");
    assert_eq!(
        reply.queued.as_deref(),
        Some("waiting for budget: quick-lookup")
    );
    assert_eq!(reply.text, None);
    assert!(seen.lock().unwrap().is_empty(), "nothing is sent");
    let rows = ThreadStore::new(&dir).read(&id).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].role.as_deref(), Some("user"));
    assert_eq!(rows[0].text.as_deref(), Some("what is a monad"));
    assert_eq!(rows[0].outcome.as_deref(), Some("queued"));
    assert_eq!(rows[0].kind.as_deref(), Some("quick-lookup"));

    let value = serde_json::to_value(&reply).unwrap();
    assert_eq!(value["pick"]["to"], "wait");
    assert_eq!(value["queued"], "waiting for budget: quick-lookup");
    assert_eq!(value["handoff_id"], id.as_str());

    let queued = queued_asks(&dir).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].0, id);
    assert_eq!(queued[0].1.text.as_deref(), Some("what is a monad"));
}

#[test]
fn a_wait_in_an_open_thread_queues_there_and_is_not_sent_as_history() {
    let dir = data_dir("open");
    let seen = Seen::default();
    answer(&dir, qwen(), "hello", Some("t-1"), &seen).unwrap();
    let reply = answer(&dir, wait(), "what is a monad", Some("t-1"), &seen).unwrap();
    assert_eq!(reply.handoff_id.as_deref(), Some("t-1"));
    assert_eq!(ThreadStore::new(&dir).read("t-1").unwrap().len(), 3);
    assert_eq!(queued_asks(&dir).unwrap().len(), 1);

    // The ask, sent again once a pool is back, goes with the answered turns only.
    answer(&dir, qwen(), "what is a monad", Some("t-1"), &seen).unwrap();
    let sent = seen.lock().unwrap().last().cloned().unwrap();
    let contents: Vec<&str> = sent["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] != "system")
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    assert_eq!(
        contents,
        ["hello", "A monad is a monoid.", "what is a monad"]
    );
    // Answered now: nothing is waiting.
    assert!(queued_asks(&dir).unwrap().is_empty());
}
