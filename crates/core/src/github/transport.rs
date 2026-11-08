//! The one way the core talks to GitHub over HTTP, behind a trait so tests replay recorded
//! answers. The token comes from the platform's `TokenStore` (on Linux in 1.0, `gh auth token`).

use crate::CoreError;
use serde_json::Value;

/// A REST answer the core needs: status, ETag and body.
#[derive(Debug, Clone, PartialEq)]
pub struct RestAnswer {
    pub status: u16,
    pub etag: Option<String>,
    pub body: Value,
}

pub trait GitHubTransport: Send + Sync {
    /// POST a GraphQL query with its variables; the `data` object back, or the error.
    fn graphql(&self, query: &str, variables: &Value) -> Result<Value, CoreError>;

    /// GET a REST path (e.g. `/repos/o/r/events`), sending `If-None-Match` when `etag` is set.
    fn rest_get(&self, path: &str, etag: Option<&str>) -> Result<RestAnswer, CoreError>;
}

/// Replays recorded answers in order (tests).
pub struct Recorded {
    pub graphql: std::sync::Mutex<Vec<Value>>,
    pub rest: std::sync::Mutex<Vec<RestAnswer>>,
    /// Every request made, for the test to count: `graphql` or `GET <path>`.
    pub log: std::sync::Mutex<Vec<String>>,
}
