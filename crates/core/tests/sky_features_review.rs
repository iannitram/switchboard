//! R25 review probe for #29: behaviour the issue states that the locked tests do not cover.

use switchboard_core::sky::features::parse;

#[test]
fn the_first_updated_line_counts_even_after_the_features_block() {
    // Issue #29, Qwen brief point 2: `updated` is the first line that starts with `Updated:`,
    // wherever it is in the file.
    let block = parse("features:\n- a: A\n\nUpdated: 2026-11-08\n").unwrap();
    assert_eq!(block.updated, Some("2026-11-08".to_string()));
    assert_eq!(block.features.len(), 1);
}
