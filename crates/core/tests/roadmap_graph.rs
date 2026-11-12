//! roadmap-graph (#398): ROADMAP.md's `nodes:` and `unspecified:` blocks, each node's spec file
//! under `roadmap/nodes/`, and the lint that reports every problem by id.

use std::path::{Path, PathBuf};

use switchboard_core::roadmap::{self, Finding, LintReport};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A throwaway clone: ROADMAP.md plus one spec file per `(id, json)`.
fn fixture(name: &str, roadmap_md: &str, specs: &[(&str, String)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("roadmap-graph-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("roadmap/nodes")).unwrap();
    std::fs::write(dir.join("ROADMAP.md"), roadmap_md).unwrap();
    for (id, json) in specs {
        std::fs::write(dir.join(format!("roadmap/nodes/{id}.json")), json).unwrap();
    }
    dir
}

/// A complete build node's spec file.
fn build_spec(id: &str) -> String {
    format!(
        r#"{{"id":"{id}","what":"w","expectations":["e"],"files_allowed":["a.rs"],"test_files":["t.rs"],"verify":[{{"name":"tests","cmd":"cargo test","expect":"pass"}}],"code_map_entries":["a.rs"]}}"#
    )
}

const ROADMAP: &str = "Updated: 2026-11-12\n\
\n\
features:\n\
- runner: The runner (release: after-1.0)\n\
\n\
nodes:\n\
- a: First (feature: runner, type: build, size: easy, after: none)\n\
- b: Second (feature: runner, type: build, size: easy, after: a)\n\
\n\
unspecified:\n\
- c: Gist (waits on: a)\n";

fn has(findings: &[Finding], check: &str, id: &str) -> bool {
    findings.iter().any(|f| f.check == check && f.id == id)
}

fn lint_fixture(name: &str, roadmap_md: &str, specs: &[(&str, String)]) -> LintReport {
    let dir = fixture(name, roadmap_md, specs);
    roadmap::lint(&dir).expect("lint reads the fixture")
}

#[test]
fn repo_roadmap_parses_nodes_and_unspecified() {
    let text = std::fs::read_to_string(repo_root().join("ROADMAP.md")).unwrap();
    let graph = roadmap::graph::parse(&text).expect("ROADMAP.md parses");
    assert_eq!(graph.nodes.len(), 19);
    let first = &graph.nodes[0];
    assert_eq!(first.id, "runner-spec");
    assert_eq!(first.feature, "runner");
    assert_eq!(first.size, roadmap::graph::Size::Medium);
    assert_eq!(first.node_type, roadmap::graph::NodeType::Build);
    assert!(first.after.is_empty());
    let fleet = graph
        .nodes
        .iter()
        .find(|n| n.id == "fleet-state-write")
        .unwrap();
    assert_eq!(fleet.node_type, roadmap::graph::NodeType::Research);
    assert_eq!(graph.unspecified.len(), 3);
    assert_eq!(graph.unspecified[0].id, "approval-lines");
    assert_eq!(
        graph.unspecified[0].waits_on,
        vec!["runner-waiting".to_string()]
    );
}

#[test]
fn repo_nodes_block_lints_clean() {
    let report = roadmap::lint(&repo_root()).expect("lint reads the repo");
    assert!(report.errors.is_empty(), "repo errors: {:?}", report.errors);
    assert!(report.ok());
}

#[test]
fn valid_fixture_lints_clean() {
    let specs = [("a", build_spec("a")), ("b", build_spec("b"))];
    let report = lint_fixture("valid", ROADMAP, &specs);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn dependency_cycle_is_reported_by_id() {
    let md = ROADMAP.replace(
        "- a: First (feature: runner, type: build, size: easy, after: none)",
        "- a: First (feature: runner, type: build, size: easy, after: b)",
    );
    let specs = [("a", build_spec("a")), ("b", build_spec("b"))];
    let report = lint_fixture("cycle", &md, &specs);
    assert!(has(&report.errors, "cycle", "a"), "{:?}", report.errors);
    assert!(has(&report.errors, "cycle", "b"), "{:?}", report.errors);
    assert!(!report.ok());
}

#[test]
fn unknown_after_id_is_reported_by_id() {
    let md = ROADMAP.replace("after: a)", "after: ghost)");
    let specs = [("a", build_spec("a")), ("b", build_spec("b"))];
    let report = lint_fixture("unknown-after", &md, &specs);
    let finding = report
        .errors
        .iter()
        .find(|f| f.check == "unknown_after" && f.id == "b")
        .unwrap_or_else(|| panic!("no unknown_after for b: {:?}", report.errors));
    assert!(finding.message.contains("ghost"));
}

#[test]
fn duplicate_id_is_reported_by_id() {
    let md = ROADMAP.replace(
        "- b: Second (feature: runner, type: build, size: easy, after: a)",
        "- a: Again (feature: runner, type: build, size: easy, after: none)",
    );
    let specs = [("a", build_spec("a")), ("b", build_spec("b"))];
    let report = lint_fixture("duplicate", &md, &specs);
    assert!(
        has(&report.errors, "duplicate_id", "a"),
        "{:?}",
        report.errors
    );
}

#[test]
fn node_without_spec_file_is_reported_by_id() {
    let specs = [("a", build_spec("a"))];
    let report = lint_fixture("no-spec", ROADMAP, &specs);
    assert!(has(&report.errors, "spec_file", "b"), "{:?}", report.errors);
    assert!(!has(&report.errors, "spec_file", "a"));
}

#[test]
fn build_node_missing_required_fields_is_reported() {
    let bare = r#"{"id":"a","what":"w","expectations":["e"]}"#.to_string();
    let specs = [("a", bare), ("b", build_spec("b"))];
    let report = lint_fixture("missing-fields", ROADMAP, &specs);
    for field in ["verify", "files_allowed", "test_files", "code_map_entries"] {
        assert!(
            report
                .errors
                .iter()
                .any(|f| f.check == "missing_field" && f.id == "a" && f.message.contains(field)),
            "no missing_field for {field}: {:?}",
            report.errors
        );
    }
    assert!(!has(&report.errors, "missing_field", "b"));
}

#[test]
fn feature_without_node_is_reported_as_a_warning() {
    let md = ROADMAP.replace(
        "- runner: The runner (release: after-1.0)\n",
        "- runner: The runner (release: after-1.0)\n- lonely: Nothing yet (release: 1.0)\n",
    );
    let specs = [("a", build_spec("a")), ("b", build_spec("b"))];
    let report = lint_fixture("feature-warning", &md, &specs);
    assert!(
        has(&report.warnings, "feature_without_node", "lonely"),
        "{:?}",
        report.warnings
    );
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.ok());
}

#[test]
fn unknown_wait_in_unspecified_is_reported_by_id() {
    let md = ROADMAP.replace("(waits on: a)", "(waits on: ghost)");
    let specs = [("a", build_spec("a")), ("b", build_spec("b"))];
    let report = lint_fixture("unknown-wait", &md, &specs);
    assert!(
        has(&report.errors, "unknown_wait", "c"),
        "{:?}",
        report.errors
    );
}
