//! Tests for FD-11's `QwenAdapter`: a recorded llama-swap answer is read into an `Answer`, the
//! request goes out as system first then the turns, at `/v1/chat/completions`; a transport
//! error or an empty reply is `Refused`. A fake transport records the call; no server is run.

use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use switchboard_core::frontdoor::adapters::{Answer, ChatAdapter, ChatTransport, QwenAdapter};
use switchboard_core::frontdoor::packet::Turn;
use switchboard_core::CoreError;

/// One call the fake transport saw: the URL and the JSON body.
type Seen = Arc<Mutex<Vec<(String, Value)>>>;

/// Answers every call with `reply`, or fails it with `Io(why)` when `reply` is `Err`.
struct FakeTransport {
    reply: Result<Value, String>,
    seen: Seen,
}

impl ChatTransport for FakeTransport {
    fn post_json(&self, url: &str, body: &Value) -> Result<Value, CoreError> {
        let mut calls = self.seen.lock().unwrap();
        calls.push((url.to_string(), body.clone()));
        match &self.reply {
            Ok(value) => Ok(value.clone()),
            Err(why) => Err(CoreError::Io(why.clone())),
        }
    }
}

/// A recorded llama-swap answer, as the OpenAI-style endpoint returns it.
fn recorded_reply(content: &str) -> Value {
    json!({
        "id": "chatcmpl-1",
        "object": "chat.completion",
        "model": "qwen3-coder",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 42, "completion_tokens": 7, "total_tokens": 49}
    })
}

fn make_adapter(reply: Result<Value, String>) -> (QwenAdapter, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let transport = FakeTransport {
        reply,
        seen: Arc::clone(&seen),
    };
    let adapter = QwenAdapter::new("http://127.0.0.1:8080", Box::new(transport));
    (adapter, seen)
}

fn turns() -> Vec<Turn> {
    vec![
        Turn {
            role: "user".to_string(),
            text: "first".to_string(),
        },
        Turn {
            role: "assistant".to_string(),
            text: "second".to_string(),
        },
        Turn {
            role: "user".to_string(),
            text: "third".to_string(),
        },
    ]
}

#[test]
fn recorded_answer_gives_text_service_model_tokens_and_zero_cost() {
    let (adapter, _seen) = make_adapter(Ok(recorded_reply("Take the fix.")));
    assert_eq!(adapter.service(), "qwen");
    let got: Answer = adapter
        .send("qwen3-coder", "be brief", &turns())
        .expect("recorded answer is a reply");
    assert_eq!(got.text, "Take the fix.");
    assert_eq!(got.service, "qwen");
    assert_eq!(got.model, "qwen3-coder");
    assert_eq!(got.tokens_in, Some(42));
    assert_eq!(got.tokens_out, Some(7));
    assert_eq!(got.cost_usd, Some(0.0));
    assert_eq!(got.handoff, None);
}

#[test]
fn handoff_marker_is_split_off_the_text() {
    let (adapter, _seen) = make_adapter(Ok(recorded_reply(
        "Not sure yet. [[handoff: needs the repo]]",
    )));
    let got = adapter
        .send("qwen3-coder", "be brief", &turns())
        .expect("reply with a marker");
    assert_eq!(got.text, "Not sure yet.");
    assert_eq!(got.handoff, Some("needs the repo".to_string()));
}

#[test]
fn body_without_usage_gives_no_token_counts() {
    let mut reply = recorded_reply("Take the fix.");
    reply.as_object_mut().unwrap().remove("usage");
    let (adapter, _seen) = make_adapter(Ok(reply));
    let got = adapter
        .send("qwen3-coder", "be brief", &turns())
        .expect("reply without usage");
    assert_eq!(got.tokens_in, None);
    assert_eq!(got.tokens_out, None);
    assert_eq!(got.cost_usd, Some(0.0));
}

#[test]
fn transport_error_is_refused_with_the_reason() {
    let (adapter, _seen) = make_adapter(Err("connection refused".to_string()));
    let err = adapter
        .send("qwen3-coder", "be brief", &turns())
        .expect_err("transport error must not be a reply");
    match err {
        CoreError::Refused(why) => assert!(why.contains("connection refused"), "{why}"),
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[test]
fn empty_choices_is_refused() {
    let reply = json!({"choices": [], "usage": {"prompt_tokens": 1, "completion_tokens": 0}});
    let (adapter, _seen) = make_adapter(Ok(reply));
    let err = adapter
        .send("qwen3-coder", "be brief", &turns())
        .expect_err("no choices means no answer");
    assert!(matches!(err, CoreError::Refused(_)), "{err:?}");
}

#[test]
fn request_has_system_first_then_turns_in_order_at_the_completions_path() {
    let (adapter, seen) = adapter(Ok(recorded_reply("ok")));
    adapter
        .send("qwen3-coder", "be brief", &turns())
        .expect("recorded answer is a reply");
    let calls = seen.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let (url, body) = &calls[0];
    assert_eq!(url, "http://127.0.0.1:8080/v1/chat/completions");
    assert_eq!(body["model"], "qwen3-coder");
    assert_eq!(
        body["messages"],
        json!([
            {"role": "system", "content": "be brief"},
            {"role": "user", "content": "first"},
            {"role": "assistant", "content": "second"},
            {"role": "user", "content": "third"}
        ])
    );
}
