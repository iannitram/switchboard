//! unit-proof (#405): `unit::proof::red_check` judges a run of a unit's test command, and
//! `sky::codemap::check_modules` checks a change's files against the code map's `modules:`
//! lines. Written before the build (R28); the builder never edits this file.

use switchboard_core::sky::codemap::{check_modules, modules, Finding, Touched};
use switchboard_core::unit::proof::{red_check, Verdict};

const CARGO_FAIL: &str = "\
running 2 tests
test a ... ok
test b ... FAILED

failures:

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
";

const CARGO_PASS: &str = "\
running 1 test
test a ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
";

const CARGO_NONE: &str = "\
running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
";

const COMPILE_ERROR: &str = "\
error[E0432]: unresolved import `switchboard_core::unit`
error: could not compile `switchboard-core` (test \"unit_proof\") due to 1 previous error
";

#[test]
fn a_failing_test_is_red() {
    assert_eq!(red_check(Some(101), CARGO_FAIL), Verdict::Red { failed: 1 });
    assert!(red_check(Some(101), CARGO_FAIL).is_red());
}

#[test]
fn results_of_every_test_binary_are_summed() {
    let two = format!("{CARGO_PASS}\n{CARGO_FAIL}");
    assert_eq!(red_check(Some(101), &two), Verdict::Red { failed: 1 });
}

#[test]
fn a_passing_test_is_not_red() {
    assert_eq!(red_check(Some(0), CARGO_PASS), Verdict::Passed);
    assert!(!red_check(Some(0), CARGO_PASS).is_red());
}

#[test]
fn a_failed_command_whose_tests_all_passed_is_not_red() {
    assert_eq!(red_check(Some(101), CARGO_PASS), Verdict::Passed);
}

#[test]
fn no_test_running_is_not_red() {
    assert_eq!(red_check(Some(0), CARGO_NONE), Verdict::NoTestRan);
}

#[test]
fn ignored_tests_do_not_count_as_running() {
    let ignored = "test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out\n";
    assert_eq!(red_check(Some(0), ignored), Verdict::NoTestRan);
}

#[test]
fn a_compile_error_is_not_red() {
    assert_eq!(red_check(Some(101), COMPILE_ERROR), Verdict::NoTestRan);
}

#[test]
fn a_command_that_did_not_run_is_not_red() {
    assert_eq!(red_check(None, CARGO_FAIL), Verdict::DidNotRun);
}

const BASE: &str = "# Code map

Updated: 2026-11-08

modules:
- unit-proof: crates/core/src/unit/proof.rs (planned)
- unit-proof: crates/core/src/sky/codemap.rs (planned)
- other-node: crates/core/src/other.rs (planned)

## Layout
";

/// The change's head: the node's lines present and the map's date moved on.
const DONE: &str = "# Code map

Updated: 2026-11-12

modules:
- unit-proof: crates/core/src/unit/proof.rs (present)
- unit-proof: crates/core/src/sky/codemap.rs (present)
- other-node: crates/core/src/other.rs (planned)

## Layout
";

fn added(path: &str) -> Touched {
    Touched {
        path: path.to_string(),
        added: true,
    }
}

fn modified(path: &str) -> Touched {
    Touched {
        path: path.to_string(),
        added: false,
    }
}

fn has(findings: &[Finding], check: &str, path: &str) -> bool {
    findings.iter().any(|f| f.check == check && f.path == path)
}

#[test]
fn modules_lines_read_node_path_and_status() {
    let found = modules(BASE);
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].node, "unit-proof");
    assert_eq!(found[0].path, "crates/core/src/unit/proof.rs");
    assert_eq!(found[0].status, "planned");
    assert_eq!(found[2].node, "other-node");
}

#[test]
fn a_new_file_with_no_modules_line_fails() {
    let touched = [added("crates/core/src/unit/claim.rs")];
    let findings = check_modules("unit-proof", &touched, BASE, DONE);
    assert!(
        has(
            &findings,
            "no_modules_line",
            "crates/core/src/unit/claim.rs"
        ),
        "{findings:?}"
    );
}

#[test]
fn a_new_file_under_a_directory_line_passes() {
    let head = DONE.replace(
        "- unit-proof: crates/core/src/unit/proof.rs (present)",
        "- unit-proof: crates/core/src/unit/ (present)",
    );
    let touched = [
        added("crates/core/src/unit/claim.rs"),
        modified("docs/code-map.md"),
    ];
    let findings = check_modules("unit-proof", &touched, BASE, &head);
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_planned_to_present_change_with_a_moved_date_passes() {
    let touched = [
        added("crates/core/src/unit/proof.rs"),
        modified("crates/core/src/sky/codemap.rs"),
        modified("docs/code-map.md"),
    ];
    let findings = check_modules("unit-proof", &touched, BASE, DONE);
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_file_under_a_line_still_planned_fails_the_status() {
    let touched = [
        added("crates/core/src/unit/proof.rs"),
        modified("docs/code-map.md"),
    ];
    let findings = check_modules(
        "unit-proof",
        &touched,
        BASE,
        &BASE.replace("2026-11-08", "2026-11-12"),
    );
    assert!(
        has(&findings, "status_planned", "crates/core/src/unit/proof.rs"),
        "{findings:?}"
    );
}

#[test]
fn a_covered_change_without_the_code_map_fails_the_date() {
    let touched = [added("crates/core/src/unit/proof.rs")];
    let findings = check_modules("unit-proof", &touched, BASE, DONE);
    assert!(
        has(
            &findings,
            "code_map_not_updated",
            "crates/core/src/unit/proof.rs"
        ),
        "{findings:?}"
    );
}

#[test]
fn a_code_map_date_older_than_the_base_fails() {
    let head = DONE.replace("2026-11-12", "2026-11-01");
    let touched = [
        added("crates/core/src/unit/proof.rs"),
        modified("docs/code-map.md"),
    ];
    let findings = check_modules("unit-proof", &touched, BASE, &head);
    assert!(
        has(
            &findings,
            "code_map_not_updated",
            "crates/core/src/unit/proof.rs"
        ),
        "{findings:?}"
    );
}

#[test]
fn a_change_outside_the_node_needs_nothing() {
    let touched = [modified("README.md")];
    let findings = check_modules("unit-proof", &touched, BASE, BASE);
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn the_repo_code_map_has_both_unit_proof_files_present() {
    let text = include_str!("../../../docs/code-map.md");
    for path in [
        "crates/core/src/unit/proof.rs",
        "crates/core/src/sky/codemap.rs",
    ] {
        assert!(
            modules(text)
                .iter()
                .any(|m| m.node == "unit-proof" && m.path == path && m.status == "present"),
            "{path} is not a present unit-proof module in docs/code-map.md"
        );
    }
}
