//! Tests for FD-11's ChatGPT adapter (`ChatGptAdapter` posts the system message and the turns
//! to `<base_url>/v1/chat/completions` with `Authorization: Bearer <key>`, and turns the reply
//! into an `Answer` whose cost is None: ChatGPT is a quota that resets, not a priced pool). The
//! transport is a fake that records the request and plays back a recorded answer; no service is
//! called.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use switchboard_core::frontdoor::adapters::{ChatAdapter, ChatGptAdapter, ChatTransport};
use switchboard_core::frontdoor::packet::Turn;
use switchboard_core::CoreError;

const KEY: &str = "sk-test-key-7b1e3d";

/// A ChatGPT answer as recorded from the endpoint (ids and timings trimmed).
const RECORDED: &str = r#"{
  "id": "chatcmpl-9a7c1e",
  "object": "chat.completion",
  "created": 1762650000,
  "model": "gpt-4o-mini",
  "choices": [
    {
      "index": 0,
      "finish_reason": "stop",
      "message": {"role": "assistant", "content": "Use a worktree per issue."}
    }
  ],
  "usage": {"prompt_tokens": 388, "completion_tokens": 29, "total_tokens": 417}
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

fn adapter(reply: Result<Value, String>) -> (ChatGptAdapter, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let transport = FakeTransport {
        reply,
        seen: seen.clone(),
    };
    (
        ChatGptAdapter::new("https://api.openai.com", KEY, Box::new(transport)),
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
fn service_is_chatgpt() {
    let (chatgpt, _) = adapter(Ok(recorded()));
    assert_eq!(chatgpt.service(), "chatgpt");
}

#[test]
fn recorded_answer_gives_text_model_tokens_and_no_cost() {
    let (chatgpt, _) = adapter(Ok(recorded()));
    let answer = chatgpt
        .send("gpt-4o-mini", "You are terse.", &turns())
        .unwrap();
    assert_eq!(answer.text, "Use a worktree per issue.");
    assert_eq!(answer.service, "chatgpt");
    assert_eq!(answer.model, "gpt-4o-mini");
    assert_eq!(answer.tokens_in, Some(388));
    assert_eq!(answer.tokens_out, Some(29));
    assert_eq!(answer.cost_usd, None, "a quota is not a priced cost");
    assert_eq!(answer.handoff, None);
}

#[test]
fn model_passed_in_is_the_one_reported() {
    let (chatgpt, seen) = adapter(Ok(recorded()));
    let answer = chatgpt.send("gpt-4.1", "sys", &turns()).unwrap();
    assert_eq!(answer.model, "gpt-4.1");
    assert_eq!(seen.lock().unwrap()[0].2["model"], json!("gpt-4.1"));
}

#[test]
fn handoff_marker_is_split_off() {
    let (chatgpt, _) = adapter(Ok(with_content(
        "I can't see the code. [[handoff: needs the repo]]",
    )));
    let answer = chatgpt.send("gpt-4o-mini", "sys", &turns()).unwrap();
    assert_eq!(answer.text, "I can't see the code.");
    assert_eq!(answer.handoff, Some("needs the repo".to_string()));
}

#[test]
fn missing_usage_gives_no_tokens() {
    let mut body = recorded();
    body.as_object_mut().unwrap().remove("usage");
    let (chatgpt, _) = adapter(Ok(body));
    let answer = chatgpt.send("gpt-4o-mini", "sys", &turns()).unwrap();
    assert_eq!(answer.tokens_in, None);
    assert_eq!(answer.tokens_out, None);
    assert_eq!(answer.cost_usd, None);
}

#[test]
fn transport_error_is_refused_and_never_names_the_key() {
    let (chatgpt, _) = adapter(Err(format!("connection refused (key {KEY})")));
    match chatgpt.send("gpt-4o-mini", "sys", &turns()) {
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
    let (chatgpt, _) = adapter(Ok(body));
    assert!(matches!(
        chatgpt.send("gpt-4o-mini", "sys", &turns()),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn request_carries_bearer_key_system_first_then_turns_at_the_chat_path() {
    let (chatgpt, seen) = adapter(Ok(recorded()));
    chatgpt
        .send("gpt-4o-mini", "You are terse.", &turns())
        .unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let (url, headers, body) = &seen[0];
    assert_eq!(url, "https://api.openai.com/v1/chat/completions");
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
    let chatgpt = ChatGptAdapter::new("https://api.openai.com/", KEY, Box::new(transport));
    chatgpt.send("gpt-4o-mini", "sys", &[]).unwrap();
    assert_eq!(
        seen.lock().unwrap()[0].0,
        "https://api.openai.com/v1/chat/completions"
    );
}

#[test]
fn transport_without_header_support_is_refused() {
    let chatgpt = ChatGptAdapter::new("https://api.openai.com", KEY, Box::new(PlainTransport));
    match chatgpt.send("gpt-4o-mini", "sys", &turns()) {
        Err(CoreError::Refused(why)) => assert!(why.contains("no headers"), "{why}"),
        other => panic!("expected Refused, got {other:?}"),
    }
}
