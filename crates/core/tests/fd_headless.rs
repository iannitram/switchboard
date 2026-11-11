//! #384: the front door for a caller with no window. `ask::ask_on` routes and answers as the
//! Chat view's Send does and a dry run stops at the route; `rating_log::rate_turn` rates a turn
//! named by thread and index; `rules::guard_front_door` says who may. The fleet is the fixture
//! stub, the transport a fake that records: no service is called.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::frontdoor::adapters::ChatTransport;
use switchboard_core::frontdoor::ask::{ask_on, known_thread, Asked, Seams};
use switchboard_core::frontdoor::chat::ChatPick;
use switchboard_core::frontdoor::rating_log::rate_turn;
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::rules::{guard_front_door, Caller, Face, Role, Verdict};
use switchboard_core::CoreError;

const MACHINE: &str = "strix-halo";
const ASK: &str = "what is a worktree";

type Seen = Arc<Mutex<Vec<String>>>;

/// Records each URL it is asked to post to and answers as a chat completion.
struct Fake {
    seen: Seen,
}

impl ChatTransport for Fake {
    fn post_json(&self, url: &str, _body: &Value) -> Result<Value, CoreError> {
        self.seen.lock().unwrap().push(url.to_string());
        Ok(json!({
            "choices": [{"message": {"content": "A second checkout."}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 4}
        }))
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

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-fd384-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A pipeline root under `dir` with the fixture stub and a config that names this machine's
/// qwen model and server.
fn fleet(dir: &Path) -> FleetBridge {
    fleet_with(dir, &|_| {})
}

/// `fleet` with the budget snapshot changed by `edit`.
fn fleet_with(dir: &Path, edit: &dyn Fn(&mut Value)) -> FleetBridge {
    let stub = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fleet-stub");
    let root = dir.join("pipeline");
    for sub in ["tools/fleet", "answers"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    for file in [
        "tools/fleet/fleet.py",
        "answers/actions.json",
        "answers/budget.json",
    ] {
        std::fs::copy(stub.join(file), root.join(file)).unwrap();
    }
    // #430: a dry run reads `fleet budget --json`, which names the local qwen pool too.
    let budget_file = root.join("answers/budget.json");
    let mut budget: Value =
        serde_json::from_str(&std::fs::read_to_string(&budget_file).unwrap()).unwrap();
    budget["qwen"] = json!({ "verdict": "on-pace", "stale": false });
    edit(&mut budget);
    std::fs::write(&budget_file, budget.to_string()).unwrap();
    let config = json!({ "machines": { MACHINE: {
        "qwen_capable": true,
        "qwen_model": "qwen3.8-27b",
        "qwen_server_url": "http://127.0.0.1:9/v1/models"
    } } });
    std::fs::write(root.join("tools/fleet/config.json"), config.to_string()).unwrap();
    FleetBridge::with_root(&root)
}

/// `ask_on` on this machine with no keys and the fake transport; what the transport saw.
fn ask(
    dir: &Path,
    message: &str,
    thread: Option<&str>,
    dry_run: bool,
) -> (Result<Asked, CoreError>, Vec<String>) {
    ask_over(dir, message, thread, dry_run, &|_| {})
}

/// `ask` with the budget snapshot changed by `edit`.
fn ask_over(
    dir: &Path,
    message: &str,
    thread: Option<&str>,
    dry_run: bool,
    edit: &dyn Fn(&mut Value),
) -> (Result<Asked, CoreError>, Vec<String>) {
    let seen: Seen = Arc::default();
    let env = |_: &str| None;
    let transport = {
        let seen = seen.clone();
        move || -> Box<dyn ChatTransport> { Box::new(Fake { seen: seen.clone() }) }
    };
    let seams = Seams {
        machine: Some(MACHINE),
        small: None,
        env: &env,
        transport: &transport,
        qwen_up: &|_| false,
    };
    let asked = ask_on(
        &fleet_with(dir, edit),
        &dir.join("data"),
        message,
        thread,
        dry_run,
        &seams,
    );
    let seen = seen.lock().unwrap().clone();
    (asked, seen)
}

fn qwen() -> ChatPick {
    ChatPick::Service {
        service: "qwen".to_string(),
        model: "qwen3.8-27b".to_string(),
    }
}

// --- ask ---

#[test]
fn a_dry_run_is_the_route_alone_and_sends_and_stores_nothing() {
    let dir = scratch("dry");
    let (asked, seen) = ask(&dir, ASK, None, true);
    let asked = asked.unwrap();
    assert!(asked.dry_run);
    assert_eq!(asked.reply.route.pick, qwen());
    assert_eq!(asked.reply.route.kind.as_deref(), Some("quick-lookup"));
    assert_eq!(asked.reply.route.step, "labeller");
    assert_eq!(asked.reply.handoff_id, None);
    assert_eq!(asked.reply.text, None);
    assert!(seen.is_empty(), "{seen:?}");
    assert!(!dir.join("data").exists());
    let wire = serde_json::to_value(&asked).unwrap();
    assert_eq!(
        wire,
        json!({
            "step": "labeller",
            "kind": "quick-lookup",
            "pick": { "to": "service", "service": "qwen", "model": "qwen3.8-27b" },
            "dry_run": true,
            "budget_snapshot": "stale (chatgpt 138 min old)"
        })
    );
}

/// #433: `fleet budget allow qwen` (no `--machine`, as a real ask calls it) lets the local lane
/// through even when every Qwen machine is busy, so the dry run must too.
#[test]
fn a_dry_run_lets_qwen_through_when_every_qwen_machine_is_busy() {
    let dir = scratch("dry-qwen-busy");
    let busy = |budget: &mut Value| {
        budget["qwen"] = json!({
            "verdict": "brake",
            "stale": false,
            "why": "all qwen machines busy: strix-halo"
        });
    };
    let (asked, seen) = ask_over(&dir, ASK, None, true, &busy);
    let asked = asked.unwrap();
    assert_eq!(asked.reply.route.pick, qwen());
    assert!(seen.is_empty());
    assert!(!dir.join("data").exists());
}

/// #433: a dry run whose route asks about a stale auto-read pool does not refresh it, but says
/// a real ask would, and how old the reading it used is.
#[test]
fn a_dry_run_says_a_real_ask_would_refresh_a_stale_auto_read_pool_first() {
    let dir = scratch("dry-refresh");
    let fresh_chatgpt = |budget: &mut Value| {
        budget["chatgpt"]["verdict"] = json!("on-pace");
    };
    let message = "search the web for the latest rust release";
    let (asked, seen) = ask_over(&dir, message, None, true, &fresh_chatgpt);
    let asked = asked.unwrap();
    assert_eq!(asked.reply.route.kind.as_deref(), Some("web-research"));
    assert_eq!(
        asked.refresh_first,
        ["a real ask would refresh chatgpt first; this answer uses the reading from 138 min ago"]
    );
    let wire = serde_json::to_value(&asked).unwrap();
    assert_eq!(
        wire["refresh_first"],
        json!([
            "a real ask would refresh chatgpt first; this answer uses the reading from 138 min ago"
        ])
    );
    assert!(seen.is_empty());
    assert!(!dir.join("data").exists(), "the dry run writes nothing");
    // A route that never asks about the stale pool says nothing of it.
    let (asked, _) = ask_over(&dir, ASK, None, true, &fresh_chatgpt);
    assert!(asked.unwrap().refresh_first.is_empty());
}

#[test]
fn an_ask_is_answered_and_both_turns_are_stored_under_the_route_the_dry_run_gave() {
    let dir = scratch("real");
    let (dry, _) = ask(&dir, ASK, None, true);
    let (asked, seen) = ask(&dir, ASK, None, false);
    let asked = asked.unwrap();
    assert!(!asked.dry_run);
    assert_eq!(asked.reply.route, dry.unwrap().reply.route);
    assert_eq!(asked.reply.text.as_deref(), Some("A second checkout."));
    assert_eq!(asked.reply.service.as_deref(), Some("qwen"));
    assert_eq!(seen, vec!["http://127.0.0.1:9/v1/chat/completions"]);
    let id = asked.reply.handoff_id.unwrap();
    let turns = ThreadStore::new(&dir.join("data")).turns(&id).unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(serde_json::to_value(asked.dry_run).unwrap(), json!(false));
}

#[test]
fn an_empty_message_is_refused_dry_or_not() {
    let dir = scratch("empty");
    for dry_run in [true, false] {
        let (asked, seen) = ask(&dir, "  ", None, dry_run);
        assert_eq!(
            asked.unwrap_err(),
            CoreError::Refused("chat: empty message".to_string())
        );
        assert!(seen.is_empty());
    }
}

#[test]
fn known_thread_refuses_a_thread_with_no_rows() {
    let dir = scratch("known");
    let (asked, _) = ask(&dir, ASK, None, false);
    let id = asked.unwrap().reply.handoff_id.unwrap();
    assert_eq!(known_thread(&dir.join("data"), &id), Ok(()));
    assert_eq!(
        known_thread(&dir.join("data"), "chat-nope"),
        Err(CoreError::Refused("no thread chat-nope".to_string()))
    );
    assert!(known_thread(&dir.join("data"), "../x").is_err());
}

// --- rate_turn ---

fn refused(text: &str) -> CoreError {
    CoreError::Refused(text.to_string())
}

#[test]
fn rate_turn_reads_the_service_model_and_kind_from_the_turn() {
    let dir = scratch("rate");
    let data = dir.join("data");
    let (asked, _) = ask(&dir, ASK, None, false);
    let id = asked.unwrap().reply.handoff_id.unwrap();
    let row = rate_turn(&data, &id, 1, "down").unwrap();
    assert_eq!(row.lane.as_deref(), Some("qwen"));
    assert_eq!(row.model.as_deref(), Some("qwen3.8-27b"));
    assert_eq!(row.kind.as_deref(), Some("quick-lookup"));
    assert_eq!(row.id, Some(format!("{id}#rating-1")));
    let store = ThreadStore::new(&data);
    assert_eq!(store.read(&id).unwrap().len(), 3);
    // A second rating of the turn replaces the first.
    let again = rate_turn(&data, &id, 1, "up").unwrap();
    assert_ne!(again.rating, row.rating);
    let rows = store.read(&id).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2], again);
}

#[test]
fn rate_turn_refuses_what_log_rating_refuses_and_writes_nothing() {
    let dir = scratch("rate-refused");
    let data = dir.join("data");
    let (asked, _) = ask(&dir, ASK, None, false);
    let id = asked.unwrap().reply.handoff_id.unwrap();
    let before = ThreadStore::new(&data).read(&id).unwrap();
    assert_eq!(
        rate_turn(&data, &id, 1, "sideways"),
        Err(refused("unknown rating: sideways"))
    );
    assert_eq!(
        rate_turn(&data, &id, 0, "up"),
        Err(refused(&format!("turn 0 of thread {id} is not an answer")))
    );
    assert_eq!(
        rate_turn(&data, &id, 2, "up"),
        Err(refused(&format!("no turn 2 in thread {id}")))
    );
    assert_eq!(
        rate_turn(&data, "chat-nope", 1, "up"),
        Err(refused("no thread chat-nope"))
    );
    assert_eq!(ThreadStore::new(&data).read(&id).unwrap(), before);
    assert!(!data.join("threads").join("chat-nope.jsonl").exists());
}

// --- guard_front_door ---

fn caller(face: Face, role: Option<Role>, in_job_worktree: bool) -> Caller {
    Caller {
        face,
        role,
        in_job_worktree,
        confirmed_in_app: false,
    }
}

#[test]
fn the_front_door_is_the_apps_and_the_project_managers() {
    assert_eq!(
        guard_front_door(&caller(Face::Gui, None, false), "ask"),
        Verdict::Allow
    );
    for face in [Face::Cli, Face::Mcp] {
        assert_eq!(
            guard_front_door(&caller(face, Some(Role::ProjectManager), false), "ask"),
            Verdict::Allow
        );
        for role in [None, Some(Role::BudgetManager), Some(Role::Satellite)] {
            assert_eq!(
                guard_front_door(&caller(face, role, false), "rate"),
                Verdict::Refuse(
                    "rate: only the project-manager role may use the front door".to_string()
                )
            );
        }
    }
}

#[test]
fn nothing_passes_the_front_door_inside_a_job_worktree() {
    for (face, role) in [
        (Face::Gui, None),
        (Face::Cli, Some(Role::ProjectManager)),
        (Face::Mcp, Some(Role::ProjectManager)),
    ] {
        assert_eq!(
            guard_front_door(&caller(face, role, true), "ask_as_job"),
            Verdict::Refuse("ask_as_job: refused inside a job worktree".to_string())
        );
    }
}

// --- FD-12: held requests and their unmet needs (#429, #433) ---

use switchboard_core::frontdoor::ask::{held_request, infer_unmet, unmet_needs, Held};
use switchboard_core::it::Loadout;

#[test]
fn unmet_needs_are_the_request_kinds_needs_the_loadout_does_not_meet() {
    let dir = scratch("unmet");
    let fleet = fleet(&dir);
    let bare = Loadout::default();
    assert_eq!(
        unmet_needs(&fleet, Some(MACHINE), "web-research", &bare),
        Ok(vec!["web".to_string()])
    );
    let web = Loadout {
        toolchains: vec!["web".to_string()],
        ..Loadout::default()
    };
    assert_eq!(
        unmet_needs(&fleet, Some(MACHINE), "web-research", &web),
        Ok(vec![])
    );
    for kind in ["no-such-kind", "small-code-change-exact-brief"] {
        assert_eq!(
            unmet_needs(&fleet, Some(MACHINE), kind, &bare),
            Err(refused(&format!("no request kind {kind}"))),
            "not a request kind: {kind}"
        );
    }
}

#[test]
fn ask_as_job_infers_its_unmet_needs_and_refuses_a_kind_this_machine_meets() {
    let dir = scratch("infer");
    let fleet = fleet(&dir);
    let bare = Loadout::default();
    assert_eq!(
        infer_unmet(&fleet, "web-research", &bare),
        Ok(vec!["web".to_string()])
    );
    assert_eq!(
        infer_unmet(&fleet, "quick-lookup", &bare),
        Err(refused(
            "this machine meets every need of quick-lookup: nothing to send as a job"
        ))
    );
}

#[test]
fn a_routed_request_is_held_only_when_the_loadout_misses_a_need() {
    let dir = scratch("held");
    let fleet = fleet(&dir);
    let message = "search the web for the latest rust release";
    let (asked, _) = ask(&dir, message, None, true);
    let reply = asked.unwrap().reply;
    let bare = Loadout::default();
    assert_eq!(
        held_request(&fleet, Some(MACHINE), &reply, message, Some(&bare)),
        Some(Held {
            kind: "web-research".to_string(),
            request: message.to_string(),
            unmet: vec!["web".to_string()],
        })
    );
    assert_eq!(
        held_request(&fleet, Some(MACHINE), &reply, message, None),
        None,
        "the loadout not read yet: nothing is held"
    );
    let (asked, _) = ask(&dir, ASK, None, true);
    let lookup = asked.unwrap().reply;
    assert_eq!(
        held_request(&fleet, Some(MACHINE), &lookup, ASK, Some(&bare)),
        None,
        "a kind with no needs is never held"
    );
}
