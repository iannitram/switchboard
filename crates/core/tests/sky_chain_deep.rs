//! #337 (SKY-7): the longest chain is found without recursion, so a very long blocked-by chain
//! does not overflow the stack.

use switchboard_core::github::Issue;
use switchboard_core::sky::chain::longest;

fn chain_of(n: u64) -> Vec<Issue> {
    (1..=n)
        .map(|k| Issue {
            number: k,
            title: format!("issue {k}"),
            state: "open".to_string(),
            body: if k > 1 {
                format!("Blocked by #{}", k - 1)
            } else {
                String::new()
            },
            ..Default::default()
        })
        .collect()
}

#[test]
fn a_chain_of_forty_thousand_issues_is_walked_without_overflowing_the_stack() {
    let issues = chain_of(40_000);
    let chain = longest(&issues);
    assert_eq!(chain.len(), 40_000);
    assert_eq!(chain.first(), Some(&1));
    assert_eq!(chain.last(), Some(&40_000));
    assert!(chain.windows(2).all(|w| w[1] == w[0] + 1));
}

#[test]
fn a_long_chain_with_a_cycle_back_to_its_start_still_ends() {
    let mut issues = chain_of(30_000);
    // #1 is blocked by the last issue: the arrow back to #1 is dropped.
    issues[0].body = "Blocked by #30000".to_string();
    let chain = longest(&issues);
    assert_eq!(chain.len(), 30_000);
    assert_eq!(chain.first(), Some(&1));
}
