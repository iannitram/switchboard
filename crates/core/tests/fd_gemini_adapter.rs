//! Tests for FD-11's Gemini adapter (`GeminiAdapter` posts the system instruction and the turns
//! to `generateContent`, and turns the reply into an `Answer` with no cost recorded). The
//! transport is a fake that records the request and plays back a recorded answer; no service is
//! called.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use switchboard_core::frontdoor::adapters::{ChatAdapter, ChatTransport, GeminiAdapter};
use switchboard_core::frontdoor::packet::Turn;
use switchboard_core::CoreError;

const BASE: &str = "http://127.0.0.1:8080";
const KEY: &str = "test-key-123";
const URL: &str =
    "http://127.0.0.1:8080/v1beta/models/gemini-2.5-flash:generateContent?key=test-key-123";

/// A generateContent answer as recorded from the endpoint (ids and safety ratings trimmed). The
/// text is split over two parts, which must be joined in order.
const RECORDED: &str = r#"{
  "candidates": [
    {
      "content": {
        "role": "model",
        "parts": [{"text": "Use a worktree per "}, {"text": "issue."}]
      },
      "finishReason": "STOP",
      "index": 0
    }
  ],
  "usageMetadata": {
    "promptTokenCount": 388,
    "candidatesTokenCount": 9,
    "totalTokenCount": 397
  },
  "modelVersion": "gemini-2.5-flash"
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

fn adapter(reply: Result<Value, String>) -> (GeminiAdapter, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let transport = FakeTransport {
        reply,
        seen: seen.clone(),
    };
    (GeminiAdapter::new(BASE, KEY, Box::new(transport)), seen)
}

fn recorded() -> Value {
    serde_json::from_str(RECORDED).unwrap()
}

fn with_text(text: &str) -> Value {
    let mut body = recorded();
    body["candidates"][0]["content"]["parts"] = json!([{"text": text}]);
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

fn assert_no_secrets(why: &str) {
    assert!(!why.contains(KEY), "reason leaks the key: {why}");
    assert!(
        !why.contains("127.0.0.1:8080"),
        "reason leaks the url: {why}"
    );
    assert!(
        !why.contains("generateContent"),
        "reason leaks the url: {why}"
    );
}

#[test]
fn service_is_gemini() {
    let (gemini, _) = adapter(Ok(recorded()));
    assert_eq!(gemini.service(), "gemini");
}

#[test]
fn recorded_answer_gives_text_model_tokens_and_no_cost() {
    let (gemini, _) = adapter(Ok(recorded()));
    let answer = gemini
        .send("gemini-2.5-flash", "You are terse.", &turns())
        .unwrap();
    assert_eq!(answer.text, "Use a worktree per issue.");
    assert_eq!(answer.service, "gemini");
    assert_eq!(answer.model, "gemini-2.5-flash");
    assert_eq!(answer.tokens_in, Some(388));
    assert_eq!(answer.tokens_out, Some(9));
    assert_eq!(answer.cost_usd, None);
    assert_eq!(answer.handoff, None);
}

#[test]
fn model_passed_in_is_the_one_reported() {
    let (gemini, seen) = adapter(Ok(recorded()));
    let answer = gemini.send("gemini-2.0-pro", "sys", &turns()).unwrap();
    assert_eq!(answer.model, "gemini-2.0-pro");
    assert_eq!(
        seen.lock().unwrap()[0].0,
        "http://127.0.0.1:8080/v1beta/models/gemini-2.0-pro:generateContent?key=test-key-123"
    );
}

#[test]
fn handoff_marker_is_split_off() {
    let (gemini, _) = adapter(Ok(with_text(
        "I can't see the code. [[handoff: needs the repo]]",
    )));
    let answer = gemini.send("gemini-2.5-flash", "sys", &turns()).unwrap();
    assert_eq!(answer.text, "I can't see the code.");
    assert_eq!(answer.handoff, Some("needs the repo".to_string()));
}

#[test]
fn missing_usage_gives_no_tokens() {
    let mut body = recorded();
    body.as_object_mut().unwrap().remove("usageMetadata");
    let (gemini, _) = adapter(Ok(body));
    let answer = gemini.send("gemini-2.5-flash", "sys", &turns()).unwrap();
    assert_eq!(answer.tokens_in, None);
    assert_eq!(answer.tokens_out, None);
    assert_eq!(answer.cost_usd, None);
}

#[test]
fn transport_error_is_refused_without_key_or_url() {
    let (gemini, _) = adapter(Err(format!("POST {URL}: connection refused")));
    match gemini.send("gemini-2.5-flash", "sys", &turns()) {
        Err(CoreError::Refused(why)) => {
            assert!(why.contains("connection refused"), "{why}");
            assert_no_secrets(&why);
        }
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[test]
fn empty_candidates_is_refused_without_key_or_url() {
    let mut body = recorded();
    body["candidates"] = json!([]);
    let (gemini, _) = adapter(Ok(body));
    match gemini.send("gemini-2.5-flash", "sys", &turns()) {
        Err(CoreError::Refused(why)) => assert_no_secrets(&why),
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[test]
fn request_has_system_instruction_then_turns_in_order_at_the_generate_path() {
    let (gemini, seen) = adapter(Ok(recorded()));
    gemini
        .send("gemini-2.5-flash", "You are terse.", &turns())
        .unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let (url, body) = &seen[0];
    assert_eq!(url, URL);
    assert_eq!(
        body["systemInstruction"],
        json!({"parts": [{"text": "You are terse."}]})
    );
    assert_eq!(
        body["contents"],
        json!([
            {"role": "user", "parts": [{"text": "How should I lay out branches?"}]},
            {"role": "model", "parts": [{"text": "One per issue."}]},
            {"role": "user", "parts": [{"text": "And worktrees?"}]},
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
    let gemini = GeminiAdapter::new("http://127.0.0.1:8080/", KEY, Box::new(transport));
    gemini.send("gemini-2.5-flash", "sys", &[]).unwrap();
    assert_eq!(seen.lock().unwrap()[0].0, URL);
}
