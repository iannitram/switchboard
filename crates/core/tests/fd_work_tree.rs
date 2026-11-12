//! #378, the parts #393 left: FD-5's work-tree line (each move is stored with where the chat
//! left, where it went and why, and REP-2's `reports::moves` reads those lines), FD-6's packet
//! on a chat sent as a fleet job (`send_thread_as_job`), and FD-7's open questions (a queued
//! ask the summary must name). The chat transport is a fake; no service and no fleet is called.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use switchboard_core::frontdoor::adapters::ChatTransport;
use switchboard_core::frontdoor::answer::{
    answer_route, open_questions, send_thread_as_job, AnswerContext, ChatReply,
};
use switchboard_core::frontdoor::ask::job_why;
use switchboard_core::frontdoor::chat::{ChatPick, ChatRoute};
use switchboard_core::frontdoor::rating_log::log_rating;
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::joblog::JobRow;
use switchboard_core::reports::{moves, Move};
use switchboard_core::CoreError;

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

/// Answers every call with the next of `replies` (the last one again when they run out).
struct Fake {
    seen: Seen,
    replies: Arc<Mutex<Vec<String>>>,
}

impl ChatTransport for Fake {
    fn post_json(&self, url: &str, body: &Value) -> Result<Value, CoreError> {
        self.seen
            .lock()
            .unwrap()
            .push((url.to_string(), body.clone()));
        let mut replies = self.replies.lock().unwrap();
        let text = if replies.len() > 1 {
            replies.remove(0)
        } else {
            replies[0].clone()
        };
        Ok(json!({"choices": [{"message": {"content": text}}]}))
    }
    fn post_json_headers(
        &self,
        url: &str,
        _headers: &[(String, String)],
        body: &Value,
    ) -> Result<Value, CoreError> {
        self.post_json(url, body)
    }
}

struct Chat {
    data: PathBuf,
    seen: Seen,
    replies: Arc<Mutex<Vec<String>>>,
}

impl Chat {
    fn new(name: &str) -> Self {
        let data = std::env::temp_dir().join(format!("sb-fd378-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        Chat {
            data,
            seen: Seen::default(),
            replies: Arc::new(Mutex::new(vec!["ok".to_string()])),
        }
    }

    fn replies(&self, replies: &[&str]) {
        *self.replies.lock().unwrap() = replies.iter().map(|r| r.to_string()).collect();
    }

    /// Runs `with` over this chat's answer context.
    fn with<T>(&self, with: impl FnOnce(&AnswerContext) -> T) -> T {
        let env = |name: &str| (name == "DEEPSEEK_API_KEY").then(|| "test-key".to_string());
        let (seen, replies) = (self.seen.clone(), self.replies.clone());
        let transport = move || -> Box<dyn ChatTransport> {
            Box::new(Fake {
                seen: seen.clone(),
                replies: replies.clone(),
            })
        };
        with(&AnswerContext {
            data_dir: &self.data,
            qwen_base_url: Some("http://127.0.0.1:8080"),
            env: &env,
            transport: &transport,
        })
    }

    fn send(&self, route: ChatRoute, message: &str) -> ChatReply {
        self.with(|ctx| answer_route(route, message, Some("t"), ctx).unwrap())
    }

    /// Thread `t`, answered once by qwen.
    fn opened(name: &str) -> Self {
        let chat = Chat::new(name);
        chat.replies(&["A monad is a monoid."]);
        chat.send(qwen(), "what is a monad");
        chat
    }

    fn rows(&self) -> Vec<JobRow> {
        ThreadStore::new(&self.data).read("t").unwrap()
    }

    fn move_rows(&self) -> Vec<JobRow> {
        let mut rows = self.rows();
        rows.retain(|r| r.role.as_deref() == Some("move"));
        rows
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.seen.lock().unwrap().clone()
    }
}

fn to(step: &str, service: &str, model: &str) -> ChatRoute {
    ChatRoute {
        step: step.to_string(),
        kind: Some("quick-lookup".to_string()),
        pick: ChatPick::Service {
            service: service.to_string(),
            model: model.to_string(),
        },
    }
}

fn qwen() -> ChatRoute {
    to("labeller", "qwen", "qwen-small")
}

fn deepseek() -> ChatRoute {
    to("labeller", "deepseek", "deepseek-chat")
}

fn line(from: (&str, &str), to: (&str, &str), why: &str) -> Move {
    Move {
        from_service: from.0.to_string(),
        from_model: from.1.to_string(),
        to_service: to.0.to_string(),
        to_model: to.1.to_string(),
        why: why.to_string(),
    }
}

/// The non-system contents of one sent request.
fn contents(body: &Value) -> Vec<String> {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] != "system")
        .map(|m| m["content"].as_str().unwrap().to_string())
        .collect()
}

// --- FD-5: each move is a line in the work tree ---

#[test]
fn a_thumbs_down_move_is_stored_as_its_work_tree_line() {
    let chat = Chat::opened("down");
    log_rating(&chat.data, "t", 1, "down", "qwen", "qwen-small", None).unwrap();
    chat.replies(&["Monads so far.", "A functor maps."]);
    let reply = chat.send(deepseek(), "what is a functor");
    assert_eq!(reply.moved_from.as_deref(), Some("qwen qwen-small"));
    assert_eq!(reply.moved_why.as_deref(), Some("thumbs down"));
    let wire = serde_json::to_value(&reply).unwrap();
    assert_eq!(wire["moved_why"], "thumbs down");

    let stored = chat.move_rows();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].moved_from.as_deref(), Some("qwen qwen-small"));
    assert_eq!(stored[0].lane.as_deref(), Some("deepseek"));
    assert_eq!(stored[0].model.as_deref(), Some("deepseek-chat"));
    assert_eq!(stored[0].moved_why.as_deref(), Some("thumbs down"));
    assert_eq!(
        moves(&chat.rows()),
        [line(
            ("qwen", "qwen-small"),
            ("deepseek", "deepseek-chat"),
            "thumbs down"
        )]
    );
}

#[test]
fn each_signal_says_why_the_chat_moved() {
    // "ask Claude": the chat goes to a Claude session, which has no model the core picks.
    let chat = Chat::opened("claude");
    chat.replies(&["Monads so far."]);
    let handoff = ChatRoute {
        step: "explicit ask claude".to_string(),
        kind: None,
        pick: ChatPick::ClaudeHandoff {
            reason: "claude: needs a Claude session with tools".to_string(),
            model: None,
        },
    };
    let reply = chat.send(handoff, "ask Claude to refactor it");
    assert_eq!(reply.moved_why.as_deref(), Some("I said \"ask Claude\""));
    assert_eq!(
        moves(&chat.rows()),
        [line(
            ("qwen", "qwen-small"),
            ("claude", "model not chosen by Switchboard"),
            "I said \"ask Claude\""
        )]
    );

    // A named agent: the router's trigger step names it.
    let chat = Chat::opened("agent");
    chat.replies(&["Monads so far.", "Reviewed."]);
    let reply = chat.send(
        to("trigger word reviewer", "deepseek", "deepseek-chat"),
        "reviewer, look at this",
    );
    assert_eq!(reply.moved_why.as_deref(), Some("I named reviewer"));

    // The model's own handoff marker, with its reason.
    let chat = Chat::new("marker");
    chat.replies(&["I cannot browse. [[handoff: needs the web]]"]);
    chat.send(qwen(), "what is new today");
    chat.replies(&["Asked for news.", "Here is the news."]);
    let reply = chat.send(deepseek(), "then who can");
    assert_eq!(
        reply.moved_why.as_deref(),
        Some("the model handed off: needs the web")
    );

    // No signal: the chat was routed again because its pool is braked (FD-3).
    let chat = Chat::opened("braked");
    chat.replies(&["Monads so far.", "A functor maps."]);
    let reply = chat.send(deepseek(), "what is a functor");
    assert_eq!(
        reply.moved_why.as_deref(),
        Some("qwen is braked or over budget")
    );
    assert_eq!(
        chat.move_rows()[0].moved_why.as_deref(),
        Some("qwen is braked or over budget")
    );
}

#[test]
fn a_threads_work_tree_is_its_move_rows_and_nothing_else() {
    let chat = Chat::opened("tree");
    chat.replies(&["Summary one.", "From deepseek."]);
    chat.send(deepseek(), "ask Claude later, first this");
    chat.replies(&["Summary two.", "From qwen."]);
    log_rating(
        &chat.data,
        "t",
        3,
        "down",
        "deepseek",
        "deepseek-chat",
        None,
    )
    .unwrap();
    chat.send(qwen(), "back to you");
    // The user rows have no lane; the rating and the summary rows name one: none is a move.
    assert_eq!(
        moves(&chat.rows()),
        [
            line(
                ("qwen", "qwen-small"),
                ("deepseek", "deepseek-chat"),
                "I said \"ask Claude\""
            ),
            line(
                ("deepseek", "deepseek-chat"),
                ("qwen", "qwen-small"),
                "thumbs down"
            ),
        ]
    );
    // A thread that never moved has an empty tree.
    assert_eq!(moves(&Chat::opened("still").rows()), []);
}

// --- FD-6, FD-12: a chat sent as a fleet job carries the packet ---

#[test]
fn a_chat_sent_as_a_job_carries_the_packet_and_stores_the_move() {
    let chat = Chat::opened("job");
    chat.replies(&["See file:///srv/notes.md for more."]);
    chat.send(qwen(), "where can I read more");
    chat.replies(&["Monads; notes at file:///srv/notes.md"]);
    let before = chat.calls().len();
    let mut got: Vec<String> = Vec::new();
    let why = job_why(&["web".to_string()]);
    let job = chat
        .with(|ctx| {
            send_thread_as_job(
                "t",
                "web-research",
                "find the paper it cites",
                &why,
                ctx,
                &mut |text| {
                    got.push(text.to_string());
                    Ok(json!({"job": "j-1"}))
                },
            )
        })
        .unwrap();
    assert_eq!(job, json!({"job": "j-1"}));
    assert_eq!(chat.calls().len(), before + 1, "one summary ask of qwen");
    assert_eq!(
        got,
        ["Summary\nMonads; notes at file:///srv/notes.md\n\n\
          Last turns\n\
          user: what is a monad\n\
          assistant: A monad is a monoid.\n\
          user: where can I read more\n\
          assistant: See file:///srv/notes.md for more.\n\n\
          Links\nfile:///srv/notes.md\n\n\
          Ask\nfind the paper it cites"]
    );
    let stored = chat.move_rows();
    assert_eq!(stored.len(), 1);
    assert_eq!(
        moves(&chat.rows()),
        [line(
            ("qwen", "qwen-small"),
            ("fleet-job", "web-research"),
            "I sent it as a fleet job (needs web)"
        )]
    );
    let summaries: Vec<_> = chat
        .rows()
        .into_iter()
        .filter(|r| r.role.as_deref() == Some("summary"))
        .collect();
    assert_eq!(summaries.len(), 1);
    assert_eq!(ThreadStore::new(&chat.data).turns("t").unwrap().len(), 4);
}

#[test]
fn a_job_fleet_does_not_queue_stores_no_move() {
    let chat = Chat::opened("job-refused");
    let before = chat.rows();
    let err = chat.with(|ctx| {
        send_thread_as_job("t", "web-research", "find it", "why", ctx, &mut |_| {
            Err(CoreError::Refused("fd-12: fleet did not queue".to_string()))
        })
    });
    assert_eq!(
        err,
        Err(CoreError::Refused("fd-12: fleet did not queue".to_string()))
    );
    assert_eq!(chat.rows(), before);
}

#[test]
fn a_thread_with_no_answer_sends_the_request_as_it_is_and_an_unknown_one_is_refused() {
    let chat = Chat::new("job-bare");
    let wait = ChatRoute {
        pick: ChatPick::Wait {
            reason: "waiting for budget: quick-lookup".to_string(),
        },
        ..qwen()
    };
    chat.send(wait, "what is a monad");
    let mut got: Vec<String> = Vec::new();
    chat.with(|ctx| {
        send_thread_as_job("t", "web-research", "find it", "why", ctx, &mut |text| {
            got.push(text.to_string());
            Ok(Value::Null)
        })
    })
    .unwrap();
    assert_eq!(got, ["find it"]);
    assert!(chat.calls().is_empty(), "no model has a summary to write");
    assert!(chat.move_rows().is_empty());

    let mut called = false;
    let err = chat.with(|ctx| {
        send_thread_as_job("nope", "web-research", "find it", "why", ctx, &mut |_| {
            called = true;
            Ok(Value::Null)
        })
    });
    assert_eq!(err, Err(CoreError::Refused("no thread nope".to_string())));
    assert!(!called, "nothing is sent for a thread that is not stored");
}

// --- FD-7: the open questions the core has logged for the thread ---

#[test]
fn a_queued_ask_is_an_open_question_the_summary_must_name() {
    let chat = Chat::opened("open");
    let wait = ChatRoute {
        pick: ChatPick::Wait {
            reason: "waiting for budget: quick-lookup".to_string(),
        },
        ..qwen()
    };
    chat.send(wait, "is a list a monad");
    assert_eq!(open_questions(&chat.rows()), ["is a list a monad"]);

    // qwen's first summary leaves the question out, so it is asked once more.
    chat.replies(&[
        "We talked about monads.",
        "We talked about monads. Open: is a list a monad",
        "A functor maps.",
    ]);
    let before = chat.calls().len();
    chat.send(deepseek(), "what is a functor");
    let calls = chat.calls()[before..].to_vec();
    assert_eq!(calls.len(), 3, "two summary asks, then the answer");
    let first = contents(&calls[0].1);
    assert!(
        first.last().unwrap().ends_with(
            "Open questions logged for this chat (asked, not answered yet). \
             Name each one word for word:\n- is a list a monad"
        ),
        "{first:?}"
    );
    let again = contents(&calls[1].1);
    assert_eq!(
        again.last().unwrap(),
        "The summary leaves out: is a list a monad. Write it again and name them."
    );
    let system = calls[2].1["messages"][0]["content"].as_str().unwrap();
    assert!(
        system.contains("Summary\nWe talked about monads. Open: is a list a monad"),
        "{system}"
    );
}

#[test]
fn a_question_asked_again_and_answered_is_open_no_more() {
    let chat = Chat::opened("answered");
    let wait = ChatRoute {
        pick: ChatPick::Wait {
            reason: "waiting for budget: quick-lookup".to_string(),
        },
        ..qwen()
    };
    chat.send(wait.clone(), "is a list a monad");
    chat.send(wait.clone(), "is a list a monad");
    chat.send(wait, "and a tree");
    assert_eq!(
        open_questions(&chat.rows()),
        ["is a list a monad", "and a tree"],
        "oldest first, no repeats"
    );
    chat.replies(&["Yes."]);
    chat.send(qwen(), "is a list a monad");
    assert_eq!(open_questions(&chat.rows()), ["and a tree"]);

    // With no open question the summary ask is as it was, and one summary ask is enough.
    let chat = Chat::opened("none");
    assert!(open_questions(&chat.rows()).is_empty());
    chat.replies(&["We talked about monads.", "A functor maps."]);
    let before = chat.calls().len();
    chat.send(deepseek(), "what is a functor");
    let calls = chat.calls()[before..].to_vec();
    assert_eq!(calls.len(), 2);
    assert!(!contents(&calls[0].1)
        .last()
        .unwrap()
        .contains("Open questions"));
}

#[test]
fn open_questions_match_after_trim_case_and_spaces() {
    // #428: the same question with other case or spacing is one open question, and asking it
    // again in other case or spacing and getting an answer closes it. No fuzzy matching.
    let chat = Chat::opened("normalised");
    let wait = ChatRoute {
        pick: ChatPick::Wait {
            reason: "waiting for budget: quick-lookup".to_string(),
        },
        ..qwen()
    };
    chat.send(wait.clone(), "Is a list a monad");
    chat.send(wait.clone(), "  is a  LIST a monad ");
    chat.send(wait, "is a list a monoid");
    assert_eq!(
        open_questions(&chat.rows()),
        ["Is a list a monad", "is a list a monoid"]
    );
    chat.replies(&["Yes."]);
    chat.send(qwen(), "IS A LIST\ta monad");
    assert_eq!(open_questions(&chat.rows()), ["is a list a monoid"]);
}
