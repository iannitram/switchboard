//! #347, #365: the transport's PATCH, the one write that is not a create. A one-request HTTP
//! server on 127.0.0.1 stands in for GitHub, so no test reaches the network.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use switchboard_core::github::transport::{GitHubTransport, HttpTransport, RestAnswer};
use switchboard_core::CoreError;

/// Serve one request with `response`; the request text comes back on the channel.
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

fn http(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\
         Content-Type: application/json\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn patch_sends_the_body_with_the_token_and_returns_the_answer() {
    let (base, rx) = serve_once(http("200 OK", r#"{"archived":true}"#));
    let t = HttpTransport::with_api("tok123", &base);
    let got = t
        .rest_patch("/repos/owner/alpha", &json!({"archived": true}))
        .unwrap();
    assert_eq!(got.status, 200);
    assert_eq!(got.body, json!({"archived": true}));
    let req = rx.recv().unwrap();
    assert!(req.starts_with("PATCH /repos/owner/alpha "), "{req}");
    let head = req.split("\r\n\r\n").next().unwrap().to_ascii_lowercase();
    assert!(head.contains("authorization: bearer tok123"), "{head}");
    assert!(head.contains("user-agent:"), "{head}");
    let sent: Value = serde_json::from_str(req.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(sent, json!({"archived": true}));
}

#[test]
fn a_status_that_is_not_200_comes_back_as_an_answer_not_a_success() {
    for status in ["403 Forbidden", "404 Not Found", "422 Unprocessable Entity"] {
        let (base, _rx) = serve_once(http(status, r#"{"message":"no"}"#));
        let t = HttpTransport::with_api("tok123", &base);
        let got = t
            .rest_patch("/repos/owner/alpha/issues/1", &json!({"state": "closed"}))
            .unwrap();
        assert_eq!(got.status.to_string(), status[..3]);
        assert_eq!(got.body, json!({"message": "no"}));
    }
}

/// A transport that only reads (every fake that does not say otherwise) refuses a PATCH.
#[test]
fn a_transport_that_does_not_implement_patch_refuses_it() {
    struct ReadOnly;
    impl GitHubTransport for ReadOnly {
        fn graphql(&self, _query: &str, _variables: &Value) -> Result<Value, CoreError> {
            Ok(json!({}))
        }
        fn rest_get(&self, _path: &str, _etag: Option<&str>) -> Result<RestAnswer, CoreError> {
            Err(CoreError::GitHub("not expected".to_string()))
        }
    }
    match ReadOnly.rest_patch("/repos/owner/alpha", &json!({"archived": true})) {
        Err(CoreError::GitHub(why)) => assert!(why.contains("only reads"), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}
