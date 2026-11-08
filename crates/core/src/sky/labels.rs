//! SKY-10: issues the core files itself never show up as orphan stars.

/// The views and the feature whose region owns each.
pub const VIEW_FEATURES: &[(&str, &str)] = &[
    ("sky", "sky"),
    ("waiting", "waiting"),
    ("reports", "reports"),
    ("budget", "budget"),
    ("chat", "frontdoor"),
    ("settings", "core"),
    ("loadout", "it"),
];

/// SKY-10: a design-pass issue gets the `feature:<id>` of the region whose view it changes;
/// one that spans views (or names a view not in `VIEW_FEATURES`) gets `feature:shell`.
pub fn design_pass_label(views: &[&str]) -> String {
    let mut features: Vec<&str> = Vec::new();
    for view in views {
        let name = view.trim();
        match VIEW_FEATURES
            .iter()
            .find(|&&(v, _)| v.eq_ignore_ascii_case(name))
        {
            Some(&(_, feature)) => {
                if !features.contains(&feature) {
                    features.push(feature);
                }
            }
            None => return "feature:shell".to_string(),
        }
    }
    match features.as_slice() {
        [only] => format!("feature:{only}"),
        _ => "feature:shell".to_string(),
    }
}

/// SKY-10, IT-4: an automatic bug gets the `feature:` label of the star whose job ran out of
/// ladder: the one `feature:` label in `star_labels`, else `feature:shell`.
pub fn bug_label(star_labels: &[String]) -> String {
    let feature_labels: Vec<&String> = star_labels
        .iter()
        .filter(|label| label.starts_with("feature:"))
        .collect();
    match feature_labels.as_slice() {
        [only] => (*only).clone(),
        _ => "feature:shell".to_string(),
    }
}
