//! Locked tests for #29 (sky: parse the ROADMAP features block). Written before the build
//! (R28); the builder never edits this file.

use switchboard_core::sky::features::{parse, Feature, FeatureBlock, Release};
use switchboard_core::CoreError;

const ROADMAP: &str = "# Roadmap: Demo

Updated: 2026-11-08

features:
- core: The core and the fleet bridge (release: 1.0)
- sky: Constellation (release: 1.0)
- relay: A headless relay per machine (release: after-1.0)
- gamification: A gamification plugin (release: after-2.0)
- fleet-rust2: Porting fleet to Rust
After the block.
- late: not in the block (release: 1.0)
";

fn feature(id: &str, title: &str, release: Release) -> Feature {
    Feature {
        id: id.to_string(),
        title: title.to_string(),
        release,
    }
}

fn parse_error_text(text: &str) -> String {
    match parse(text) {
        Err(CoreError::Parse(why)) => why,
        other => panic!("expected CoreError::Parse, got {other:?}"),
    }
}

#[test]
fn reads_updated_and_every_line_of_the_block_in_order() {
    let block = parse(ROADMAP).unwrap();
    assert_eq!(
        block,
        FeatureBlock {
            updated: Some("2026-11-08".to_string()),
            features: vec![
                feature("core", "The core and the fleet bridge", Release::V1),
                feature("sky", "Constellation", Release::V1),
                feature("relay", "A headless relay per machine", Release::After1),
                feature("gamification", "A gamification plugin", Release::After2),
                feature("fleet-rust2", "Porting fleet to Rust", Release::V1),
            ],
        }
    );
}

#[test]
fn a_line_without_a_release_marker_is_release_1_0() {
    let block = parse("features:\n- cli: The switchboard CLI\n").unwrap();
    assert_eq!(
        block.features,
        vec![feature("cli", "The switchboard CLI", Release::V1)]
    );
}

#[test]
fn the_block_ends_at_the_first_line_not_starting_with_dash_space() {
    let block = parse(ROADMAP).unwrap();
    assert!(block.features.iter().all(|f| f.id != "late"));
    let blank_ends_it = parse("features:\n- a: A\n\n- b: B\n").unwrap();
    assert_eq!(blank_ends_it.features, vec![feature("a", "A", Release::V1)]);
}

#[test]
fn no_features_line_gives_an_empty_list() {
    let block = parse("# Roadmap\n\nUpdated: 2026-10-01\n\n- core: not a block\n").unwrap();
    assert_eq!(block.updated, Some("2026-10-01".to_string()));
    assert!(block.features.is_empty());
}

#[test]
fn no_updated_line_gives_none() {
    let block = parse("features:\n- core: Core (release: 1.0)\n").unwrap();
    assert_eq!(block.updated, None);
    assert_eq!(block.features.len(), 1);
}

#[test]
fn updated_is_kept_as_written_after_trimming() {
    let block = parse("Updated:   2026-11-08  \nfeatures:\n").unwrap();
    assert_eq!(block.updated, Some("2026-11-08".to_string()));
}

#[test]
fn a_bad_id_is_a_parse_error_naming_its_line() {
    let why = parse_error_text("# R\n\nfeatures:\n- core: Core\n- Bad_Id: Shouty (release: 1.0)\n");
    assert_eq!(why, "ROADMAP.md line 5: - Bad_Id: Shouty (release: 1.0)");
}

#[test]
fn a_line_with_no_colon_space_is_a_parse_error() {
    let why = parse_error_text("features:\n- core The core\n");
    assert_eq!(why, "ROADMAP.md line 2: - core The core");
}

#[test]
fn an_empty_id_or_title_is_a_parse_error() {
    assert_eq!(
        parse_error_text("features:\n- : No id\n"),
        "ROADMAP.md line 2: - : No id"
    );
    assert_eq!(
        parse_error_text("features:\n- core: (release: 1.0)\n"),
        "ROADMAP.md line 2: - core: (release: 1.0)"
    );
}

#[test]
fn an_unknown_release_value_is_a_parse_error() {
    let why = parse_error_text("features:\n- core: Core (release: 2.0)\n");
    assert_eq!(why, "ROADMAP.md line 2: - core: Core (release: 2.0)");
}

#[test]
fn the_repo_roadmap_parses() {
    let text = include_str!("../../../ROADMAP.md");
    let block = parse(text).unwrap();
    assert!(block.updated.is_some());
    assert!(block
        .features
        .contains(&feature("sky", "Constellation", Release::V1)));
    assert!(block
        .features
        .iter()
        .any(|f| f.id == "relay" && f.release == Release::After1));
    assert!(block
        .features
        .iter()
        .any(|f| f.id == "gamification" && f.release == Release::After2));
}
