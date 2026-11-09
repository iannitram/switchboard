//! Tests for FD-11's Claude API adapter (`ClaudeApiAdapter` posts the system text and the turns
//! to `<base_url>/v1/messages` with `x-api-key` and `anthropic-version` headers, and turns the
//! reply into an `Answer` whose cost is priced from the given USD-per-million rates). The
//! transport is a fake that records the request and plays back a recorded answer; no service is
//! called.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use switchboard_core::frontdoor::adapters::{ChatAdapter, ChatTransport, ClaudeApiAdapter};
use switchboard_core::frontdoor::packet::Turn;
use switchboard_core::CoreError;

const KEY: &str = "sk-ant-test-key-7b3e1d";
const IN_PRICE: f64 = 3.00;
const OUT_PRICE: f64 = 15.00;

/// A Messages answer as recorded from the endpoint (ids and timings trimmed).
const RECORDED: &str = r#"{
  "id": "msg_01claudeapi",
  "type": "message",
  "role": "assistant",
  "model": "claude-sonnet-4-5",
  "content": [
    {"type": "text", "text": "Use a worktree per issue."}
  ],
  "stop_reason": "end_turn",
  "usage": {"input_tokens": 412, "output_tokens": 37}
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

fn adapter(reply: Result<Value, String>) -> (ClaudeApiAdapter, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let transport = FakeTransport {
        reply,
        seen: seen.clone(),
    };
    (
        ClaudeApiAdapter::new(
            "https://api.anthropic.com",
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

fn with_content(blocks: Value) -> Value {
    let mut body = recorded();
    body["content"] = blocks;
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
fn service_is_claudeapi() {
    let (claude, _) = adapter(Ok(recorded()));
    assert_eq!(claude.service(), "claudeapi");
}

#[test]
fn recorded_answer_gives_text_model_tokens_and_priced_cost() {
    let (claude, _) = adapter(Ok(recorded()));
    let answer = claude
        .send("claude-sonnet-4-5", "You are terse.", &turns())
        .unwrap();
    assert_eq!(answer.text, "Use a worktree per issue.");
    assert_eq!(answer.service, "claudeapi");
    assert_eq!(answer.model, "claude-sonnet-4-5");
    assert_eq!(answer.tokens_in, Some(412));
    assert_eq!(answer.tokens_out, Some(37));
    let expected = 412.0 / 1e6 * 3.00 + 37.0 / 1e6 * 15.00;
    let cost = answer.cost_usd.expect("cost is known when tokens are");
    assert!((cost - expected).abs() < 1e-9, "cost {cost} vs {expected}");
    assert_eq!(answer.handoff, None);
}

#[test]
fn model_passed_in_is_the_one_reported() {
    let (claude, seen) = adapter(Ok(recorded()));
    let answer = claude.send("claude-opus-4-1", "sys", &turns()).unwrap();
    assert_eq!(answer.model, "claude-opus-4-1");
    assert_eq!(seen.lock().unwrap()[0].2["model"], json!("claude-opus-4-1"));
}

#[test]
fn request_has_headers_max_tokens_system_and_ordered_turns() {
    let (claude, seen) = adapter(Ok(recorded()));
    claude
        .send("claude-sonnet-4-5", "You are terse.", &turns())
        .unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let (url, headers, body) = &seen[0];
    assert_eq!(url, "https://api.anthropic.com/v1/messages");
    assert!(headers.contains(&("x-api-key".to_string(), KEY.to_string())));
    assert!(headers.contains(&("anthropic-version".to_string(), "2023-06-01".to_string())));
    assert_eq!(body["max_tokens"], json!(4096));
    assert_eq!(body["system"], json!("You are terse."));
    assert_eq!(
        body["messages"],
        json!([
            {"role": "user", "content": "How should I lay out branches?"},
            {"role": "assistant", "content": "One per issue."},
            {"role": "user", "content": "And worktrees?"},
        ])
    );
}

#[test]
fn handoff_marker_is_split_off() {
    let (claude, _) = adapter(Ok(with_content(json!([
        {"type": "text", "text": "I can't see the code. [[handoff: needs the repo]]"}
    ]))));
    let answer = claude.send("claude-sonnet-4-5", "sys", &turns()).unwrap();
    assert_eq!(answer.text, "I can't see the code.");
    assert_eq!(answer.handoff, Some("needs the repo".to_string()));
}

#[test]
fn only_text_blocks_are_kept_when_a_tool_use_block_is_present() {
    let (claude, _) = adapter(Ok(with_content(json!([
        {"type": "text", "text": "Let me look. "},
        {"type": "tool_use", "id": "toolu_1", "name": "read_file", "input": {"path": "a.rs"}},
        {"type": "text", "text": "Done."}
    ]))));
    let answer = claude.send("claude-sonnet-4-5", "sys", &turns()).unwrap();
    assert_eq!(answer.text, "Let me look. Done.");
    assert_eq!(answer.handoff, None);
}

#[test]
fn body_without_usage_gives_no_tokens_and_no_cost() {
    let mut body = recorded();
    body.as_object_mut().unwrap().remove("usage");
    let (claude, _) = adapter(Ok(body));
    let answer = claude.send("claude-sonnet-4-5", "sys", &turns()).unwrap();
    assert_eq!(answer.text, "Use a worktree per issue.");
    assert_eq!(answer.tokens_in, None);
    assert_eq!(answer.tokens_out, None);
    assert_eq!(answer.cost_usd, None);
}

#[test]
fn transport_error_is_refused_without_the_key() {
    let (claude, _) = adapter(Err(format!(
        "POST https://api.anthropic.com/v1/messages: status 401 for key {KEY}"
    )));
    match claude.send("claude-sonnet-4-5", "sys", &turns()) {
        Err(CoreError::Refused(why)) => {
            assert!(!why.contains(KEY), "reason leaks the key: {why}");
            assert!(why.contains("401"));
        }
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[test]
fn reply_with_no_text_block_is_refused_without_the_key() {
    let (claude, _) = adapter(Ok(with_content(json!([
        {"type": "tool_use", "id": "toolu_1", "name": "read_file", "input": {}}
    ]))));
    match claude.send("claude-sonnet-4-5", "sys", &turns()) {
        Err(CoreError::Refused(why)) => assert!(!why.contains(KEY)),
        other => panic!("expected Refused, got {other:?}"),
    }
}
