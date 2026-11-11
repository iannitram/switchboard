//! #384: the front door from the CLI, each a thin call into the core the Chat view calls.
//! `ask "<text>" [--dry-run] [--thread <id>]` routes one message and answers it
//! (`frontdoor::ask::ask_headless`); `--dry-run` prints the route and its reason and sends,
//! stores and spends nothing, so it needs no role. `ask_as_job <kind> "<text>" --unmet
//! <need>[,<need>] [--thread <id>]` is the Chat view's "Send as a fleet job"
//! (`ask::send_as_job`; with `--thread` the job carries that chat's packet, FD-6); `rate
//! <thread> <turn> <up|down>` is its thumbs (`rating_log::rate_turn`). The three writes go
//! through `rules::guard_front_door`: the project-manager role, never in a job worktree.
//! A bad line is exit 2 with the usage; a refusal or a core error is its text on stderr, exit 1.

use std::io::Write;
use std::process::ExitCode;

use serde_json::Value;
use switchboard_core::error::CoreError;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::frontdoor::ask::{self, Asked};
use switchboard_core::frontdoor::chat::ChatPick;
use switchboard_core::frontdoor::rating_log;
use switchboard_core::rules::{self, Caller, Verdict};

use super::{data_or_fail, emit_to, pipeline_or_fail, refuse, terminal_safe, Fail, USAGE};

/// The front door's flags taken out of `argv`: `ask`'s `--dry-run`, `ask` and `ask_as_job`'s
/// `--thread <id>`, `ask_as_job`'s `--unmet <need>[,<need>]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flags {
    pub dry_run: bool,
    pub thread: Option<String>,
    pub unmet: Option<String>,
}

/// `Flags` out of `argv`, with the other words kept in order. A value flag with no value is an
/// error.
pub fn take_flags(argv: &[String]) -> Result<(Flags, Vec<String>), String> {
    let mut flags = Flags::default();
    let mut rest = Vec::new();
    let mut words = argv.iter();
    while let Some(word) = words.next() {
        let slot = match word.as_str() {
            "--thread" => &mut flags.thread,
            "--unmet" => &mut flags.unmet,
            "--dry-run" => {
                flags.dry_run = true;
                continue;
            }
            _ => {
                rest.push(word.clone());
                continue;
            }
        };
        match words.next() {
            Some(value) if !value.starts_with("--") => *slot = Some(value.clone()),
            _ => return Err(format!("{word} needs a value")),
        }
    }
    Ok((flags, rest))
}

/// The usage error for a front door flag given to a command it is not for.
pub fn misplaced(flags: &Flags, command: Option<&str>) -> Option<&'static str> {
    // `--thread` also names the chat `ask_as_job` sends (FD-6).
    let for_ask = flags.dry_run && command != Some("ask");
    if for_ask || (flags.thread.is_some() && !matches!(command, Some("ask" | "ask_as_job"))) {
        return Some("--dry-run and --thread are for ask");
    }
    if flags.unmet.is_some() && command != Some("ask_as_job") {
        return Some("--unmet is for ask_as_job");
    }
    None
}

/// One front door command line, read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Ask {
        message: String,
        thread: Option<String>,
        dry_run: bool,
    },
    AskAsJob {
        kind: String,
        request: String,
        unmet: Vec<String>,
        thread: Option<String>,
    },
    Rate {
        thread: String,
        turn: u32,
        rating: String,
    },
}

/// The call `words` (the command and its words) and `flags` ask for, or the usage error.
fn call_from(words: &[String], flags: &Flags) -> Result<Call, String> {
    let command = words.first().map(String::as_str).unwrap_or_default();
    match (command, words.get(1..).unwrap_or_default()) {
        ("ask", [message]) => Ok(Call::Ask {
            message: message.clone(),
            thread: flags.thread.clone(),
            dry_run: flags.dry_run,
        }),
        ("ask", _) => Err("ask needs one \"<text>\" (quote it)".to_string()),
        ("ask_as_job", [kind, request]) => {
            let unmet: Vec<String> = flags
                .unmet
                .as_deref()
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|need| !need.is_empty())
                .map(str::to_string)
                .collect();
            if unmet.is_empty() {
                return Err("ask_as_job needs --unmet <need>[,<need>]".to_string());
            }
            Ok(Call::AskAsJob {
                kind: kind.clone(),
                request: request.clone(),
                unmet,
                thread: flags.thread.clone(),
            })
        }
        ("ask_as_job", _) => Err("ask_as_job needs <kind> \"<text>\"".to_string()),
        ("rate", [thread, turn, rating]) => {
            let Ok(turn) = turn.parse::<u32>() else {
                return Err(format!(
                    "rate: turn must be a whole number from 0, got {}",
                    terminal_safe(turn)
                ));
            };
            if rating_log::thumbs_from_name(rating).is_none() {
                return Err(format!(
                    "rate: rating must be up or down, got {}",
                    terminal_safe(rating)
                ));
            }
            Ok(Call::Rate {
                thread: thread.clone(),
                turn,
                rating: rating.clone(),
            })
        }
        _ => Err("rate needs <thread> <turn> <up|down>".to_string()),
    }
}

/// Text from a model or the fleet, safe for a terminal line by line (#352): the line breaks
/// stay, every other control character is escaped.
fn safe_lines(text: &str) -> String {
    text.lines()
        .map(terminal_safe)
        .collect::<Vec<_>>()
        .join("\n")
}

/// `ask` as text: where the message goes and why, then what came of it.
fn ask_text(asked: &Asked) -> String {
    let reply = &asked.reply;
    let (to, why) = match &reply.route.pick {
        ChatPick::Service { service, model } => (format!("{service} {model}"), None),
        ChatPick::ClaudeHandoff { reason } => ("claude handoff".to_string(), Some(reason)),
        ChatPick::Wait { reason } => ("wait".to_string(), Some(reason)),
    };
    let mut reason = reply.route.step.clone();
    if let Some(kind) = &reply.route.kind {
        reason.push_str(&format!(", kind {kind}"));
    }
    if let Some(why) = why {
        reason.push_str(&format!(": {why}"));
    }
    let mut lines = vec![
        format!("route: {}", terminal_safe(&to)),
        format!("reason: {}", terminal_safe(&reason)),
    ];
    if asked.dry_run {
        lines.push("dry run: nothing sent, nothing stored, nothing spent".to_string());
    }
    let mut line = |name: &str, value: &Option<String>| {
        if let Some(value) = value {
            lines.push(format!("{name}: {}", terminal_safe(value)));
        }
    };
    line("fallback", &reply.fallback);
    line("thread", &reply.handoff_id);
    line("moved from", &reply.moved_from);
    line("moved because", &reply.moved_why);
    line("queued", &reply.queued);
    line("handoff", &reply.handoff);
    for (name, body) in [("packet", &reply.packet), ("answer", &reply.text)] {
        if let Some(body) = body {
            lines.push(format!("{name}:\n{}", safe_lines(body)));
        }
    }
    lines.join("\n")
}

/// Run `call` in the core: the text to print, and the same as JSON.
fn run_call(call: &Call) -> Result<(String, Value), Fail> {
    let core = |command: &str, e: CoreError| Fail {
        code: 1,
        message: terminal_safe(&format!("{command}: {e}")),
    };
    match call {
        Call::Ask {
            message,
            thread,
            dry_run,
        } => {
            let (root, data) = (pipeline_or_fail()?, data_or_fail()?);
            let fleet = FleetBridge::new(&root);
            let asked = ask::ask_headless(&fleet, &data, message, thread.as_deref(), *dry_run)
                .map_err(|e| core("ask", e))?;
            Ok((ask_text(&asked), serde_json::to_value(&asked)?))
        }
        Call::AskAsJob {
            kind,
            request,
            unmet,
            thread,
        } => {
            let fleet = FleetBridge::new(&pipeline_or_fail()?);
            let data = match thread {
                Some(_) => Some(data_or_fail()?),
                None => None,
            };
            let chat = data.as_deref().zip(thread.as_deref());
            let job = ask::send_as_job(&fleet, kind, request, unmet, chat)
                .map_err(|e| core("ask_as_job", e))?;
            let text = terminal_safe(&format!(
                "ask_as_job: queued {kind} for another machine (needs {})",
                unmet.join(", ")
            ));
            Ok((text, job))
        }
        Call::Rate {
            thread,
            turn,
            rating,
        } => {
            let row = rating_log::rate_turn(&data_or_fail()?, thread, *turn, rating)
                .map_err(|e| core("rate", e))?;
            let text = terminal_safe(&format!(
                "rate {thread}#{turn}: {rating} for {} {}",
                row.lane.as_deref().unwrap_or_default(),
                row.model.as_deref().unwrap_or_default()
            ));
            Ok((text, serde_json::to_value(&row)?))
        }
    }
}

/// CLI-3 (#384): one front door command. `words` is the command and its words, `caller` the
/// CLI's guard caller (role and job worktree). `--json` prints what the app's command of the
/// same press answers: the reply with `dry_run`, the queued job, the rating row.
pub fn command(
    words: &[String],
    json: bool,
    flags: &Flags,
    caller: &Caller,
    err: &mut dyn Write,
    out: &mut dyn Write,
) -> ExitCode {
    let call = match call_from(words, flags) {
        Ok(call) => call,
        Err(e) => {
            let _ = writeln!(err, "switchboard: {e}");
            let _ = writeln!(err, "{USAGE}");
            return ExitCode::from(2);
        }
    };
    // A dry run only reads, so anyone may ask for one.
    if !matches!(call, Call::Ask { dry_run: true, .. }) {
        let what = words.first().map(String::as_str).unwrap_or_default();
        if let Verdict::Refuse(text) | Verdict::ToWaiting(text) =
            rules::guard_front_door(caller, what)
        {
            return refuse(err, &terminal_safe(&text));
        }
    }
    let (text, value) = match run_call(&call) {
        Ok(done) => done,
        Err(fail) => {
            let _ = writeln!(err, "{}", fail.message);
            return ExitCode::from(fail.code);
        }
    };
    let text = if json {
        match serde_json::to_string_pretty(&value) {
            Ok(text) => text,
            Err(e) => {
                let _ = writeln!(err, "switchboard: {e}");
                return ExitCode::from(1);
            }
        }
    } else {
        text
    };
    emit_to(out, &format!("{text}\n"))
}
