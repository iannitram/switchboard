//! Tests for FD-2's small labeller (`LlamaLabeller` asks the small model for one kind id, or
//! `none`, and keeps the answer only when it equals a kind). The transport is a fake that records
//! each request and plays back a reply or an error, and counts its calls; no model runs.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use switchboard_core::abilities::AbilitiesIndex;
use switchboard_core::frontdoor::adapters::ChatTransport;
use switchboard_core::frontdoor::labeller::{
    label, FallbackLabeller, LlamaLabeller, PaidLabeller, SmallLabeller,
};
use switchboard_core::frontdoor::router::{self, BudgetGate, Complexity, Step};
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

    fn post_json_headers(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &Value,
    ) -> Result<Value, CoreError> {
        assert_eq!(
            headers,
            &[("Authorization".to_string(), "Bearer test-key".to_string())]
        );
        self.post_json(url, body)
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
    let got = label(
        "Tell me a joke",
        &kinds(),
        Some(&small as &dyn SmallLabeller),
    );
    assert_eq!(got, Some("writing-editing".to_string()));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn combined_label_never_calls_the_small_model_when_a_rule_matches() {
    let (small, seen) = labeller(Ok(with_content("writing-editing")));
    let got = label(
        "What is a mutex?",
        &kinds(),
        Some(&small as &dyn SmallLabeller),
    );
    assert_eq!(got, Some("quick-lookup".to_string()));
    assert!(seen.lock().unwrap().is_empty());
}

struct Allowed(Vec<&'static str>);

impl BudgetGate for Allowed {
    fn allows(&self, service: &str) -> bool {
        self.0.contains(&service)
    }
}

fn paid_index() -> AbilitiesIndex {
    AbilitiesIndex::from_json(&json!({
        "quick-lookup": {"request": true, "first": ["chatgpt"], "ok": {
            "gemini": {"model": "gemini-small"},
            "chatgpt": {"model": "gpt-small"},
            "deepseek": {"model": "deepseek-small"}
        }},
        "writing-editing": {"request": true, "first": ["chatgpt"]}
    }))
    .unwrap()
}

fn fallback(
    local_reply: Result<Value, String>,
    has_key: bool,
    has_budget: bool,
) -> (impl SmallLabeller, Seen, Seen) {
    let (local, local_seen) = labeller(local_reply);
    let paid_seen = Seen::default();
    let seen = paid_seen.clone();
    let small = FallbackLabeller::new(local, move || {
        PaidLabeller::new(
            &paid_index(),
            &Allowed(if has_budget { vec!["chatgpt"] } else { vec![] }),
            &|_| has_key.then(|| "test-key".to_string()),
            &|| {
                Box::new(FakeTransport {
                    reply: Ok(with_content("writing-editing")),
                    seen: seen.clone(),
                })
            },
        )
    });
    (small, local_seen, paid_seen)
}

#[test]
fn local_failure_uses_the_paid_labeller() {
    for error in ["connection refused", "request timed out after 8 s"] {
        let (small, local_seen, paid_seen) = fallback(Err(error.to_string()), true, true);
        let got = router::route(
            "Tell me a joke",
            &paid_index(),
            &Allowed(vec!["chatgpt"]),
            Complexity::Small,
            Some(&small),
        );
        assert_eq!(
            got.step,
            Step::Labeller {
                kind: "writing-editing".to_string()
            }
        );
        assert_eq!(local_seen.lock().unwrap().len(), 1);
        let seen = paid_seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "https://api.openai.com/v1/chat/completions");
        assert_eq!(seen[0].1["model"], "gpt-small");
        assert_eq!(seen[0].1["messages"][1]["content"], "Tell me a joke");
        let system = seen[0].1["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("quick-lookup"));
        assert!(system.contains("writing-editing"));
    }
}

fn assert_rules_only(has_key: bool, has_budget: bool) {
    let (small, local_seen, paid_seen) = fallback(Err("down".to_string()), has_key, has_budget);
    let index = paid_index();
    let budget = Allowed(vec!["chatgpt"]);
    let got = router::route(
        "Tell me a joke",
        &index,
        &budget,
        Complexity::Small,
        Some(&small),
    );
    let rules_only = router::route("Tell me a joke", &index, &budget, Complexity::Small, None);
    assert_eq!(got, rules_only);
    assert_eq!(local_seen.lock().unwrap().len(), 1);
    assert!(paid_seen.lock().unwrap().is_empty());
}

#[test]
fn local_failure_without_a_key_keeps_the_rules_only_route() {
    assert_rules_only(false, true);
}

#[test]
fn local_failure_without_budget_keeps_the_rules_only_route() {
    assert_rules_only(true, false);
}

#[test]
fn local_success_never_even_constructs_the_paid_labeller() {
    for content in ["quick-lookup", "none"] {
        let (local, seen) = labeller(Ok(with_content(content)));
        let small =
            FallbackLabeller::new(local, || panic!("paid labeller must not be constructed"));
        assert_eq!(
            label("Tell me a joke", &kinds(), Some(&small)),
            (content != "none").then(|| content.to_string())
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}

#[test]
fn matching_rules_never_call_either_labeller() {
    let (small, local_seen, paid_seen) = fallback(Err("down".to_string()), true, true);
    assert_eq!(
        label("What is a mutex?", &kinds(), Some(&small)),
        Some("quick-lookup".to_string())
    );
    assert!(local_seen.lock().unwrap().is_empty());
    assert!(paid_seen.lock().unwrap().is_empty());
}

#[test]
fn paid_labeller_uses_the_first_small_pool_with_a_key_and_budget() {
    for (allowed, key_var, expected_url, expected_model) in [
        (vec!["gemini", "chatgpt", "deepseek"], "GEMINI_API_KEY", "https://generativelanguage.googleapis.com/v1beta/models/gemini-small:generateContent?key=test-key", None),
        (vec!["chatgpt", "deepseek"], "OPENAI_API_KEY", "https://api.openai.com/v1/chat/completions", Some("gpt-small")),
        (vec!["deepseek"], "DEEPSEEK_API_KEY", "https://api.deepseek.com/chat/completions", Some("deepseek-small")),
    ] {
        let seen = Seen::default();
        let paid = PaidLabeller::new(&paid_index(), &Allowed(allowed),
            &|name| (name == key_var).then(|| "test-key".to_string()),
            &|| Box::new(FakeTransport {
                reply: Ok(json!({
                    "choices": [{"message": {"content": " Quick-Lookup\n"}}],
                    "candidates": [{"content": {"parts": [{"text": " Quick-Lookup\n"}]}}]
                })), seen: seen.clone(),
            }),
        ).unwrap();
        assert_eq!(paid.label("Tell me a joke", &kinds()), Some("quick-lookup".to_string()));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, expected_url);
        if let Some(model) = expected_model { assert_eq!(seen[0].1["model"], model); }
    }
}

#[test]
fn paid_failure_or_unclear_reply_gives_no_kind() {
    for reply in [
        Err("down".to_string()),
        Ok(with_content("not-a-kind")),
        Ok(with_content("none")),
        Ok(with_content("I think quick-lookup")),
    ] {
        let paid = PaidLabeller::new(
            &paid_index(),
            &Allowed(vec!["chatgpt"]),
            &|_| Some("test-key".to_string()),
            &|| {
                Box::new(FakeTransport {
                    reply: reply.clone(),
                    seen: Seen::default(),
                })
            },
        )
        .unwrap();
        assert_eq!(paid.label("Tell me a joke", &kinds()), None);
    }
}
