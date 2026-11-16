//! #413: the dependency layers of a ROADMAP's `nodes:` block, as the sky answers them.
use serde_json::Value;
use switchboard_core::github::{Issue, RepoSnapshot};
use switchboard_core::sky::model::build;

const NODES: &str = "features:
- core: Core

nodes:
- a: First (feature: core, type: build, size: easy, after: none)
- b: Second (feature: core, type: build, size: easy, after: a)
- c: Third (feature: core, type: build, size: easy, after: a)
- d: Fourth (feature: core, type: build, size: easy, after: b c)
- e: Fifth (feature: core, type: build, size: easy, after: a d)
";

fn issue(number: u64, labels: &[&str]) -> Issue {
    Issue {
        number,
        state: "open".into(),
        labels: labels.iter().map(|label| label.to_string()).collect(),
        ..Default::default()
    }
}

/// The sky for one project, as the JSON the face reads.
fn sky_json(roadmap_md: &str, issues: Vec<Issue>) -> Value {
    let snap = RepoSnapshot {
        repo: "owner/demo".into(),
        roadmap_md: Some(roadmap_md.into()),
        issues,
        ..Default::default()
    };
    let sky = build(&[snap], None, &[], 0).expect("a bad nodes: block must not fail the sky");
    serde_json::to_value(&sky).unwrap()
}

/// Every star of the one project, in feature order (an orphan may repeat).
fn stars(json: &Value) -> Vec<&Value> {
    json["constellations"][0]["features"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|feature| feature["stars"].as_array().unwrap().iter())
        .collect()
}

#[test]
fn layers_follow_the_deepest_after_and_each_star_takes_its_nodes_layer() {
    let json = sky_json(
        NODES,
        vec![
            issue(1, &["feature:core", "node:a"]),
            issue(2, &["feature:core", "node:b"]),
            issue(3, &["feature:core", "node:c"]),
            issue(4, &["feature:core", "node:d"]),
            issue(5, &["feature:core", "node:e"]),
            issue(6, &["feature:core", "node:zz"]),
            issue(7, &["feature:core"]),
        ],
    );
    let c = &json["constellations"][0];
    // a is 0; b and c are 1; d is 2; e waits on d, so 3. Four columns.
    assert_eq!(c["nodeLayers"], 4);
    let layer = |number: u64| {
        stars(&json)
            .into_iter()
            .find(|star| star["number"] == number)
            .unwrap()
            .get("nodeLayer")
            .cloned()
    };
    assert_eq!(layer(1), Some(Value::from(0)));
    assert_eq!(layer(2), Some(Value::from(1)));
    assert_eq!(layer(3), Some(Value::from(1)));
    assert_eq!(layer(4), Some(Value::from(2)));
    assert_eq!(layer(5), Some(Value::from(3)));
    // An id outside the block, and no node label at all, leave no layer: the face falls back.
    assert_eq!(layer(6), None);
    assert_eq!(layer(7), None);
    // No star disappears.
    assert_eq!(stars(&json).len(), 7);
}

#[test]
fn a_project_without_a_usable_nodes_block_sends_no_layer_keys() {
    let cases = [
        ("no nodes block", ""),
        ("an empty nodes block", "nodes:\n"),
        (
            "a cycle",
            "nodes:\n- a: A (feature: core, type: build, size: easy, after: b)\n- b: B (feature: core, type: build, size: easy, after: a)\n",
        ),
        (
            "a node after itself",
            "nodes:\n- a: A (feature: core, type: build, size: easy, after: a)\n",
        ),
        (
            "an after naming no node",
            "nodes:\n- a: A (feature: core, type: build, size: easy, after: nope)\n",
        ),
        (
            "a repeated id",
            "nodes:\n- a: A (feature: core, type: build, size: easy, after: none)\n- a: Again (feature: core, type: build, size: easy, after: none)\n",
        ),
        (
            "a line that does not parse",
            "nodes:\n- a: A (feature: core, type: build, size: huge, after: none)\n",
        ),
    ];
    for (name, nodes) in cases {
        let roadmap = format!("features:\n- core: Core\n\n{nodes}");
        let json = sky_json(&roadmap, vec![issue(1, &["feature:core", "node:a"])]);
        let c = &json["constellations"][0];
        assert!(c.get("nodeLayers").is_none(), "{name}: nodeLayers sent");
        let star = stars(&json);
        assert_eq!(star.len(), 1, "{name}: the star must still be drawn");
        assert!(star[0].get("nodeLayer").is_none(), "{name}: nodeLayer sent");
    }
}
