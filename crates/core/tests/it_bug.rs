//! Tests for IT-4 (a ladder-out coding error becomes a bug in the project's repo, labelled `bug`
//! and the star's one `feature:` label, never an IT ticket). `ladder_out_bug` only builds the
//! create-queue item, so these tests need no fleet and never call `fleet ghq`.

use switchboard_core::it::ladder_out_bug;

const REPO: &str = "owner/project";
const TITLE: &str = "Coding error: ladder ran out";

fn labels(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn a_star_with_one_feature_label_gives_a_bug_on_that_feature() {
    let item = ladder_out_bug(REPO, 42, TITLE, &labels(&["feature:sky"])).expect("one feature");
    assert_eq!(item["kind"], "issue");
    assert_eq!(item["repo"], REPO);
    assert_eq!(item["title"], TITLE);
    assert_eq!(item["labels"], serde_json::json!(["bug", "feature:sky"]));
    assert!(item["milestone"].is_null());
}

#[test]
fn the_body_links_the_star_issue() {
    let item = ladder_out_bug(REPO, 42, TITLE, &labels(&["feature:sky"])).expect("one feature");
    let body = item["body"].as_str().expect("a body");
    assert!(body.contains("#42"), "body was: {body}");
}

#[test]
fn no_it_label_appears_for_any_input() {
    let stars = [
        labels(&["feature:sky"]),
        labels(&["it", "feature:sky"]),
        labels(&["feature:it"]),
        labels(&["feature:it", "it"]),
        labels(&["it"]),
        labels(&["bug", "feature:waiting", "it"]),
    ];
    for star in &stars {
        if let Some(item) = ladder_out_bug(REPO, 7, TITLE, star) {
            let names = item["labels"].as_array().expect("labels");
            assert!(
                names.iter().all(|n| n.as_str() != Some("it")),
                "labels were: {names:?}"
            );
        }
    }
}

#[test]
fn a_star_with_no_feature_label_gives_none() {
    assert!(ladder_out_bug(REPO, 42, TITLE, &labels(&[])).is_none());
    assert!(ladder_out_bug(REPO, 42, TITLE, &labels(&["it", "bug"])).is_none());
}

#[test]
fn a_star_with_two_feature_labels_gives_none() {
    let star = labels(&["feature:sky", "feature:waiting"]);
    assert!(ladder_out_bug(REPO, 42, TITLE, &star).is_none());
}

#[test]
fn a_bare_feature_prefix_is_not_a_feature_label() {
    assert!(ladder_out_bug(REPO, 42, TITLE, &labels(&["feature:"])).is_none());
}
