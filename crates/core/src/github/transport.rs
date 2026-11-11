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

    /// PATCH a REST path with a JSON body: the one write that is not a create (GH-4 sends
    /// creates through the queue). It archives a repo after my confirm (#347) and closes or
    /// renumbers what a restore put back (#365). A status GitHub answers is an answer the
    /// caller reads. A transport that only reads keeps this default, which refuses.
    fn rest_patch(&self, path: &str, _body: &Value) -> Result<RestAnswer, CoreError> {
        Err(CoreError::GitHub(format!(
            "PATCH {path}: this transport only reads"
        )))
    }
}

/// Replays recorded answers in order (tests).
pub struct Recorded {
    pub graphql: std::sync::Mutex<Vec<Value>>,
    pub rest: std::sync::Mutex<Vec<RestAnswer>>,
    /// Every request made, for the test to count: `graphql`, `GET <path>` or
    /// `PATCH <path> <body>`.
    pub log: std::sync::Mutex<Vec<String>>,
    /// The answers to PATCH requests, in order; when none is left a PATCH is answered 200 with
    /// the body it sent.
    pub patch: std::sync::Mutex<Vec<RestAnswer>>,
}

impl Recorded {
    pub fn new(graphql: Vec<Value>, rest: Vec<RestAnswer>) -> Self {
        Self {
            graphql: std::sync::Mutex::new(graphql),
            rest: std::sync::Mutex::new(rest),
            log: std::sync::Mutex::new(Vec::new()),
            patch: std::sync::Mutex::new(Vec::new()),
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

    fn rest_patch(&self, path: &str, body: &Value) -> Result<RestAnswer, CoreError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("PATCH {path} {body}"));
        let mut queue = self.patch.lock().unwrap();
        if queue.is_empty() {
            return Ok(RestAnswer {
                status: 200,
                etag: None,
                body: body.clone(),
            });
        }
        Ok(queue.remove(0))
    }
}

/// The real transport: GraphQL POSTs, REST GETs and the REST PATCH over HTTPS with the stored
/// token. Creates go through the fleet queue (GH-4), never through here.
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
        answer_of(resp, "GET", &url)
    }

    fn rest_patch(&self, path: &str, body: &Value) -> Result<RestAnswer, CoreError> {
        let url = format!("{}{}", self.api, path);
        let sent = self
            .agent
            .request("PATCH", &url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("User-Agent", "switchboard")
            .set("Accept", "application/vnd.github+json")
            .send_json(body);
        // A 403, 404 or 422 is an answer the caller reads, not a transport failure.
        let resp = match sent {
            Ok(resp) => resp,
            Err(ureq::Error::Status(_, resp)) => resp,
            Err(e) => return Err(CoreError::GitHub(format!("PATCH {url}: {e}"))),
        };
        answer_of(resp, "PATCH", &url)
    }
}

/// A response as the core reads it: the status, the ETag and the body as JSON (the text itself
/// when it is not JSON, null when empty).
fn answer_of(resp: ureq::Response, verb: &str, url: &str) -> Result<RestAnswer, CoreError> {
    let status = resp.status();
    let etag = resp.header("ETag").map(String::from);
    let text = resp
        .into_string()
        .map_err(|e| CoreError::GitHub(format!("{verb} {url}: {e}")))?;
    let body = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    };
    Ok(RestAnswer { status, etag, body })
}
