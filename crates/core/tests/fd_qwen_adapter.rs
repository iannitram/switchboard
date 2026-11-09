//! Tests for FD-11's Qwen adapter (`QwenAdapter` posts the system message and the turns to
//! llama-swap's `/v1/chat/completions`, and turns the reply into an `Answer` with cost 0.0).
//! The transport is a fake that records the request and plays back a recorded answer; no
//! service is called.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use switchboard_core::frontdoor::adapters::{ChatAdapter, ChatTransport, QwenAdapter};
use switchboard_core::frontdoor::packet::Turn;
use switchboard_core::CoreError;

/// A llama-swap answer as recorded from the endpoint (ids and timings trimmed).
const RECORDED: &str = r#"{
  "id": "chatcmpl-qlocal",
  "object": "chat.completion",
  "created": 1762650000,
  "model": "qwen3-coder-30b",
  "choices": [
    {
      "index": 0,
      "finish_reason": "stop",
      "message": {"role": "assistant", "content": "Use a worktree per issue."}
    }
  ],
  "usage": {"prompt_tokens": 412, "completion_tokens": 37, "total_tokens": 449}
}"#;

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

struct FakeTransport {
    reply: Result<Value, String>,
    seen: Seen,
}

impl ChatTransport for FakeTransport {
    fn post_json(&self, url: &str, body: &Value) -> Result<Value, CoreError> {
        self.seen
            .lock()
            .unwrap()
            .push((url.to_string(), body.clone()));
        self.reply.clone().map_err(CoreError::Refused)
    }
}

fn adapter(reply: Result<Value, String>) -> (QwenAdapter, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let transport = FakeTransport {
        reply,
        seen: seen.clone(),
    };
    (
        QwenAdapter::new("http://127.0.0.1:8080", Box::new(transport)),
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
fn service_is_qwen() {
    let (qwen, _) = adapter(Ok(recorded()));
    assert_eq!(qwen.service(), "qwen");
}

#[test]
fn recorded_answer_gives_text_model_tokens_and_zero_cost() {
    let (qwen, _) = adapter(Ok(recorded()));
    let answer = qwen
        .send("qwen3-coder-30b", "You are terse.", &turns())
        .unwrap();
    assert_eq!(answer.text, "Use a worktree per issue.");
    assert_eq!(answer.service, "qwen");
    assert_eq!(answer.model, "qwen3-coder-30b");
    assert_eq!(answer.tokens_in, Some(412));
    assert_eq!(answer.tokens_out, Some(37));
    assert_eq!(answer.cost_usd, Some(0.0));
    assert_eq!(answer.handoff, None);
}

#[test]
fn model_passed_in_is_the_one_reported() {
    let (qwen, seen) = adapter(Ok(recorded()));
    let answer = qwen.send("qwen3-8b", "sys", &turns()).unwrap();
    assert_eq!(answer.model, "qwen3-8b");
    assert_eq!(seen.lock().unwrap()[0].1["model"], json!("qwen3-8b"));
}

#[test]
fn handoff_marker_is_split_off() {
    let (qwen, _) = adapter(Ok(with_content(
        "I can't see the code. [[handoff: needs the repo]]",
    )));
    let answer = qwen.send("qwen3-coder-30b", "sys", &turns()).unwrap();
    assert_eq!(answer.text, "I can't see the code.");
    assert_eq!(answer.handoff, Some("needs the repo".to_string()));
}

#[test]
fn missing_usage_gives_no_tokens() {
    let mut body = recorded();
    body.as_object_mut().unwrap().remove("usage");
    let (qwen, _) = adapter(Ok(body));
    let answer = qwen.send("qwen3-coder-30b", "sys", &turns()).unwrap();
    assert_eq!(answer.tokens_in, None);
    assert_eq!(answer.tokens_out, None);
    assert_eq!(answer.cost_usd, Some(0.0));
}

#[test]
fn transport_error_is_refused() {
    let (qwen, _) = adapter(Err("connection refused".to_string()));
    match qwen.send("qwen3-coder-30b", "sys", &turns()) {
        Err(CoreError::Refused(why)) => assert!(why.contains("connection refused"), "{why}"),
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[test]
fn empty_choices_is_refused() {
    let mut body = recorded();
    body["choices"] = json!([]);
    let (qwen, _) = adapter(Ok(body));
    assert!(matches!(
        qwen.send("qwen3-coder-30b", "sys", &turns()),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn request_has_system_first_then_turns_in_order_at_the_completions_path() {
    let (qwen, seen) = adapter(Ok(recorded()));
    qwen.send("qwen3-coder-30b", "You are terse.", &turns())
        .unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let (url, body) = &seen[0];
    assert_eq!(url, "http://127.0.0.1:8080/v1/chat/completions");
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
    let qwen = QwenAdapter::new("http://127.0.0.1:8080/", Box::new(transport));
    qwen.send("qwen3-coder-30b", "sys", &[]).unwrap();
    assert_eq!(
        seen.lock().unwrap()[0].0,
        "http://127.0.0.1:8080/v1/chat/completions"
    );
}
