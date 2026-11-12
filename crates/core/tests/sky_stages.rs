//! #392: development stages and their issue counts, through the reader and sky model.
use switchboard_core::github::{Issue, RepoSnapshot};
use switchboard_core::sky::{features::parse_stages, model::build};

const TABLE: &str = "## Development stages

| # | Stage | Features | Waypoint (SPEC.md) |
|---|---|---|---|
| 2 | **The read views**, in parallel. | `sky`, `cli` | WP2 |
| 1 | **Core first.** Settings and the bridge. | `core` | - |
| 3 | **The design pass** on the running build. | all with a view | WP3 |
";

#[test]
fn normal_table_reads_numbers_names_and_feature_ids_in_stage_order() {
    let stages = parse_stages(TABLE).unwrap();
    assert_eq!(
        stages.iter().map(|s| s.number).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(stages[0].name, "Core first.");
    assert_eq!(stages[0].features, ["core"]);
    assert_eq!(stages[1].name, "The read views");
    assert_eq!(stages[1].features, ["sky", "cli"]);
    assert!(stages[2].features.is_empty());
}

#[test]
fn a_feature_in_two_stages_belongs_to_the_first_stage_only() {
    let stages =
        parse_stages(&TABLE.replace("`sky`, `cli`", "`core`, `sky`, `cli`, `sky`")).unwrap();
    assert_eq!(stages[0].features, ["core"]);
    assert_eq!(stages[1].features, ["sky", "cli"]);
}

#[test]
fn missing_table_is_none_including_a_heading_without_a_table() {
    for text in [
        "",
        "features:\n- core: Core\n",
        "## Development stages\nLater.\n## Other\n| # | Stage | Features |\n",
    ] {
        assert_eq!(parse_stages(text), None);
    }
}

#[test]
fn fenced_examples_and_tables_in_other_sections_are_not_stages() {
    assert_eq!(
        parse_stages(&format!("```md\n{TABLE}\n```\n## Other\n{TABLE}"))
            .unwrap()
            .len(),
        3
    );
    assert_eq!(parse_stages(&format!("~~~md\n{TABLE}\n~~~")), None);
    assert_eq!(
        parse_stages(&TABLE.replace("Development stages", "Features")),
        None
    );
}

#[test]
fn the_real_roadmap_reads_its_stages_and_resolves_the_repeated_it_feature() {
    let stages = parse_stages(include_str!("../../../ROADMAP.md")).unwrap();
    assert_eq!(stages.len(), 8);
    assert!(stages[5].features.contains(&"it".to_string()));
    assert!(!stages[6].features.contains(&"it".to_string()));
}

#[test]
fn sky_counts_unique_issues_and_marks_only_nonempty_closed_stages_complete() {
    let mut snap = RepoSnapshot {
        repo: "owner/demo".into(),
        roadmap_md: Some(format!(
            "features:\n- core: Core\n- sky: Sky\n- cli: CLI\n\n{TABLE}"
        )),
        issues: vec![
            Issue {
                number: 1,
                state: "closed".into(),
                labels: vec!["feature:core".into()],
                ..Default::default()
            },
            Issue {
                number: 2,
                state: "open".into(),
                labels: vec!["feature:sky".into(), "feature:cli".into()],
                body: "Blocked by #1".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let sky = build(&[snap.clone()], None, &[], 0).unwrap();
    let c = &sky.constellations[0];
    let stages = c.stages.as_ref().unwrap();
    assert_eq!(
        (stages[0].done, stages[0].total, stages[0].complete),
        (1, 1, true)
    );
    assert_eq!(
        (stages[1].done, stages[1].total, stages[1].complete),
        (0, 1, false)
    );
    assert_eq!(
        (stages[2].done, stages[2].total, stages[2].complete),
        (0, 0, false)
    );
    assert_eq!(c.edges, [[1, 2]]);
    let json = serde_json::to_value(c).unwrap();
    assert_eq!(json["stages"][0]["name"], "Core first.");
    snap.issues[1].state = "closed".into();
    assert!(
        build(&[snap], None, &[], 0).unwrap().constellations[0]
            .stages
            .as_ref()
            .unwrap()[1]
            .complete
    );
}

#[test]
fn missing_stages_preserve_the_legacy_sky_shape() {
    let snap = RepoSnapshot {
        repo: "owner/demo".into(),
        ..Default::default()
    };
    let sky = build(&[snap], None, &[], 0).unwrap();
    assert!(sky.constellations[0].stages.is_none());
    assert!(serde_json::to_value(&sky.constellations[0])
        .unwrap()
        .get("stages")
        .is_none());
}
