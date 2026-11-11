// FD-12 (#429): the held request `chat_route` hands the Chat view, and the guard that files a
// held request once while `chat_send_as_job` runs off the main thread.
use super::*;
use serde_json::json;
use switchboard_core::frontdoor::answer::ChatReply;
use switchboard_core::frontdoor::ask::Held;
use switchboard_core::frontdoor::chat::{ChatPick, ChatRoute};

fn reply() -> ChatReply {
    ChatReply {
        route: ChatRoute {
            step: "labeller".to_string(),
            kind: Some("web-research".to_string()),
            pick: ChatPick::Wait {
                reason: "waiting for budget".to_string(),
            },
        },
        handoff_id: None,
        text: None,
        service: None,
        model: None,
        handoff: None,
        queued: None,
        moved_from: None,
        moved_why: None,
        fallback: None,
        packet: None,
        claude_label: None,
    }
}

#[test]
fn a_routed_reply_carries_its_held_request_beside_the_route() {
    let held = Held {
        kind: "web-research".to_string(),
        request: "find the docs".to_string(),
        unmet: vec!["web".to_string()],
    };
    let wire = serde_json::to_value(ChatRouted {
        reply: reply(),
        held: Some(held),
    })
    .unwrap();
    assert_eq!(wire["step"], "labeller");
    assert_eq!(wire["kind"], "web-research");
    assert_eq!(
        wire["held"],
        json!({ "kind": "web-research", "request": "find the docs", "unmet": ["web"] })
    );
    let none = serde_json::to_value(ChatRouted {
        reply: reply(),
        held: None,
    })
    .unwrap();
    assert!(none.get("held").is_none(), "{none}");
}

#[test]
fn a_held_request_being_filed_is_not_filed_again_until_it_ends() {
    let key = filing_key("web-research", "find the docs (filing test)", Some("t"));
    let first = start_filing(key.clone()).expect("the first press files");
    assert!(
        start_filing(key.clone()).is_none(),
        "a second press while filing"
    );
    let other = filing_key("web-research", "find the docs (filing test)", None);
    let elsewhere = start_filing(other).expect("another chat's request is its own");
    drop(first);
    assert!(
        start_filing(key).is_some(),
        "once filing ends it can be sent again"
    );
    drop(elsewhere);
}

#[test]
fn chat_send_as_job_runs_off_the_main_thread() {
    let source = include_str!("commands.rs");
    let at = source.find("pub fn chat_send_as_job(").unwrap();
    let attr = source[..at].rfind("#[tauri::command").unwrap();
    assert!(source[attr..at].starts_with("#[tauri::command(async)]"));
    assert!(source[at..].contains("start_filing(filing_key("));
}
