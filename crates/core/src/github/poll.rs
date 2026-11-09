//! GH-1, GH-2, GH-3: the 10-minute GraphQL poll, the ETag skip and refresh-on-action.

use super::snapshot::{Issue, Milestone, PullRequest, RepoSnapshot, ReviewVerdict, SnapshotStore};
use super::transport::GitHubTransport;
use crate::clock::{Clock, UnixSeconds};
use crate::CoreError;
use serde_json::{json, Value};

/// GH-1: ten minutes between reads.
pub const POLL_SECONDS: i64 = 600;

/// GH-1: the GraphQL query for one repo: issues (labels, milestone, body, closing PR), labels,
/// milestones, PRs (head, labels, closing refs, review verdict comments, check state),
/// ROADMAP.md and docs/code-map.md.
pub const REPO_QUERY: &str = r#"query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    isArchived
    labels(first: 100) { nodes { name } }
    milestones(first: 50, states: [OPEN, CLOSED]) {
      nodes { title openIssues: issues(states: [OPEN]) { totalCount } closedIssues: issues(states: [CLOSED]) { totalCount } }
    }
    issues(first: 100, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        number title state body
        labels(first: 20) { nodes { name } }
        milestone { title }
        timelineItems(itemTypes: [CLOSED_EVENT], last: 1) {
          nodes { ... on ClosedEvent { closer { __typename ... on PullRequest { number } } } }
        }
      }
    }
    pullRequests(first: 50, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        number title state headRefName headRefOid
        labels(first: 20) { nodes { name } }
        closingIssuesReferences(first: 10) { nodes { number } }
        comments(last: 100) { nodes { body createdAt } }
        commits(last: 100) { nodes { commit { oid committedDate statusCheckRollup { state } } } }
      }
    }
    roadmap: object(expression: "HEAD:ROADMAP.md") { ... on Blob { text } }
    codeMap: object(expression: "HEAD:docs/code-map.md") { ... on Blob { text } }
  }
}"#;

/// GH-1: turn the GraphQL `data` for one repo into a snapshot.
pub fn parse_repo(
    repo: &str,
    data: &Value,
    read_at: UnixSeconds,
) -> Result<RepoSnapshot, CoreError> {
    let Some(answer) = data.get("repository") else {
        return Err(CoreError::Parse(format!(
            "{repo}: no repository in the answer"
        )));
    };
    if answer.is_null() {
        return Err(CoreError::GitHub(format!("{repo}: repository not found")));
    }

    let labels: Vec<String> = nodes(answer, "labels")
        .iter()
        .map(|n| str_at(n, "name"))
        .collect();
    let milestones: Vec<Milestone> = nodes(answer, "milestones")
        .iter()
        .map(|n| Milestone {
            title: str_at(n, "title"),
            open_issues: count_at(n, "openIssues"),
            closed_issues: count_at(n, "closedIssues"),
        })
        .collect();

    let mut issues: Vec<Issue> = nodes(answer, "issues")
        .iter()
        .map(|n| {
            let state = str_at(n, "state").to_lowercase();
            let closed_by_pr = if state == "closed" {
                nodes(n, "timelineItems")
                    .last()
                    .and_then(|t| t.get("closer"))
                    .filter(|c| c.get("__typename").and_then(Value::as_str) == Some("PullRequest"))
                    .and_then(|c| c.get("number").and_then(Value::as_u64))
            } else {
                None
            };
            Issue {
                number: num_at(n, "number"),
                title: str_at(n, "title"),
                state,
                labels: nodes(n, "labels")
                    .iter()
                    .map(|l| str_at(l, "name"))
                    .collect(),
                milestone: n
                    .get("milestone")
                    .and_then(|m| m.get("title"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                body: str_at(n, "body"),
                closed_by_pr,
            }
        })
        .collect();
    issues.sort_by_key(|i| i.number);

    let mut pulls: Vec<PullRequest> = nodes(answer, "pullRequests")
        .iter()
        .map(|n| {
            let labels: Vec<String> = nodes(n, "labels")
                .iter()
                .map(|l| str_at(l, "name"))
                .collect();
            let commits = nodes(n, "commits");
            let comments = nodes(n, "comments");
            let verdicts: Vec<ReviewVerdict> = comments
                .iter()
                .filter_map(|c| {
                    let verdict = verdict_in(&str_at(c, "body"))?;
                    Some(ReviewVerdict {
                        verdict: verdict.to_string(),
                        sha: sha_at_or_before(&commits, &str_at(c, "createdAt")),
                        needs_me: verdict == "FAIL"
                            && !labels.iter().any(|l| l == "fixed-in-review"),
                    })
                })
                .collect();
            PullRequest {
                number: num_at(n, "number"),
                title: str_at(n, "title"),
                state: str_at(n, "state").to_lowercase(),
                head_sha: str_at(n, "headRefOid"),
                head_ref: str_at(n, "headRefName"),
                labels,
                closes: nodes(n, "closingIssuesReferences")
                    .iter()
                    .map(|c| num_at(c, "number"))
                    .collect(),
                verdicts,
                ci_on_head: ci_word(
                    commits
                        .last()
                        .and_then(|c| c.get("commit"))
                        .and_then(|c| c.get("statusCheckRollup"))
                        .and_then(|r| r.get("state"))
                        .and_then(Value::as_str),
                )
                .to_string(),
            }
        })
        .collect();
    pulls.sort_by_key(|p| p.number);

    Ok(RepoSnapshot {
        repo: repo.to_string(),
        read_at,
        etag: None,
        archived: answer
            .get("isArchived")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        issues,
        pulls,
        milestones,
        labels,
        roadmap_md: text_of(answer, "roadmap"),
        code_map_md: text_of(answer, "codeMap"),
    })
}

/// The R25 verdict in a PR comment body, the same rule as `merge_pr.py`: the last line that,
/// without whitespace and the `*`, `_`, backtick markup and in any case, is exactly
/// `VERDICT: PASS` or `VERDICT: FAIL`; `None` when there is none.
pub fn verdict_in(body: &str) -> Option<&'static str> {
    let mut found = None;
    for line in body.lines() {
        let cleaned: String = line
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '*' && *c != '_' && *c != '`')
            .collect::<String>()
            .to_uppercase();
        if let Some(v) = match cleaned.as_str() {
            "VERDICT:PASS" => Some("PASS"),
            "VERDICT:FAIL" => Some("FAIL"),
            _ => None,
        } {
            found = Some(v);
        }
    }
    found
}

/// `obj[key].totalCount`, 0 when missing.
fn count_at(obj: &Value, key: &str) -> u64 {
    obj.get(key)
        .and_then(|v| v.get("totalCount"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// `obj[key]` as a u64, 0 when missing.
fn num_at(obj: &Value, key: &str) -> u64 {
    obj.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// `obj[key]` as a string, `""` when missing.
fn str_at(obj: &Value, key: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// `obj[key].nodes`, `[]` when the list (or its `nodes`) is missing.
fn nodes<'a>(obj: &'a Value, key: &str) -> Vec<&'a Value> {
    obj.get(key)
        .and_then(|v| v.get("nodes"))
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

/// `obj[key].text`, `None` when the object is null or has no `text`.
fn text_of(obj: &Value, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(|o| o.get("text"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// The head sha a verdict was posted after: the last commit committed at or before `at`
/// (ISO strings compared as strings), `""` when none is.
fn sha_at_or_before(commits: &[&Value], at: &str) -> String {
    commits
        .iter()
        .rev()
        .find(|c| {
            c.get("commit")
                .and_then(|c| c.get("committedDate"))
                .and_then(Value::as_str)
                .is_some_and(|d| d <= at)
        })
        .and_then(|c| c.get("commit"))
        .and_then(|c| c.get("oid"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// The check rollup state on the head, in the four words the views use.
fn ci_word(state: Option<&str>) -> &'static str {
    match state {
        Some("SUCCESS") => "success",
        Some("FAILURE") | Some("ERROR") => "failure",
        Some("PENDING") | Some("EXPECTED") => "pending",
        _ => "none",
    }
}

/// GH-2: true when the REST conditional request answered 304 (unchanged), so no GraphQL read.
pub fn unchanged(
    transport: &dyn GitHubTransport,
    repo: &str,
    etag: Option<&str>,
) -> Result<(bool, Option<String>), CoreError> {
    let path = format!("/repos/{repo}/events");
    let a = transport.rest_get(&path, etag)?;
    match a.status {
        304 => Ok((true, a.etag.or_else(|| etag.map(String::from)))),
        200 => Ok((false, a.etag)),
        n => Err(CoreError::GitHub(format!("GET {path} answered {n}"))),
    }
}

/// The poller: which repos are due, read them, store them.
pub struct Poller<'a> {
    pub transport: &'a dyn GitHubTransport,
    pub store: &'a SnapshotStore,
    pub clock: &'a dyn Clock,
    /// The constellations' repos, `owner/name`.
    pub repos: Vec<String>,
}

impl Poller<'_> {
    /// GH-1: read every repo whose snapshot is `POLL_SECONDS` old or more (or missing). Returns
    /// the repos read; the first error met is returned at the end, the other repos are still read.
    pub fn tick(&self) -> Result<Vec<String>, CoreError> {
        let now = self.clock.now();
        let mut read = Vec::new();
        let mut first_error = None;
        for repo in &self.repos {
            let stored = match self.store.load(repo) {
                Ok(stored) => stored,
                Err(e) => {
                    first_error.get_or_insert(e);
                    continue;
                }
            };
            let due = match &stored {
                None => true,
                Some(snap) => now - snap.read_at >= POLL_SECONDS,
            };
            if !due {
                continue;
            }
            match &stored {
                Some(old) => match unchanged(self.transport, repo, old.etag.as_deref()) {
                    Ok((true, etag)) => {
                        let mut snap = old.clone();
                        snap.read_at = now;
                        snap.etag = etag;
                        match self.store.save(&snap) {
                            Ok(()) => read.push(repo.clone()),
                            Err(e) => {
                                first_error.get_or_insert(e);
                            }
                        }
                    }
                    Ok((false, etag)) => match self.read_one(repo, etag) {
                        Ok(()) => read.push(repo.clone()),
                        Err(e) => {
                            first_error.get_or_insert(e);
                        }
                    },
                    Err(e) => {
                        first_error.get_or_insert(e);
                    }
                },
                None => match self.read_one(repo, None) {
                    Ok(()) => read.push(repo.clone()),
                    Err(e) => {
                        first_error.get_or_insert(e);
                    }
                },
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(read),
        }
    }

    /// The Refresh button: read every repo now, whatever its age, keeping the stored ETag.
    pub fn refresh_all(&self) -> Result<Vec<String>, CoreError> {
        let mut read = Vec::new();
        let mut first_error = None;
        for repo in &self.repos {
            let etag = self.store.load(repo).ok().flatten().and_then(|s| s.etag);
            match self.read_one(repo, etag) {
                Ok(()) => read.push(repo.clone()),
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(read),
        }
    }

    /// GH-1: one repo: split it, the GraphQL read, the snapshot, stored with its ETag. Nothing
    /// is stored when any step fails, so the old snapshot stays.
    fn read_one(&self, repo: &str, etag: Option<String>) -> Result<(), CoreError> {
        let Some((owner, name)) = repo.split_once('/') else {
            return Err(CoreError::GitHub(format!("not owner/name: {repo}")));
        };
        let data = self
            .transport
            .graphql(REPO_QUERY, &json!({"owner": owner, "name": name}))?;
        let mut snap = parse_repo(repo, &data, self.clock.now())?;
        snap.etag = etag;
        self.store.save(&snap)
    }

    /// GH-3: re-read one repo right after a write to it.
    pub fn after_write(&self, repo: &str) -> Result<(), CoreError> {
        let etag = self.store.load(repo).ok().flatten().and_then(|s| s.etag);
        self.read_one(repo, etag)
    }
}
