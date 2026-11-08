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

impl Recorded {
    pub fn new(graphql: Vec<Value>, rest: Vec<RestAnswer>) -> Self {
        Self {
            graphql: std::sync::Mutex::new(graphql),
            rest: std::sync::Mutex::new(rest),
            log: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl GitHubTransport for Recorded {
    fn graphql(&self, _query: &str, _variables: &Value) -> Result<Value, CoreError> {
        self.log.lock().unwrap().push("graphql".to_string());
        let mut queue = self.graphql.lock().unwrap();
        if queue.is_empty() {
            return Err(CoreError::GitHub(
                "no recorded GraphQL answer left".to_string(),
            ));
        }
        Ok(queue.remove(0))
    }

    fn rest_get(&self, path: &str, _etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        self.log.lock().unwrap().push(format!("GET {path}"));
        let mut queue = self.rest.lock().unwrap();
        if queue.is_empty() {
            return Err(CoreError::GitHub(
                "no recorded REST answer left".to_string(),
            ));
        }
        Ok(queue.remove(0))
    }
}

/// The real transport: GraphQL POSTs and REST GETs over HTTPS with the stored token. It only
/// reads; creates go through the fleet queue (GH-4).
pub struct HttpTransport {
    token: String,
    api: String,
    agent: ureq::Agent,
}

impl HttpTransport {
    pub const API: &'static str = "https://api.github.com";

    /// The transport for the token the platform's store holds for GitHub.
    pub fn new(tokens: &dyn crate::platform::TokenStore) -> Result<Self, CoreError> {
        match tokens.get("github") {
            None => Err(CoreError::GitHub(
                "no GitHub token: run `gh auth login`".to_string(),
            )),
            Some(t) => Ok(Self::with_api(&t, Self::API)),
        }
    }

    /// A transport against `api` (no trailing `/`), for tests and GitHub Enterprise.
    pub fn with_api(token: &str, api: &str) -> Self {
        Self {
            token: token.to_string(),
            api: api.trim_end_matches('/').to_string(),
            agent: ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(30))
                .build(),
        }
    }
}

impl GitHubTransport for HttpTransport {
    fn graphql(&self, query: &str, variables: &Value) -> Result<Value, CoreError> {
        let url = format!("{}/graphql", self.api);
        let answer: Value = self
            .agent
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("User-Agent", "switchboard")
            .send_json(serde_json::json!({"query": query, "variables": variables}))
            .map_err(|e| CoreError::GitHub(format!("POST {url}: {e}")))?
            .into_json()
            .map_err(|e| CoreError::GitHub(format!("POST {url}: {e}")))?;
        if let Some(message) = answer["errors"][0]["message"].as_str() {
            return Err(CoreError::GitHub(message.to_string()));
        }
        Ok(answer["data"].clone())
    }

    fn rest_get(&self, path: &str, etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        let url = format!("{}{}", self.api, path);
        let mut request = self
            .agent
            .get(&url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("User-Agent", "switchboard")
            .set("Accept", "application/vnd.github+json");
        if let Some(tag) = etag {
            request = request.set("If-None-Match", tag);
        }
        // A 304 or 404 is an answer the caller reads, not a transport failure.
        let resp = match request.call() {
            Ok(resp) => resp,
            Err(ureq::Error::Status(_, resp)) => resp,
            Err(e) => return Err(CoreError::GitHub(format!("GET {url}: {e}"))),
        };
        let status = resp.status();
        let etag = resp.header("ETag").map(String::from);
        let text = resp
            .into_string()
            .map_err(|e| CoreError::GitHub(format!("GET {url}: {e}")))?;
        let body = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        Ok(RestAnswer { status, etag, body })
    }
}
