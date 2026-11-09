//! Locked tests for #36 (SKY-6: read the reuses block of a code map). Written before the build
//! (R28); the builder never edits this file.

use switchboard_core::sky::codemap::{is_warning, reuses, Reuse};

const CODE_MAP: &str = "# Code map

Updated: 2026-11-08

reuses:
- brocrew: the chat parts and the fixed report (Apache-2.0)
- nc-outliner:   the outline tree
- wavcraft
After the block.
- late: not in the block

## Layout
";

fn reuse(repo: &str, what: &str) -> Reuse {
    Reuse {
        repo: repo.to_string(),
        what: what.to_string(),
    }
}

#[test]
fn reads_every_line_of_the_block_in_order_trimmed() {
    assert_eq!(
        reuses(CODE_MAP),
        vec![
            reuse(
                "brocrew",
                "the chat parts and the fixed report (Apache-2.0)"
            ),
            reuse("nc-outliner", "the outline tree"),
            reuse("wavcraft", ""),
        ]
    );
}

#[test]
fn what_is_split_at_the_first_colon_space_only() {
    assert_eq!(
        reuses("reuses:\n- brocrew: parts: the composer\n"),
        vec![reuse("brocrew", "parts: the composer")]
    );
}

#[test]
fn the_block_ends_at_the_first_line_not_starting_with_dash_space() {
    assert!(reuses(CODE_MAP).iter().all(|r| r.repo != "late"));
    assert_eq!(reuses("reuses:\n- a: A\n\n- b: B\n"), vec![reuse("a", "A")]);
}

#[test]
fn no_reuses_line_gives_an_empty_list() {
    assert!(reuses("# Code map\n\n- brocrew: not a block\n").is_empty());
    assert!(reuses("").is_empty());
}

#[test]
fn an_indented_or_longer_reuses_line_does_not_start_the_block() {
    assert!(reuses("  reuses:\n- a: A\n").is_empty());
    assert!(reuses("reuses: none\n- a: A\n").is_empty());
}

#[test]
fn the_repo_code_map_names_brocrew() {
    let text = include_str!("../../../docs/code-map.md");
    let found = reuses(text);
    assert!(found.iter().any(|r| r.repo == "brocrew"), "{found:?}");
}

#[test]
fn an_archived_repo_is_a_warning() {
    assert!(is_warning(true, None));
    assert!(is_warning(true, Some("Dev Loop")));
}

#[test]
fn a_parked_repo_is_a_warning_whatever_the_case() {
    assert!(is_warning(false, Some("Parked")));
    assert!(is_warning(false, Some(" parked ")));
}

#[test]
fn a_live_repo_is_not_a_warning() {
    assert!(!is_warning(false, None));
    assert!(!is_warning(false, Some("Dev Loop")));
    assert!(!is_warning(false, Some("Prototype / Beta")));
}
