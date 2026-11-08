//! Locked tests for #33 (SKY-7: blocked-by lines and the longest chain). Written before the
//! build (R28); the builder never edits this file.

use switchboard_core::github::Issue;
use switchboard_core::sky::chain::{edges, longest};

fn issue(number: u64, state: &str, body: &str) -> Issue {
    Issue {
        number,
        title: format!("issue {number}"),
        state: state.to_string(),
        body: body.to_string(),
        ..Default::default()
    }
}

/// Open issues `n` with `Blocked by #b` for each (n, b).
fn graph(open: &[u64], closed: &[u64], blocked_by: &[(u64, u64)]) -> Vec<Issue> {
    let body = |n: u64| -> String {
        blocked_by
            .iter()
            .filter(|(x, _)| *x == n)
            .map(|(_, b)| format!("Blocked by #{b}\n"))
            .collect()
    };
    let mut out: Vec<Issue> = open.iter().map(|&n| issue(n, "open", &body(n))).collect();
    out.extend(closed.iter().map(|&n| issue(n, "closed", &body(n))));
    out
}

#[test]
fn reads_blocked_by_lines_as_blocked_blocker_pairs() {
    let body = "## What\nFill it.\n\n## Blocked by\n- GH-1: snapshot store on disk\n\nBlocked by #29\nBlocked by #14\n";
    assert_eq!(edges(&[issue(31, "open", body)]), vec![(31, 29), (31, 14)]);
}

#[test]
fn several_numbers_on_one_line_in_order() {
    let i = issue(9, "open", "Blocked by: #3, #4 and #12");
    assert_eq!(edges(&[i]), vec![(9, 3), (9, 4), (9, 12)]);
}

#[test]
fn a_list_dash_or_star_leading_spaces_and_any_case_still_count() {
    let i = issue(
        20,
        "open",
        "  - Blocked by #5\n* blocked by #6\n   BLOCKED BY #7",
    );
    assert_eq!(edges(&[i]), vec![(20, 5), (20, 6), (20, 7)]);
}

#[test]
fn numbers_on_other_lines_and_a_bare_hash_are_ignored() {
    let i = issue(
        8,
        "open",
        "See #1.\nBlocks #9\n## Blocked by\nNothing.\nBlocked by # and #x\nRelated: Blocked by #2",
    );
    assert_eq!(edges(&[i]), Vec::<(u64, u64)>::new());
}

#[test]
fn a_repeat_and_a_self_reference_are_dropped() {
    let i = issue(4, "open", "Blocked by #2\nBlocked by #2, #4\nBlocked by #3");
    assert_eq!(edges(&[i]), vec![(4, 2), (4, 3)]);
}

#[test]
fn edges_follow_the_issue_order_of_the_slice() {
    let issues = vec![
        issue(5, "open", "Blocked by #1"),
        issue(2, "open", "Blocked by #1"),
    ];
    assert_eq!(edges(&issues), vec![(5, 1), (2, 1)]);
}

#[test]
fn the_longest_chain_runs_from_the_first_blocker_to_the_last_blocked() {
    // 1 -> 2 -> 3 and 1 -> 4 -> 5 -> 6
    let issues = graph(
        &[1, 2, 3, 4, 5, 6],
        &[],
        &[(2, 1), (3, 2), (4, 1), (5, 4), (6, 5)],
    );
    assert_eq!(longest(&issues), vec![1, 4, 5, 6]);
}

#[test]
fn closed_issues_and_unknown_numbers_are_not_on_a_chain() {
    // 7 is closed and 99 is not in the slice: 7 -> 2 and 99 -> 3 do not count.
    let issues = graph(&[1, 2, 3], &[7], &[(2, 7), (3, 99), (3, 1)]);
    assert_eq!(longest(&issues), vec![1, 3]);
    let closed_middle = graph(&[1, 3], &[2], &[(2, 1), (3, 2)]);
    assert_eq!(longest(&closed_middle), Vec::<u64>::new());
}

#[test]
fn no_blocked_by_line_between_open_issues_is_no_chain() {
    assert_eq!(longest(&graph(&[1, 2, 3], &[], &[])), Vec::<u64>::new());
    assert_eq!(longest(&[]), Vec::<u64>::new());
}

#[test]
fn a_tie_goes_to_the_chain_whose_first_issue_is_lower() {
    // 5 -> 6 -> 7 and 2 -> 8 -> 9
    let issues = graph(&[5, 6, 7, 2, 8, 9], &[], &[(6, 5), (7, 6), (8, 2), (9, 8)]);
    assert_eq!(longest(&issues), vec![2, 8, 9]);
}

#[test]
fn a_tie_from_the_same_first_issue_goes_to_the_lower_next_issue() {
    // 1 -> 3 -> 4 and 1 -> 2 -> 9; a diamond 1 -> 2 -> 9 and 1 -> 3 -> 9 picks 2.
    let issues = graph(&[1, 2, 3, 4, 9], &[], &[(3, 1), (4, 3), (2, 1), (9, 2)]);
    assert_eq!(longest(&issues), vec![1, 2, 9]);
    let diamond = graph(&[1, 2, 3, 9], &[], &[(2, 1), (3, 1), (9, 2), (9, 3)]);
    assert_eq!(longest(&diamond), vec![1, 2, 9]);
}

#[test]
fn a_cycle_is_broken_at_its_lowest_numbered_issue() {
    // 10 blocked by 12, 11 by 10, 12 by 11: the line into 10 is dropped.
    let issues = graph(&[12, 11, 10], &[], &[(10, 12), (11, 10), (12, 11)]);
    assert_eq!(longest(&issues), vec![10, 11, 12]);
}

#[test]
fn a_two_issue_cycle_reached_from_a_lower_issue_drops_the_line_back() {
    // 1 -> 5, 5 -> 3, 3 -> 5: the walk from 1 reaches 5, then 3; 3 -> 5 goes back and is dropped.
    let issues = graph(&[1, 3, 5], &[], &[(5, 1), (3, 5), (5, 3)]);
    assert_eq!(longest(&issues), vec![1, 5, 3]);
}

#[test]
fn a_cycle_does_not_hang_or_repeat_an_issue() {
    let issues = graph(&[1, 2], &[], &[(1, 2), (2, 1)]);
    assert_eq!(longest(&issues), vec![1, 2]);
}
