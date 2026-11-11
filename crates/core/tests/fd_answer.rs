//! Tests for FD-11 / FD-13's `answer_route`: a routed turn is sent through the pick's adapter and
//! both turns land in the thread. The transport is a fake and the key lookup is a closure; no
//! service is called and the process environment is not read.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use switchboard_core::frontdoor::adapters::ChatTransport;
use switchboard_core::frontdoor::answer::{
    answer_route, key_var, qwen_base_from_config, AnswerContext,
};
use switchboard_core::frontdoor::chat::{ChatPick, ChatRoute};
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::CoreError;

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

struct Fake {
    seen: Seen,
}

impl Fake {
    fn record(&self, url: &str, body: &Value) -> Result<Value, CoreError> {
        self.seen
            .lock()
            .unwrap()
            .push((url.to_string(), body.clone()));
        Ok(json!({
            "choices": [{"message": {"content": "Use a worktree."}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 4}
        }))
    }
}

impl ChatTransport for Fake {
    fn post_json(&self, url: &str, body: &Value) -> Result<Value, CoreError> {
        self.record(url, body)
    }

    fn post_json_headers(
        &self,
        url: &str,
        _headers: &[(String, String)],
        body: &Value,
    ) -> Result<Value, CoreError> {
        self.record(url, body)
    }
}

fn data_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-answer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn route(pick: ChatPick) -> ChatRoute {
    ChatRoute {
        step: "labeller".to_string(),
        kind: Some("quick-lookup".to_string()),
        pick,
    }
}

fn service(name: &str, model: &str) -> ChatPick {
    ChatPick::Service {
        service: name.to_string(),
        model: model.to_string(),
    }
}

/// Run `answer_route` with the given keys; return the result and what the fake transport saw.
fn run(
    dir: &PathBuf,
    pick: ChatPick,
    keys: &[(&str, &str)],
    handoff_id: Option<&str>,
) -> (
    Result<switchboard_core::frontdoor::answer::ChatReply, CoreError>,
    Seen,
) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let keys: Vec<(String, String)> = keys
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let env = move |name: &str| keys.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let for_transport = seen.clone();
    let transport = move || -> Box<dyn ChatTransport> {
        Box::new(Fake {
            seen: for_transport.clone(),
        })
    };
    let ctx = AnswerContext {
        data_dir: dir,
        qwen_base_url: Some("http://127.0.0.1:8080"),
        env: &env,
        transport: &transport,
    };
    let reply = answer_route(route(pick), "How do I branch?", handoff_id, &ctx);
    (reply, seen)
}

#[test]
fn local_pick_answers_and_both_turns_land_in_the_thread() {
    let dir = data_dir("local");
    let (reply, seen) = run(&dir, service("qwen", "qwen-small"), &[], Some("t-1"));
    let reply = reply.unwrap();
    assert_eq!(reply.text.as_deref(), Some("Use a worktree."));
    assert_eq!(reply.service.as_deref(), Some("qwen"));
    assert_eq!(reply.model.as_deref(), Some("qwen-small"));
    assert_eq!(reply.handoff_id.as_deref(), Some("t-1"));
    assert_eq!(reply.route.step, "labeller");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "http://127.0.0.1:8080/v1/chat/completions");

    let rows = ThreadStore::new(&dir).read("t-1").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].role.as_deref(), Some("user"));
    assert_eq!(rows[0].text.as_deref(), Some("How do I branch?"));
    assert_eq!(rows[1].role.as_deref(), Some("assistant"));
    assert_eq!(rows[1].text.as_deref(), Some("Use a worktree."));
    assert_eq!(rows[1].lane.as_deref(), Some("qwen"));
    assert_eq!(rows[1].model.as_deref(), Some("qwen-small"));
    assert_eq!(rows[1].tokens, Some(14));
    assert_eq!(rows[1].cost_usd, Some(0.0));
    assert_eq!(rows[1].kind.as_deref(), Some("quick-lookup"));
    assert_eq!(rows[1].handoff_id.as_deref(), Some("t-1"));
}

#[test]
fn a_follow_up_sends_the_earlier_turns() {
    let dir = data_dir("follow");
    run(&dir, service("qwen", "qwen-small"), &[], Some("t-2"))
        .0
        .unwrap();
    let (reply, seen) = run(&dir, service("qwen", "qwen-small"), &[], Some("t-2"));
    reply.unwrap();
    let seen = seen.lock().unwrap();
    // system message, then user, assistant, user.
    assert_eq!(seen[0].1["messages"].as_array().unwrap().len(), 4);
    assert_eq!(ThreadStore::new(&dir).read("t-2").unwrap().len(), 4);
}

#[test]
fn no_handoff_id_starts_a_new_thread() {
    let dir = data_dir("newid");
    let reply = run(&dir, service("qwen", "qwen-small"), &[], None)
        .0
        .unwrap();
    let id = reply.handoff_id.unwrap();
    assert_eq!(ThreadStore::new(&dir).list().unwrap(), vec![id]);
}

#[test]
fn paid_pick_with_a_key_is_answered() {
    let dir = data_dir("paid");
    let (reply, seen) = run(
        &dir,
        service("deepseek", "deepseek-chat"),
        &[("DEEPSEEK_API_KEY", "sk-test-1")],
        Some("t-3"),
    );
    let reply = reply.unwrap();
    assert_eq!(reply.service.as_deref(), Some("deepseek"));
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(ThreadStore::new(&dir).read("t-3").unwrap().len(), 2);
}

#[test]
fn paid_pick_with_no_key_is_an_error_and_stores_nothing() {
    for (svc, var) in [
        ("deepseek", "DEEPSEEK_API_KEY"),
        ("chatgpt", "OPENAI_API_KEY"),
        ("gemini", "GEMINI_API_KEY"),
        ("claudeapi", "ANTHROPIC_API_KEY"),
    ] {
        let dir = data_dir(&format!("nokey-{svc}"));
        // A key for another service, and a blank one for this one, do not count.
        let (reply, seen) = run(
            &dir,
            service(svc, "some-model"),
            &[("OTHER_API_KEY", "x"), (var, "  ")],
            Some("t-4"),
        );
        let err = reply.unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("refused: no key for {svc}: set {var}")
        );
        assert!(seen.lock().unwrap().is_empty(), "{svc}: sent a call");
        assert!(ThreadStore::new(&dir).list().unwrap().is_empty(), "{svc}");
        assert!(!err.to_string().contains("sk-"));
    }
}

#[test]
fn claude_handoff_sends_and_stores_nothing() {
    // #377: this test once asserted the same for a wait; a wait now queues the ask (see
    // fd_queued_ask.rs).
    let pick = ChatPick::ClaudeHandoff {
        reason: "needs tools".to_string(),
    };
    let dir = data_dir("handoff");
    let (reply, seen) = run(&dir, pick.clone(), &[], Some("t-5"));
    let reply = reply.unwrap();
    assert_eq!(reply.route.pick, pick);
    assert_eq!(reply.text, None);
    assert_eq!(reply.handoff_id, None);
    assert!(seen.lock().unwrap().is_empty());
    assert!(ThreadStore::new(&dir).list().unwrap().is_empty());
}

#[test]
fn reply_json_keeps_the_route_shape_and_adds_the_answer() {
    let dir = data_dir("json");
    let reply = run(&dir, service("qwen", "qwen-small"), &[], Some("t-6"))
        .0
        .unwrap();
    let value = serde_json::to_value(&reply).unwrap();
    assert_eq!(value["step"], "labeller");
    assert_eq!(value["kind"], "quick-lookup");
    assert_eq!(value["pick"]["to"], "service");
    assert_eq!(value["text"], "Use a worktree.");
    assert_eq!(value["service"], "qwen");
    assert_eq!(value["model"], "qwen-small");
    assert_eq!(value["handoff_id"], "t-6");
    assert!(value.get("handoff").is_none());

    let none = run(
        &data_dir("json2"),
        ChatPick::ClaudeHandoff {
            reason: "r".to_string(),
        },
        &[],
        None,
    )
    .0
    .unwrap();
    let value = serde_json::to_value(&none).unwrap();
    assert!(value.get("text").is_none());
    assert_eq!(value["pick"]["to"], "claude-handoff");
}

#[test]
fn qwen_base_url_drops_the_models_tail() {
    let config = json!({"machines": {"m1": {"qwen_server_url": "http://127.0.0.1:8080/v1/models"},
                                       "m2": {"qwen_server_url": "http://host:9/v1/models/"},
                                       "m3": {}}});
    assert_eq!(
        qwen_base_from_config(&config, Some("m1")).as_deref(),
        Some("http://127.0.0.1:8080")
    );
    assert_eq!(
        qwen_base_from_config(&config, Some("m2")).as_deref(),
        Some("http://host:9")
    );
    assert_eq!(qwen_base_from_config(&config, Some("m3")), None);
    assert_eq!(qwen_base_from_config(&config, None), None);
}

#[test]
fn key_vars_are_one_per_paid_service() {
    assert_eq!(key_var("deepseek"), Some("DEEPSEEK_API_KEY"));
    assert_eq!(key_var("chatgpt"), Some("OPENAI_API_KEY"));
    assert_eq!(key_var("gemini"), Some("GEMINI_API_KEY"));
    assert_eq!(key_var("claudeapi"), Some("ANTHROPIC_API_KEY"));
    assert_eq!(key_var("qwen"), None);
}
