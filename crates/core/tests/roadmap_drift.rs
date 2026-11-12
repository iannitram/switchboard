//! roadmap-drift (#400): the drift between ROADMAP.md and the issues. A node with no issue, an
//! issue with no node, a roadmap whose `Updated:` date is stale, and an unspecified item whose
//! waits-on nodes are all done. A roadmap in step with its issues gives no items.

use switchboard_core::github::Issue;
use switchboard_core::roadmap::drift::{self, DriftItem, Kind};

/// `today` for every test: the roadmap's `Updated:` line is this date unless a test says otherwise.
const TODAY: &str = "2026-11-12";

/// Two nodes, `a` and `b`, and two unspecified items that wait on them.
const ROADMAP: &str = "Updated: 2026-11-12\n\
\n\
features:\n\
- runner: The runner (release: 1.0)\n\
\n\
nodes:\n\
- a: First (feature: runner, type: build, size: easy, after: none)\n\
- b: Second (feature: runner, type: build, size: easy, after: a)\n\
\n\
unspecified:\n\
- c: Gist (waits on: a)\n\
- d: Gist (waits on: a b)\n";

fn issue(number: u64, state: &str, labels: &[&str]) -> Issue {
    Issue {
        number,
        title: format!("issue {number}"),
        state: state.to_string(),
        labels: labels.iter().map(|label| label.to_string()).collect(),
        milestone: None,
        body: String::new(),
        closed_by_pr: None,
    }
}

/// Every item as `(kind, ref)`, in the order `find` reports them.
fn pairs(roadmap_md: &str, issues: &[Issue]) -> Vec<(Kind, String)> {
    drift::find(roadmap_md, issues, TODAY)
        .expect("drift reads the roadmap")
        .into_iter()
        .map(|item| (item.kind, item.reference))
        .collect()
}

#[test]
fn a_roadmap_in_step_with_its_issues_gives_no_items() {
    let issues = [issue(1, "open", &["node:a"]), issue(2, "open", &["node:b"])];
    assert_eq!(pairs(ROADMAP, &issues), Vec::new());
}

#[test]
fn a_node_with_no_issue_is_named_by_its_id() {
    let issues = [issue(1, "open", &["node:a"])];
    assert_eq!(
        pairs(ROADMAP, &issues),
        vec![(Kind::NodeWithoutUnit, "b".to_string())]
    );
}

#[test]
fn a_closed_issue_still_counts_as_the_nodes_issue() {
    let issues = [
        issue(1, "open", &["node:a"]),
        issue(2, "closed", &["node:b"]),
    ];
    assert_eq!(pairs(ROADMAP, &issues), Vec::new());
}

#[test]
fn an_issue_with_no_node_is_named_by_its_number() {
    let issues = [
        issue(1, "open", &["node:a"]),
        issue(2, "open", &["node:b"]),
        issue(9, "open", &["feature:runner"]),
        issue(10, "open", &["node:gone"]),
    ];
    assert_eq!(
        pairs(ROADMAP, &issues),
        vec![
            (Kind::UnitWithoutNode, "#9".to_string()),
            (Kind::UnitWithoutNode, "#10".to_string()),
        ]
    );
}

#[test]
fn a_roadmap_past_its_stale_date_is_drift() {
    let roadmap = ROADMAP.replacen("Updated: 2026-11-12", "Updated: 2026-10-01", 1);
    let issues = [issue(1, "open", &["node:a"]), issue(2, "open", &["node:b"])];
    assert_eq!(
        pairs(&roadmap, &issues),
        vec![(Kind::StaleDate, "ROADMAP.md".to_string())]
    );
}

#[test]
fn a_roadmap_fourteen_days_old_is_not_stale() {
    let roadmap = ROADMAP.replacen("Updated: 2026-11-12", "Updated: 2026-10-29", 1);
    let issues = [issue(1, "open", &["node:a"]), issue(2, "open", &["node:b"])];
    assert_eq!(pairs(&roadmap, &issues), Vec::new());
}

#[test]
fn a_roadmap_with_no_updated_line_is_stale() {
    let roadmap = ROADMAP.replacen("Updated: 2026-11-12\n", "", 1);
    let issues = [issue(1, "open", &["node:a"]), issue(2, "open", &["node:b"])];
    assert_eq!(
        pairs(&roadmap, &issues),
        vec![(Kind::StaleDate, "ROADMAP.md".to_string())]
    );
}

#[test]
fn an_unspecified_item_is_fog_ready_when_all_its_waits_on_nodes_are_done() {
    // `a` is done; `b` is still open, so `d` (waits on a and b) is not ready, but `c` is.
    let issues = [
        issue(1, "closed", &["node:a"]),
        issue(2, "open", &["node:b"]),
    ];
    assert_eq!(
        pairs(ROADMAP, &issues),
        vec![(Kind::FogReady, "c".to_string())]
    );
}

#[test]
fn every_item_names_its_kind_and_its_id_in_json() {
    let roadmap = ROADMAP.replacen("Updated: 2026-11-12", "Updated: 2026-10-01", 1);
    let issues = [issue(1, "closed", &["node:a"]), issue(9, "open", &[])];
    let items: Vec<DriftItem> = drift::find(&roadmap, &issues, TODAY).expect("drift reads");
    assert!(!items.is_empty());
    for item in &items {
        assert!(!item.reference.is_empty(), "{item:?} names no id");
        assert!(!item.message.is_empty(), "{item:?} says nothing");
        let json = serde_json::to_value(item).expect("a drift item serialises");
        assert!(json["kind"].is_string(), "{json} has no kind");
        assert!(json["ref"].is_string(), "{json} has no ref");
    }
    let kinds: Vec<String> = items
        .iter()
        .map(|item| {
            serde_json::to_value(item).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            "node-without-unit",
            "unit-without-node",
            "stale-date",
            "fog-ready",
        ]
    );
}
