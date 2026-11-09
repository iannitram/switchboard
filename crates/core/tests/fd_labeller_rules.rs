//! Tests for FD-2's rules (`by_rules` names the request kind a message's words name, or nothing;
//! `label` tries the rules, then the small labeller, and drops a kind the abilities index does not
//! list). The kinds come from the abilities index fixture; the small labeller is a stub. No model
//! runs.

use std::path::PathBuf;
use switchboard_core::abilities::AbilitiesIndex;
use switchboard_core::frontdoor::labeller::{by_rules, label, SmallLabeller};

/// The request kind ids the abilities index fixture lists, in id order.
fn kinds() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fleet-stub")
        .join("answers")
        .join("actions.json");
    let text = std::fs::read_to_string(path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    AbilitiesIndex::from_json(&value)
        .unwrap()
        .request_kinds()
        .iter()
        .map(|kind| kind.id.clone())
        .collect()
}

/// A small labeller that answers the same thing every time.
struct Answers(Option<String>);

impl SmallLabeller for Answers {
    fn label(&self, _message: &str, _kinds: &[String]) -> Option<String> {
        self.0.clone()
    }
}

/// A small labeller that fails the test if it is ever asked.
struct Panics;

impl SmallLabeller for Panics {
    fn label(&self, _message: &str, _kinds: &[String]) -> Option<String> {
        panic!("the small labeller must not be called when a rule matches")
    }
}

#[test]
fn search_the_web_names_web_research() {
    assert_eq!(
        by_rules("search the web for the new release").as_deref(),
        Some("web-research")
    );
}

#[test]
fn web_search_names_web_research() {
    assert_eq!(
        by_rules("run a web search on sidecar ports").as_deref(),
        Some("web-research")
    );
}

#[test]
fn proofread_names_writing_editing() {
    assert_eq!(
        by_rules("proofread this paragraph").as_deref(),
        Some("writing-editing")
    );
}

#[test]
fn rewrite_names_writing_editing() {
    assert_eq!(
        by_rules("rewrite the intro to be shorter").as_deref(),
        Some("writing-editing")
    );
}

#[test]
fn should_we_names_judgement_planning() {
    assert_eq!(
        by_rules("should we move the queue to Postgres").as_deref(),
        Some("judgement-planning")
    );
}

#[test]
fn trade_off_names_judgement_planning() {
    assert_eq!(
        by_rules("a trade-off between speed and cost").as_deref(),
        Some("judgement-planning")
    );
}

#[test]
fn define_names_quick_lookup() {
    assert_eq!(
        by_rules("define idempotent").as_deref(),
        Some("quick-lookup")
    );
}

#[test]
fn what_is_names_quick_lookup() {
    assert_eq!(
        by_rules("what is a sky brightness probe?").as_deref(),
        Some("quick-lookup")
    );
}

#[test]
fn rename_names_small_code_change() {
    assert_eq!(
        by_rules("rename the config key to port").as_deref(),
        Some("small-code-change-exact-brief")
    );
}

#[test]
fn matching_ignores_case() {
    assert_eq!(
        by_rules("SEARCH THE WEB please").as_deref(),
        Some("web-research")
    );
    assert_eq!(
        by_rules("Proofread This").as_deref(),
        Some("writing-editing")
    );
}

#[test]
fn a_rule_does_not_fire_inside_a_longer_word() {
    assert_eq!(by_rules("a rewriter for logs"), None);
    assert_eq!(by_rules("the rewritten draft"), None);
    assert_eq!(by_rules("an undefined value"), None);
    assert_eq!(by_rules("whatever happens"), None);
    assert_eq!(by_rules("the what isn't here"), None);
}

#[test]
fn a_message_no_rule_matches_gives_none() {
    assert_eq!(by_rules("hello there"), None);
    assert_eq!(by_rules(""), None);
}

#[test]
fn a_rule_match_gives_its_kind_with_no_small_labeller() {
    assert_eq!(
        label("proofread this", &kinds(), None).as_deref(),
        Some("writing-editing")
    );
}

#[test]
fn a_rule_match_never_calls_the_small_labeller() {
    assert_eq!(
        label("proofread this", &kinds(), Some(&Panics)).as_deref(),
        Some("writing-editing")
    );
}

#[test]
fn no_rule_match_and_no_small_labeller_gives_none() {
    assert_eq!(label("hello there", &kinds(), None), None);
}

#[test]
fn no_rule_match_takes_the_small_labellers_kind_when_listed() {
    let small = Answers(Some("quick-lookup".to_string()));
    assert_eq!(
        label("hello there", &kinds(), Some(&small)).as_deref(),
        Some("quick-lookup")
    );
}

#[test]
fn no_rule_match_drops_a_small_labeller_kind_not_listed() {
    let small = Answers(Some("made-up-kind".to_string()));
    assert_eq!(label("hello there", &kinds(), Some(&small)), None);
}

#[test]
fn no_rule_match_and_a_silent_small_labeller_gives_none() {
    let small = Answers(None);
    assert_eq!(label("hello there", &kinds(), Some(&small)), None);
}

#[test]
fn a_rule_kind_the_index_does_not_list_is_dropped_without_asking_small() {
    let unlisted: Vec<String> = kinds()
        .into_iter()
        .filter(|kind| kind != "web-research")
        .collect();
    assert_eq!(label("search the web", &unlisted, Some(&Panics)), None);
}
