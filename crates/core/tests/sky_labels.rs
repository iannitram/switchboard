//! Locked tests for #35 (SKY-10: labels for the design pass and bugs). Written before the build
//! (R28); the builder never edits this file.

use switchboard_core::sky::labels::{bug_label, design_pass_label, VIEW_FEATURES};

fn labels(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_view_table_is_unchanged() {
    assert_eq!(
        VIEW_FEATURES,
        &[
            ("sky", "sky"),
            ("waiting", "waiting"),
            ("reports", "reports"),
            ("budget", "budget"),
            ("chat", "frontdoor"),
            ("settings", "core"),
            ("loadout", "it"),
        ]
    );
}

#[test]
fn one_view_gets_its_regions_feature() {
    assert_eq!(design_pass_label(&["sky"]), "feature:sky");
    assert_eq!(design_pass_label(&["chat"]), "feature:frontdoor");
    assert_eq!(design_pass_label(&["settings"]), "feature:core");
    assert_eq!(design_pass_label(&["loadout"]), "feature:it");
}

#[test]
fn every_view_in_the_table_maps_to_its_feature() {
    for (view, feature) in VIEW_FEATURES {
        assert_eq!(design_pass_label(&[*view]), format!("feature:{feature}"));
    }
}

#[test]
fn view_names_match_trimmed_and_in_any_case() {
    assert_eq!(design_pass_label(&["  Budget "]), "feature:budget");
    assert_eq!(design_pass_label(&["CHAT"]), "feature:frontdoor");
}

#[test]
fn the_same_view_named_twice_is_still_one_view() {
    assert_eq!(design_pass_label(&["sky", "Sky"]), "feature:sky");
}

#[test]
fn two_views_get_the_shell() {
    assert_eq!(design_pass_label(&["sky", "waiting"]), "feature:shell");
}

#[test]
fn an_unknown_view_or_no_view_gets_the_shell() {
    assert_eq!(design_pass_label(&["nav bar"]), "feature:shell");
    assert_eq!(design_pass_label(&["sky", "nav bar"]), "feature:shell");
    assert_eq!(design_pass_label(&[]), "feature:shell");
}

#[test]
fn a_bug_takes_its_stars_one_feature_label() {
    let star = labels(&["size:easy", "feature:budget", "kind:ui"]);
    assert_eq!(bug_label(&star), "feature:budget");
}

#[test]
fn a_star_with_no_feature_label_gives_the_shell() {
    assert_eq!(
        bug_label(&labels(&["size:easy", "kind:ui"])),
        "feature:shell"
    );
    assert_eq!(bug_label(&[]), "feature:shell");
}

#[test]
fn a_star_with_two_feature_labels_gives_the_shell() {
    let star = labels(&["feature:sky", "feature:budget"]);
    assert_eq!(bug_label(&star), "feature:shell");
}

#[test]
fn a_label_merely_containing_feature_is_not_one() {
    let star = labels(&["my-feature:sky", "feature:reports"]);
    assert_eq!(bug_label(&star), "feature:reports");
}
