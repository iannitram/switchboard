//! Locked tests for #15 (GH-1: GitHub transport with the stored token).
//! Qwen never edits this file (R28). A one-request HTTP server on 127.0.0.1 stands in for
//! GitHub, so no test reaches the network.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use switchboard_core::github::transport::{GitHubTransport, HttpTransport, Recorded, RestAnswer};
use switchboard_core::platform::{GhAuthToken, TokenStore};
use switchboard_core::CoreError;

/// Serve one request with `response` (a full HTTP/1.1 answer); the request text comes back on
/// the channel. Returns the base URL.
fn serve_once(response: String) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = s.read(&mut chunk).unwrap();
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(end) = text.find("\r\n\r\n") {
                let len = text[..end]
                    .lines()
                    .find_map(|l| {
                        let l = l.to_ascii_lowercase();
                        l.strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if buf.len() >= end + 4 + len || n == 0 {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        s.write_all(response.as_bytes()).unwrap();
        tx.send(String::from_utf8_lossy(&buf).to_string()).unwrap();
    });
    (base, rx)
}

fn http(status: &str, extra: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
        body.len()
    )
}

fn header(request: &str, name: &str) -> Option<String> {
    let want = format!("{}:", name.to_ascii_lowercase());
    request
        .split("\r\n\r\n")
        .next()
        .unwrap()
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with(&want))
        .map(|l| l[want.len()..].trim().to_string())
}

fn body(request: &str) -> Value {
    serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap()
}

struct Tokens(Option<&'static str>);
impl TokenStore for Tokens {
    fn get(&self, service: &str) -> Option<String> {
        assert_eq!(service, "github");
        self.0.map(String::from)
    }
}

#[test]
fn graphql_posts_query_and_variables_with_the_token_and_returns_data() {
    let (base, rx) = serve_once(http(
        "200 OK",
        "Content-Type: application/json\r\n",
        r#"{"data":{"repository":{"isArchived":false}}}"#,
    ));
    let t = HttpTransport::with_api("tok123", &base);
    let data = t
        .graphql("query { x }", &json!({"owner": "o", "name": "r"}))
        .unwrap();
    assert_eq!(data, json!({"repository": {"isArchived": false}}));
    let req = rx.recv().unwrap();
    assert!(req.starts_with("POST /graphql "), "{req}");
    assert_eq!(
        header(&req, "Authorization").as_deref(),
        Some("Bearer tok123")
    );
    assert!(header(&req, "User-Agent").is_some());
    assert_eq!(
        body(&req),
        json!({"query": "query { x }", "variables": {"owner": "o", "name": "r"}})
    );
}

#[test]
fn graphql_errors_in_the_answer_are_a_github_error_with_the_first_message() {
    let (base, _rx) = serve_once(http(
        "200 OK",
        "Content-Type: application/json\r\n",
        r#"{"data":null,"errors":[{"message":"Could not resolve to a Repository"}]}"#,
    ));
    let t = HttpTransport::with_api("tok", &base);
    match t.graphql("query { x }", &json!({})) {
        Err(CoreError::GitHub(m)) => assert!(m.contains("Could not resolve"), "{m}"),
        other => panic!("expected a GitHub error, got {other:?}"),
    }
}

#[test]
fn graphql_http_error_status_is_a_github_error() {
    let (base, _rx) = serve_once(http(
        "401 Unauthorized",
        "",
        r#"{"message":"Bad credentials"}"#,
    ));
    let t = HttpTransport::with_api("tok", &base);
    assert!(matches!(
        t.graphql("query { x }", &json!({})),
        Err(CoreError::GitHub(_))
    ));
}

#[test]
fn rest_get_sends_if_none_match_and_a_304_comes_back_as_an_answer() {
    let (base, rx) = serve_once(http("304 Not Modified", "ETag: \"e1\"\r\n", ""));
    let t = HttpTransport::with_api("tok", &base);
    let a = t.rest_get("/repos/o/r/events", Some("\"e1\"")).unwrap();
    assert_eq!(
        a,
        RestAnswer {
            status: 304,
            etag: Some("\"e1\"".to_string()),
            body: Value::Null,
        }
    );
    let req = rx.recv().unwrap();
    assert!(req.starts_with("GET /repos/o/r/events "), "{req}");
    assert_eq!(header(&req, "If-None-Match").as_deref(), Some("\"e1\""));
    assert_eq!(header(&req, "Authorization").as_deref(), Some("Bearer tok"));
    assert_eq!(
        header(&req, "Accept").as_deref(),
        Some("application/vnd.github+json")
    );
}

#[test]
fn rest_get_without_an_etag_sends_no_if_none_match_and_parses_the_body() {
    let (base, rx) = serve_once(http(
        "200 OK",
        "ETag: W/\"e2\"\r\nContent-Type: application/json\r\n",
        r#"[{"id":"1"}]"#,
    ));
    let t = HttpTransport::with_api("tok", &base);
    let a = t.rest_get("/repos/o/r/events", None).unwrap();
    assert_eq!(a.status, 200);
    assert_eq!(a.etag.as_deref(), Some("W/\"e2\""));
    assert_eq!(a.body, json!([{"id": "1"}]));
    assert_eq!(header(&rx.recv().unwrap(), "If-None-Match"), None);
}

#[test]
fn rest_get_error_status_is_an_answer_not_an_error() {
    let (base, _rx) = serve_once(http("404 Not Found", "", r#"{"message":"Not Found"}"#));
    let t = HttpTransport::with_api("tok", &base);
    let a = t.rest_get("/repos/o/gone/events", None).unwrap();
    assert_eq!(a.status, 404);
    assert_eq!(a.etag, None);
}

#[test]
fn no_server_is_a_github_error() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let t = HttpTransport::with_api("tok", &format!("http://127.0.0.1:{port}"));
    assert!(matches!(
        t.rest_get("/repos/o/r/events", None),
        Err(CoreError::GitHub(_))
    ));
    assert!(matches!(
        t.graphql("query { x }", &json!({})),
        Err(CoreError::GitHub(_))
    ));
}

#[test]
fn new_takes_the_github_token_from_the_store_and_names_a_missing_one() {
    assert!(HttpTransport::new(&Tokens(Some("tok"))).is_ok());
    assert!(matches!(
        HttpTransport::new(&Tokens(None)),
        Err(CoreError::GitHub(_))
    ));
    assert_eq!(HttpTransport::API, "https://api.github.com");
}

#[test]
fn recorded_replays_in_order_logs_each_request_and_errs_when_empty() {
    let r = Recorded::new(
        vec![json!({"n": 1}), json!({"n": 2})],
        vec![RestAnswer {
            status: 304,
            etag: None,
            body: Value::Null,
        }],
    );
    assert_eq!(r.graphql("q", &json!({})).unwrap(), json!({"n": 1}));
    assert_eq!(
        r.rest_get("/repos/o/r/events", Some("x")).unwrap().status,
        304
    );
    assert_eq!(r.graphql("q", &json!({})).unwrap(), json!({"n": 2}));
    assert!(matches!(
        r.graphql("q", &json!({})),
        Err(CoreError::GitHub(_))
    ));
    assert!(matches!(
        r.rest_get("/repos/o/r/events", None),
        Err(CoreError::GitHub(_))
    ));
    assert_eq!(
        *r.log.lock().unwrap(),
        vec![
            "graphql",
            "GET /repos/o/r/events",
            "graphql",
            "graphql",
            "GET /repos/o/r/events"
        ]
    );
}

#[test]
fn gh_auth_token_answers_only_github_and_none_when_gh_is_missing() {
    let missing = GhAuthToken {
        program: "switchboard-no-such-gh-program".to_string(),
    };
    assert_eq!(missing.get("github"), None);
    assert_eq!(GhAuthToken::default().program, "gh");
    assert_eq!(GhAuthToken::default().get("gemini"), None);
}
