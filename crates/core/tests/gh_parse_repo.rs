//! Locked tests for #16 (GH-1: one GraphQL read per repo parsed into a snapshot).
//! Qwen never edits this file (R28).

use serde_json::{json, Value};
use switchboard_core::github::poll::{parse_repo, verdict_in, REPO_QUERY};
use switchboard_core::github::{Issue, Milestone};
use switchboard_core::CoreError;

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// A recorded `data` object in the shape REPO_QUERY asks for.
fn recorded() -> Value {
    json!({
      "repository": {
        "isArchived": false,
        "labels": { "nodes": [ { "name": "size:easy" }, { "name": "feature:github" } ] },
        "milestones": { "nodes": [
          { "title": "1.0", "openIssues": { "totalCount": 40 }, "closedIssues": { "totalCount": 2 } }
        ] },
        "issues": { "nodes": [
          { "number": 15, "title": "Transport", "state": "CLOSED", "body": "b15",
            "labels": { "nodes": [ { "name": "size:medium" } ] },
            "milestone": { "title": "1.0" },
            "timelineItems": { "nodes": [
              { "closer": { "__typename": "PullRequest", "number": 70 } } ] } },
          { "number": 14, "title": "Snapshot store", "state": "OPEN", "body": "b14",
            "labels": { "nodes": [ { "name": "size:easy" }, { "name": "feature:github" } ] },
            "milestone": null,
            "timelineItems": { "nodes": [] } },
          { "number": 9, "title": "Closed by hand", "state": "CLOSED", "body": "",
            "labels": { "nodes": [] },
            "milestone": null,
            "timelineItems": { "nodes": [
              { "closer": { "__typename": "Commit" } } ] } }
        ] },
        "pullRequests": { "nodes": [
          { "number": 71, "title": "Poll", "state": "OPEN",
            "headRefName": "qwen/17-poll", "headRefOid": B,
            "labels": { "nodes": [ { "name": "kind:state" } ] },
            "closingIssuesReferences": { "nodes": [ { "number": 17 } ] },
            "comments": { "nodes": [
              { "body": "Looks fine.\n\nVERDICT: PASS\nAll tests green.", "createdAt": "2026-11-08T10:05:00Z" },
              { "body": "just a note", "createdAt": "2026-11-08T10:30:00Z" },
              { "body": "**VERDICT: FAIL**\nR25: the ETag is never stored.", "createdAt": "2026-11-08T11:05:00Z" }
            ] },
            "commits": { "nodes": [
              { "commit": { "oid": A, "committedDate": "2026-11-08T10:00:00Z", "statusCheckRollup": { "state": "SUCCESS" } } },
              { "commit": { "oid": B, "committedDate": "2026-11-08T11:00:00Z", "statusCheckRollup": { "state": "PENDING" } } }
            ] } },
          { "number": 70, "title": "Transport", "state": "MERGED",
            "headRefName": "qwen/15-transport", "headRefOid": A,
            "labels": { "nodes": [ { "name": "fixed-in-review" } ] },
            "closingIssuesReferences": { "nodes": [ { "number": 15 } ] },
            "comments": { "nodes": [
              { "body": "verdict: fail\nsee gap 1", "createdAt": "2026-11-08T10:01:00Z" }
            ] },
            "commits": { "nodes": [
              { "commit": { "oid": A, "committedDate": "2026-11-08T10:00:00Z", "statusCheckRollup": null } }
            ] } }
        ] },
        "roadmap": { "text": "# Roadmap\n" },
        "codeMap": null
      }
    })
}

#[test]
fn repo_query_asks_for_everything_the_snapshot_holds() {
    for part in [
        "query($owner: String!, $name: String!)",
        "repository(owner: $owner, name: $name)",
        "isArchived",
        "milestones(",
        "issues(first: 100",
        "timelineItems(itemTypes: [CLOSED_EVENT]",
        "pullRequests(first: 50",
        "headRefOid",
        "closingIssuesReferences",
        "comments(last: 100)",
        "statusCheckRollup",
        "committedDate",
        "HEAD:ROADMAP.md",
        "HEAD:docs/code-map.md",
    ] {
        assert!(REPO_QUERY.contains(part), "REPO_QUERY lacks {part}");
    }
}

#[test]
fn repo_read_at_and_archived_come_through_and_etag_is_none() {
    let s = parse_repo("iannitram/switchboard", &recorded(), 1_234).unwrap();
    assert_eq!(s.repo, "iannitram/switchboard");
    assert_eq!(s.read_at, 1_234);
    assert_eq!(s.etag, None);
    assert!(!s.archived);
    let mut data = recorded();
    data["repository"]["isArchived"] = json!(true);
    assert!(parse_repo("o/r", &data, 0).unwrap().archived);
}

#[test]
fn labels_and_milestones_in_answer_order() {
    let s = parse_repo("o/r", &recorded(), 0).unwrap();
    assert_eq!(s.labels, vec!["size:easy", "feature:github"]);
    assert_eq!(
        s.milestones,
        vec![Milestone {
            title: "1.0".to_string(),
            open_issues: 40,
            closed_issues: 2,
        }]
    );
}

#[test]
fn issues_sorted_by_number_with_lowercase_state_and_closing_pr() {
    let s = parse_repo("o/r", &recorded(), 0).unwrap();
    let numbers: Vec<u64> = s.issues.iter().map(|i| i.number).collect();
    assert_eq!(numbers, vec![9, 14, 15]);
    assert_eq!(
        s.issues[1],
        Issue {
            number: 14,
            title: "Snapshot store".to_string(),
            state: "open".to_string(),
            labels: vec!["size:easy".to_string(), "feature:github".to_string()],
            milestone: None,
            body: "b14".to_string(),
            closed_by_pr: None,
        }
    );
    assert_eq!(s.issues[2].state, "closed");
    assert_eq!(s.issues[2].milestone.as_deref(), Some("1.0"));
    assert_eq!(s.issues[2].closed_by_pr, Some(70));
    // closed by a commit or by hand: no PR
    assert_eq!(s.issues[0].closed_by_pr, None);
}

#[test]
fn pulls_sorted_by_number_with_head_closes_labels_and_state() {
    let s = parse_repo("o/r", &recorded(), 0).unwrap();
    let numbers: Vec<u64> = s.pulls.iter().map(|p| p.number).collect();
    assert_eq!(numbers, vec![70, 71]);
    let p = &s.pulls[1];
    assert_eq!(p.title, "Poll");
    assert_eq!(p.state, "open");
    assert_eq!(p.head_sha, B);
    assert_eq!(p.head_ref, "qwen/17-poll");
    assert_eq!(p.labels, vec!["kind:state"]);
    assert_eq!(p.closes, vec![17]);
    assert_eq!(s.pulls[0].state, "merged");
}

#[test]
fn ci_on_head_is_the_last_commits_rollup_mapped_to_four_words() {
    let s = parse_repo("o/r", &recorded(), 0).unwrap();
    assert_eq!(s.pulls[1].ci_on_head, "pending");
    assert_eq!(s.pulls[0].ci_on_head, "none");
    for (state, word) in [
        ("SUCCESS", "success"),
        ("FAILURE", "failure"),
        ("ERROR", "failure"),
        ("PENDING", "pending"),
        ("EXPECTED", "pending"),
    ] {
        let mut data = recorded();
        data["repository"]["pullRequests"]["nodes"][0]["commits"]["nodes"][1]["commit"]
            ["statusCheckRollup"]["state"] = json!(state);
        let s = parse_repo("o/r", &data, 0).unwrap();
        assert_eq!(s.pulls[1].ci_on_head, word, "{state}");
    }
}

#[test]
fn verdicts_oldest_first_each_on_the_head_it_was_posted_after() {
    let s = parse_repo("o/r", &recorded(), 0).unwrap();
    let v = &s.pulls[1].verdicts;
    assert_eq!(v.len(), 2, "the comment with no verdict line is skipped");
    assert_eq!(v[0].verdict, "PASS");
    assert_eq!(
        v[0].sha, A,
        "posted at 10:05, after A (10:00), before B (11:00)"
    );
    assert_eq!(v[1].verdict, "FAIL");
    assert_eq!(v[1].sha, B, "posted at 11:05, after B");
}

#[test]
fn a_verdict_posted_before_every_commit_has_an_empty_sha() {
    let mut data = recorded();
    data["repository"]["pullRequests"]["nodes"][0]["comments"]["nodes"][0]["createdAt"] =
        json!("2026-11-08T09:00:00Z");
    let s = parse_repo("o/r", &data, 0).unwrap();
    assert_eq!(s.pulls[1].verdicts[0].sha, "");
}

#[test]
fn needs_me_only_for_a_fail_on_a_pr_without_fixed_in_review() {
    let s = parse_repo("o/r", &recorded(), 0).unwrap();
    assert!(!s.pulls[1].verdicts[0].needs_me, "a PASS never needs me");
    assert!(s.pulls[1].verdicts[1].needs_me, "FAIL, no fixed-in-review");
    assert_eq!(s.pulls[0].verdicts[0].verdict, "FAIL");
    assert!(
        !s.pulls[0].verdicts[0].needs_me,
        "FAIL on a fixed-in-review PR"
    );
}

#[test]
fn roadmap_and_code_map_text_or_none() {
    let s = parse_repo("o/r", &recorded(), 0).unwrap();
    assert_eq!(s.roadmap_md.as_deref(), Some("# Roadmap\n"));
    assert_eq!(s.code_map_md, None);
}

#[test]
fn a_null_repository_is_a_github_error() {
    let data = json!({ "repository": null });
    assert!(matches!(
        parse_repo("o/gone", &data, 0),
        Err(CoreError::GitHub(_))
    ));
}

#[test]
fn an_answer_with_no_repository_key_is_a_parse_error() {
    let data = json!({ "viewer": {} });
    assert!(matches!(
        parse_repo("o/r", &data, 0),
        Err(CoreError::Parse(_))
    ));
}

#[test]
fn missing_lists_and_fields_read_as_empty() {
    let data = json!({ "repository": { "issues": { "nodes": [ { "number": 3 } ] } } });
    let s = parse_repo("o/r", &data, 0).unwrap();
    assert!(!s.archived);
    assert!(s.labels.is_empty() && s.milestones.is_empty() && s.pulls.is_empty());
    assert_eq!(s.issues[0].title, "");
    assert_eq!(s.issues[0].state, "");
    assert!(s.issues[0].labels.is_empty());
    assert_eq!(s.roadmap_md, None);
}

#[test]
fn verdict_in_reads_the_last_verdict_line_in_any_case_and_markup() {
    assert_eq!(verdict_in("VERDICT: PASS"), Some("PASS"));
    assert_eq!(verdict_in("  **VERDICT: FAIL**  "), Some("FAIL"));
    assert_eq!(verdict_in("`verdict: pass`"), Some("PASS"));
    assert_eq!(
        verdict_in("VERDICT: FAIL\nfixed\nVERDICT: PASS"),
        Some("PASS")
    );
    assert_eq!(verdict_in("The VERDICT: PASS is below"), None);
    assert_eq!(verdict_in("VERDICT: MAYBE"), None);
    assert_eq!(verdict_in(""), None);
}
