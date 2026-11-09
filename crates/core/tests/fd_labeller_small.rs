//! Tests for FD-2's small labeller (`LlamaLabeller` asks the small model for one kind id, or
//! `none`, and keeps the answer only when it equals a kind). The transport is a fake that records
//! each request and plays back a reply or an error, and counts its calls; no model runs.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use switchboard_core::frontdoor::adapters::ChatTransport;
use switchboard_core::frontdoor::labeller::{label, LlamaLabeller, SmallLabeller};
use switchboard_core::CoreError;

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

fn kinds() -> Vec<String> {
    vec!["quick-lookup".to_string(), "writing-editing".to_string()]
}

fn labeller(reply: Result<Value, String>) -> (LlamaLabeller, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let transport = FakeTransport {
        reply,
        seen: seen.clone(),
    };
    (
        LlamaLabeller::new("http://127.0.0.1:8080/", "qwen-small", Box::new(transport)),
        seen,
    )
}

fn with_content(content: &str) -> Value {
    json!({"choices": [{"index": 0, "message": {"role": "assistant", "content": content}}]})
}

#[test]
fn exact_kind_id_is_returned() {
    let (small, _) = labeller(Ok(with_content("quick-lookup")));
    assert_eq!(
        small.label("What is a mutex?", &kinds()),
        Some("quick-lookup".to_string())
    );
}

#[test]
fn surrounding_space_case_and_newline_are_ignored() {
    let (small, _) = labeller(Ok(with_content(" Quick-Lookup\n")));
    assert_eq!(
        small.label("What is a mutex?", &kinds()),
        Some("quick-lookup".to_string())
    );
}

#[test]
fn kind_not_in_the_list_is_none() {
    let (small, _) = labeller(Ok(with_content("web-research")));
    assert_eq!(small.label("Look this up online", &kinds()), None);
}

#[test]
fn none_is_none() {
    let (small, _) = labeller(Ok(with_content("none")));
    assert_eq!(small.label("Tell me a joke", &kinds()), None);
}

#[test]
fn extra_words_are_none() {
    let (small, _) = labeller(Ok(with_content("I think quick-lookup")));
    assert_eq!(small.label("What is a mutex?", &kinds()), None);
}

#[test]
fn empty_choices_is_none() {
    let (small, _) = labeller(Ok(json!({"choices": []})));
    assert_eq!(small.label("What is a mutex?", &kinds()), None);
}

#[test]
fn transport_error_is_none() {
    let (small, _) = labeller(Err("connection refused".to_string()));
    assert_eq!(small.label("What is a mutex?", &kinds()), None);
}

#[test]
fn request_names_the_model_with_temperature_zero_and_lists_the_kinds() {
    let (small, seen) = labeller(Ok(with_content("writing-editing")));
    let message = "Please proofread this paragraph.";
    assert_eq!(
        small.label(message, &kinds()),
        Some("writing-editing".to_string())
    );

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let (url, body) = &seen[0];
    assert_eq!(url, "http://127.0.0.1:8080/v1/chat/completions");
    assert_eq!(body["model"], json!("qwen-small"));
    assert_eq!(body["temperature"], json!(0));
    assert_eq!(body["max_tokens"], json!(16));

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], json!("system"));
    let system = messages[0]["content"].as_str().unwrap();
    for kind in kinds() {
        assert!(system.contains(&kind), "system message lacks {kind}");
    }
    assert_eq!(messages[1]["role"], json!("user"));
    assert_eq!(messages[1]["content"], json!(message));
}

#[test]
fn combined_label_uses_the_small_model_when_rules_miss() {
    let (small, seen) = labeller(Ok(with_content("writing-editing")));
    let got = label("Tell me a joke", &kinds(), Some(&small as &dyn SmallLabeller));
    assert_eq!(got, Some("writing-editing".to_string()));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn combined_label_never_calls_the_small_model_when_a_rule_matches() {
    let (small, seen) = labeller(Ok(with_content("writing-editing")));
    let got = label("What is a mutex?", &kinds(), Some(&small as &dyn SmallLabeller));
    assert_eq!(got, Some("quick-lookup".to_string()));
    assert!(seen.lock().unwrap().is_empty());
}
