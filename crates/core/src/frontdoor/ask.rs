//! #384: the front door as one call, for every face. The Chat view's Send, the CLI's `ask` and
//! MCP's `ask` all route and answer here (FD-1, FD-11); "Send as a fleet job" (FD-12) is
//! `send_as_job`. A dry run routes and stops: nothing is sent, stored or spent.

use super::adapters::{ChatTransport, HttpChatTransport};
use super::answer::{self, AnswerContext, ChatReply};
use super::chat;
use super::labeller::SmallLabeller;
use super::router;
use super::thread::ThreadStore;
use crate::abilities::FleetModels;
use crate::fleet::FleetBridge;
use crate::CoreError;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// What one ask came to: the Chat view's reply (the route, and the answer when one was made),
/// and whether it was a dry run (the route alone).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Asked {
    #[serde(flatten)]
    pub reply: ChatReply,
    pub dry_run: bool,
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
pub fn ask_on(
    fleet: &FleetBridge,
    data_dir: &Path,
    message: &str,
    handoff_id: Option<&str>,
    dry_run: bool,
    seams: &Seams,
) -> Result<Asked, CoreError> {
    let route = chat::route_in_thread_on(
        fleet,
        seams.machine,
        data_dir,
        handoff_id,
        message,
        chat::estimate_complexity(message),
        |message, index, gate, complexity| {
            router::route(message, index, gate, complexity, seams.small)
        },
    )?;
    let fallback = chat::qwen_fallback_on(fleet, seams.machine, &route, seams.qwen_up)?;
    if dry_run {
        let reply = match fallback {
            Some(fallback) => ChatReply {
                fallback: Some(fallback.reason),
                ..answer::route_alone(fallback.route)
            },
            None => answer::route_alone(route),
        };
        return Ok(Asked { reply, dry_run });
    }
    let qwen = answer::qwen_base_url(fleet, seams.machine);
    let ctx = AnswerContext {
        data_dir,
        qwen_base_url: qwen.as_deref(),
        env: seams.env,
        transport: seams.transport,
    };
    let reply = match fallback {
        Some(fallback) => {
            answer::answer_fallback(fallback.route, &fallback.reason, message, handoff_id, &ctx)?
        }
        None => answer::answer_route(route, message, handoff_id, &ctx)?,
    };
    Ok(Asked { reply, dry_run })
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
