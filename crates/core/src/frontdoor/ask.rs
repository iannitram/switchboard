//! #384: the front door as one call, for every face. The Chat view's Send, the CLI's `ask` and
//! MCP's `ask` all route and answer here (FD-1, FD-11); "Send as a fleet job" (FD-12) is
//! `send_as_job`. A dry run routes and stops: nothing is sent, stored or spent.

use super::adapters::{ChatTransport, HttpChatTransport};
use super::answer::{self, AnswerContext, ChatReply};
use super::chat;
use super::labeller::SmallLabeller;
use super::router;
use super::thread::ThreadStore;
use crate::abilities::{AbilitiesIndex, FleetModels};
use crate::fleet::FleetBridge;
use crate::it::Loadout;
use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What one ask came to: the Chat view's reply (the route, and the answer when one was made),
/// and whether it was a dry run (the route alone).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Asked {
    #[serde(flatten)]
    pub reply: ChatReply,
    pub dry_run: bool,
    /// #430: on a dry run whose budget snapshot holds a stale reading, how old:
    /// `stale (<pool> <age> min old, ...)` (`chat::SnapshotGate::stale_line`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_snapshot: Option<String>,
    /// #433: on a dry run, one line per stale auto-read pool the route asked about, which a real
    /// ask would refresh before trusting (`chat::SnapshotGate::refresh_lines`): `a real ask would
    /// refresh <pool> first; this answer uses the reading from <N> min ago`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refresh_first: Vec<String>,
}

/// What an ask reads from outside the core; `ask` passes the real ones.
pub struct Seams<'a> {
    /// This machine's fleet name (`FleetModels::local_machine`); None: no local model.
    pub machine: Option<&'a str>,
    /// FD-2's small labeller, asked only when the rules name no kind.
    pub small: Option<&'a dyn SmallLabeller>,
    /// The value of an environment variable, by name.
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// Makes a transport for one call.
    pub transport: &'a dyn Fn() -> Box<dyn ChatTransport>,
    /// FD-3 (#377): whether the local Qwen server answers now, asked with the fleet config's
    /// `qwen_server_url` for this machine.
    pub qwen_up: &'a dyn Fn(&str) -> bool,
}

/// `ask` over its seams. The route is `chat::route_in_thread_on` with the real router; a wait
/// for budget then falls back to the local Qwen lane when it is up (`chat::qwen_fallback_on`,
/// FD-3). Unless `dry_run`, `answer` then answers it and stores the turn; a dry run's reply is
/// the route (and the fallback's reason) alone.
///
/// #430: a dry run never asks `fleet budget allow` (which may refresh a stale paid pool's
/// reading) and writes nothing: it routes over the budget snapshot on disk
/// (`chat::SnapshotGate`) and says how old the snapshot is when a reading in it is stale.
/// #433: it says which stale auto-read pool a real ask would refresh first, and lets the
/// local Qwen lane through as `fleet budget allow qwen` does.
pub fn ask_on(
    fleet: &FleetBridge,
    data_dir: &Path,
    message: &str,
    handoff_id: Option<&str>,
    dry_run: bool,
    seams: &Seams,
) -> Result<Asked, CoreError> {
    let router = |message: &str, index: &_, gate: &dyn router::BudgetGate, complexity| {
        router::route(message, index, gate, complexity, seams.small)
    };
    let complexity = chat::estimate_complexity(message);
    if dry_run {
        let snapshot = chat::SnapshotGate::read(fleet)?;
        let route = chat::route_in_thread_gated(
            fleet,
            seams.machine,
            data_dir,
            handoff_id,
            message,
            complexity,
            &snapshot,
            router,
        )?;
        let fallback =
            chat::qwen_fallback_gated(fleet, seams.machine, &route, &snapshot, seams.qwen_up)?;
        let reply = match fallback {
            Some(fallback) => ChatReply {
                fallback: Some(fallback.reason),
                ..answer::route_alone(fallback.route)
            },
            None => answer::route_alone(route),
        };
        return Ok(Asked {
            reply,
            dry_run,
            budget_snapshot: snapshot.stale_line(),
            refresh_first: snapshot.refresh_lines(),
        });
    }
    let route = chat::route_in_thread_on(
        fleet,
        seams.machine,
        data_dir,
        handoff_id,
        message,
        complexity,
        router,
    )?;
    let fallback = chat::qwen_fallback_on(fleet, seams.machine, &route, seams.qwen_up)?;
    let qwen = answer::qwen_base_url(fleet, seams.machine);
    let ctx = AnswerContext {
        data_dir,
        qwen_base_url: qwen.as_deref(),
        env: seams.env,
        transport: seams.transport,
    };
    let reply = match fallback {
        Some(fallback) => answer::answer_fallback(fallback, message, handoff_id, &ctx)?,
        None => answer::answer_route(route, message, handoff_id, &ctx)?,
    };
    Ok(Asked {
        reply,
        dry_run,
        budget_snapshot: None,
        refresh_first: Vec::new(),
    })
}

/// FD-1, FD-11: route one message in thread `handoff_id` (None starts one) and answer it, as
/// the Chat composer's Send does: this machine's name, the live labeller, the process
/// environment for the keys and the real HTTP transport. With `dry_run` it routes and stops:
/// no model is sent the message, no thread is written and nothing is spent (the labeller, a
/// local model, may still be asked for the kind, so the route is the one a real ask takes).
pub fn ask(
    fleet: &FleetBridge,
    data_dir: &Path,
    message: &str,
    handoff_id: Option<&str>,
    dry_run: bool,
) -> Result<Asked, CoreError> {
    let machine = FleetModels::local_machine();
    let small = chat::live_labeller(fleet, machine.as_deref());
    let env = |name: &str| std::env::var(name).ok();
    let transport = || -> Box<dyn ChatTransport> { Box::new(HttpChatTransport::new()) };
    let seams = Seams {
        machine: machine.as_deref(),
        small: small.as_ref().map(|s| s as &dyn SmallLabeller),
        env: &env,
        transport: &transport,
        qwen_up: &crate::autopilot::passes::http_qwen_probe,
    };
    ask_on(fleet, data_dir, message, handoff_id, dry_run, &seams)
}

/// A caller with no window names a thread by hand, so the thread must be there: one that has
/// no stored row is refused (the window only names threads it opened).
pub fn known_thread(data_dir: &Path, handoff_id: &str) -> Result<(), CoreError> {
    if ThreadStore::new(data_dir).exists(handoff_id)? {
        Ok(())
    } else {
        Err(CoreError::Refused(format!("no thread {handoff_id}")))
    }
}

/// `ask` for the CLI and MCP: a named thread must exist (`known_thread`).
pub fn ask_headless(
    fleet: &FleetBridge,
    data_dir: &Path,
    message: &str,
    handoff_id: Option<&str>,
    dry_run: bool,
) -> Result<Asked, CoreError> {
    if let Some(id) = handoff_id {
        known_thread(data_dir, id)?;
    }
    ask(fleet, data_dir, message, handoff_id, dry_run)
}

/// FD-12: send a held request as a fleet job another machine pulls (`it::send_held_job`), from
/// this machine: its name is `FLEET_MACHINE`, else the machine file under the home folder
/// (`it::this_machine`). With `thread` (the app data folder and the chat's handoff id) the job
/// carries the chat's packet and the move is stored (`answer::send_thread_as_job`, FD-6);
/// without one the request goes as it is.
pub fn send_as_job(
    fleet: &FleetBridge,
    kind: &str,
    request: &str,
    unmet: &[String],
    thread: Option<(&Path, &str)>,
) -> Result<serde_json::Value, CoreError> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let env_name = std::env::var("FLEET_MACHINE").ok();
    let machine = crate::it::this_machine(env_name.as_deref(), &home).unwrap_or_default();
    let mut send = |text: &str| crate::it::send_held_job(fleet, kind, text, unmet, &machine);
    let Some((data_dir, handoff_id)) = thread else {
        return send(request);
    };
    let qwen = answer::qwen_base_url(fleet, FleetModels::local_machine().as_deref());
    let env = |name: &str| std::env::var(name).ok();
    let transport = || -> Box<dyn ChatTransport> { Box::new(HttpChatTransport::new()) };
    let ctx = AnswerContext {
        data_dir,
        qwen_base_url: qwen.as_deref(),
        env: &env,
        transport: &transport,
    };
    answer::send_thread_as_job(handoff_id, kind, request, &job_why(unmet), &ctx, &mut send)
}

/// Why a chat sent as a fleet job moved, for its work-tree line.
pub fn job_why(unmet: &[String]) -> String {
    if unmet.is_empty() {
        "I sent it as a fleet job".to_string()
    } else {
        format!("I sent it as a fleet job (needs {})", unmet.join(", "))
    }
}

/// FD-12 (#429): a request this machine holds, which the Chat view offers to send as a fleet
/// job: its kind, the request text, and the needs this machine's loadout does not meet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    pub kind: String,
    pub request: String,
    pub unmet: Vec<String>,
}

/// FD-12 (#429, #433): the needs of request kind `kind` that `loadout` does not meet
/// (`it::unmet`). A kind the abilities index does not list as a request kind is refused.
pub fn unmet_needs(
    fleet: &FleetBridge,
    machine: Option<&str>,
    kind: &str,
    loadout: &Loadout,
) -> Result<Vec<String>, CoreError> {
    let index = AbilitiesIndex::load_with(fleet, machine)?;
    let ability = index
        .get(kind)
        .filter(|ability| ability.request)
        .ok_or_else(|| CoreError::Refused(format!("no request kind {kind}")))?;
    Ok(crate::it::unmet(&ability.needs, loadout))
}

/// FD-12 (#429): the request the Chat view holds for `reply`: its kind and the message, with
/// the needs `loadout` (this machine's, as last read) does not meet. None when the loadout has
/// not been read, the route names no kind or not a request kind, or every need is met.
pub fn held_request(
    fleet: &FleetBridge,
    machine: Option<&str>,
    reply: &ChatReply,
    message: &str,
    loadout: Option<&Loadout>,
) -> Option<Held> {
    let loadout = loadout?;
    let kind = reply.route.kind.as_deref()?;
    let unmet = unmet_needs(fleet, machine, kind, loadout).ok()?;
    (!unmet.is_empty()).then(|| Held {
        kind: kind.to_string(),
        request: message.to_string(),
        unmet,
    })
}

/// FD-12 (#433): the unmet needs for `ask_as_job` when the caller names none: request kind
/// `kind`'s needs against this machine's loadout, read now (`it::read_this_machine`). A kind
/// whose needs this machine meets is refused: there is nothing to hold.
pub fn infer_unmet(
    fleet: &FleetBridge,
    kind: &str,
    loadout: &Loadout,
) -> Result<Vec<String>, CoreError> {
    let unmet = unmet_needs(
        fleet,
        FleetModels::local_machine().as_deref(),
        kind,
        loadout,
    )?;
    if unmet.is_empty() {
        return Err(CoreError::Refused(format!(
            "this machine meets every need of {kind}: nothing to send as a job"
        )));
    }
    Ok(unmet)
}
