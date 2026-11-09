//! Tests for FD-11's DeepSeek adapter (`DeepSeekAdapter` posts the system message and the turns
//! to `<base_url>/chat/completions` with `Authorization: Bearer <key>`, and turns the reply into
//! an `Answer` whose cost is priced from the given USD-per-million rates). The transport is a
//! fake that records the request and plays back a recorded answer; no service is called.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use switchboard_core::frontdoor::adapters::{ChatAdapter, ChatTransport, DeepSeekAdapter};
use switchboard_core::frontdoor::packet::Turn;
use switchboard_core::CoreError;

const KEY: &str = "sk-test-key-4f2a9c";
const IN_PRICE: f64 = 0.30;
const OUT_PRICE: f64 = 1.20;

/// A DeepSeek answer as recorded from the endpoint (ids and timings trimmed).
const RECORDED: &str = r#"{
  "id": "e1f0c2d4-deepseek",
  "object": "chat.completion",
  "created": 1762650000,
  "model": "deepseek-chat",
  "choices": [
    {
      "index": 0,
      "finish_reason": "stop",
      "message": {"role": "assistant", "content": "Use a worktree per issue."}
    }
  ],
  "usage": {"prompt_tokens": 412, "completion_tokens": 37, "total_tokens": 449}
}"#;

type Seen = Arc<Mutex<Vec<(String, Vec<(String, String)>, Value)>>>;

struct FakeTransport {
    reply: Result<Value, String>,
    seen: Seen,
}

impl ChatTransport for FakeTransport {
    fn post_json(&self, _url: &str, _body: &Value) -> Result<Value, CoreError> {
        Err(CoreError::Refused("post_json not expected".to_string()))
    }

    fn post_json_headers(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &Value,
    ) -> Result<Value, CoreError> {
        self.seen
            .lock()
            .unwrap()
            .push((url.to_string(), headers.to_vec(), body.clone()));
        self.reply.clone().map_err(CoreError::Refused)
    }
}

/// A transport that only implements `post_json`, so it does not override `post_json_headers`.
struct PlainTransport;

impl ChatTransport for PlainTransport {
    fn post_json(&self, _url: &str, _body: &Value) -> Result<Value, CoreError> {
        Ok(recorded())
    }
}

fn adapter(reply: Result<Value, String>) -> (DeepSeekAdapter, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let transport = FakeTransport {
        reply,
        seen: seen.clone(),
    };
    (
        DeepSeekAdapter::new(
            "https://api.deepseek.com",
            KEY,
            IN_PRICE,
            OUT_PRICE,
            Box::new(transport),
        ),
        seen,
    )
}

fn recorded() -> Value {
    serde_json::from_str(RECORDED).unwrap()
}

fn with_content(content: &str) -> Value {
    let mut body = recorded();
    body["choices"][0]["message"]["content"] = json!(content);
    body
}

fn turns() -> Vec<Turn> {
    vec![
        Turn {
            role: "user".to_string(),
            text: "How should I lay out branches?".to_string(),
        },
        Turn {
            role: "assistant".to_string(),
            text: "One per issue.".to_string(),
        },
        Turn {
            role: "user".to_string(),
            text: "And worktrees?".to_string(),
        },
    ]
}

#[test]
fn service_is_deepseek() {
    let (deepseek, _) = adapter(Ok(recorded()));
    assert_eq!(deepseek.service(), "deepseek");
}

#[test]
fn recorded_answer_gives_text_model_tokens_and_priced_cost() {
    let (deepseek, _) = adapter(Ok(recorded()));
    let answer = deepseek
        .send("deepseek-chat", "You are terse.", &turns())
        .unwrap();
    assert_eq!(answer.text, "Use a worktree per issue.");
    assert_eq!(answer.service, "deepseek");
    assert_eq!(answer.model, "deepseek-chat");
    assert_eq!(answer.tokens_in, Some(412));
    assert_eq!(answer.tokens_out, Some(37));
    let expected = 412.0 / 1e6 * 0.30 + 37.0 / 1e6 * 1.20;
    let cost = answer.cost_usd.expect("cost is known when tokens are");
    assert!((cost - expected).abs() < 1e-9, "cost {cost} vs {expected}");
    assert_eq!(answer.handoff, None);
}

#[test]
fn model_passed_in_is_the_one_reported() {
    let (deepseek, seen) = adapter(Ok(recorded()));
    let answer = deepseek.send("deepseek-reasoner", "sys", &turns()).unwrap();
    assert_eq!(answer.model, "deepseek-reasoner");
    assert_eq!(
        seen.lock().unwrap()[0].2["model"],
        json!("deepseek-reasoner")
    );
}

#[test]
fn handoff_marker_is_split_off() {
    let (deepseek, _) = adapter(Ok(with_content(
        "I can't see the code. [[handoff: needs the repo]]",
    )));
    let answer = deepseek.send("deepseek-chat", "sys", &turns()).unwrap();
    assert_eq!(answer.text, "I can't see the code.");
    assert_eq!(answer.handoff, Some("needs the repo".to_string()));
}

#[test]
fn missing_usage_gives_no_tokens_and_no_cost() {
    let mut body = recorded();
    body.as_object_mut().unwrap().remove("usage");
    let (deepseek, _) = adapter(Ok(body));
    let answer = deepseek.send("deepseek-chat", "sys", &turns()).unwrap();
    assert_eq!(answer.tokens_in, None);
    assert_eq!(answer.tokens_out, None);
    assert_eq!(answer.cost_usd, None);
}

#[test]
fn transport_error_is_refused_and_never_names_the_key() {
    let (deepseek, _) = adapter(Err(format!("connection refused (key {KEY})")));
    match deepseek.send("deepseek-chat", "sys", &turns()) {
        Err(CoreError::Refused(why)) => {
            assert!(why.contains("connection refused"), "{why}");
            assert!(!why.contains(KEY), "{why}");
        }
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[test]
fn empty_choices_is_refused() {
    let mut body = recorded();
    body["choices"] = json!([]);
    let (deepseek, _) = adapter(Ok(body));
    assert!(matches!(
        deepseek.send("deepseek-chat", "sys", &turns()),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn request_carries_bearer_key_system_first_then_turns_at_the_completions_path() {
    let (deepseek, seen) = adapter(Ok(recorded()));
    deepseek
        .send("deepseek-chat", "You are terse.", &turns())
        .unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let (url, headers, body) = &seen[0];
    assert_eq!(url, "https://api.deepseek.com/chat/completions");
    assert!(headers.contains(&("Authorization".to_string(), format!("Bearer {KEY}"))));
    assert_eq!(
        body["messages"],
        json!([
            {"role": "system", "content": "You are terse."},
            {"role": "user", "content": "How should I lay out branches?"},
            {"role": "assistant", "content": "One per issue."},
            {"role": "user", "content": "And worktrees?"},
        ])
    );
}

#[test]
fn trailing_slash_on_base_url_is_dropped() {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let transport = FakeTransport {
        reply: Ok(recorded()),
        seen: seen.clone(),
    };
    let deepseek = DeepSeekAdapter::new(
        "https://api.deepseek.com/",
        KEY,
        IN_PRICE,
        OUT_PRICE,
        Box::new(transport),
    );
    deepseek.send("deepseek-chat", "sys", &[]).unwrap();
    assert_eq!(
        seen.lock().unwrap()[0].0,
        "https://api.deepseek.com/chat/completions"
    );
}

#[test]
fn transport_without_header_support_is_refused() {
    let deepseek = DeepSeekAdapter::new(
        "https://api.deepseek.com",
        KEY,
        IN_PRICE,
        OUT_PRICE,
        Box::new(PlainTransport),
    );
    match deepseek.send("deepseek-chat", "sys", &turns()) {
        Err(CoreError::Refused(why)) => assert!(why.contains("no headers"), "{why}"),
        other => panic!("expected Refused, got {other:?}"),
    }
}
