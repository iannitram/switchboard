//! `it`: this machine's loadout (IT-1), fleet errors as IT tickets (IT-2), `it-triage` jobs for
//! another machine (IT-3), and ladder-out coding errors as bugs in the project (IT-4).

use crate::clock::UnixSeconds;
use crate::fleet::{FleetBridge, FleetError, FleetErrorLog};
use crate::platform::{current_os, loadout_probes, Probe};
use crate::CoreError;
use serde::Serialize;
use std::path::Path;
use std::time::{Duration, Instant};

/// IT-1: what this machine can do right now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Loadout {
    pub read_at: UnixSeconds,
    /// CLI name and whether it is signed in.
    pub clis: Vec<(String, bool)>,
    pub models_loaded: Vec<String>,
    pub toolchains: Vec<String>,
    pub busy: bool,
    pub recent_errors: Vec<String>,
}

/// One probe's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub name: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
}

/// Runs probes (tests stub it).
pub trait ProbeRunner: Send + Sync {
    fn run(&self, probe: &Probe) -> ProbeResult;
}

/// IT-1: read the loadout from the platform's probe list.
/// It runs each probe once and never fails: a CLI is listed as not signed in, a toolchain is
/// left out, llama-swap gives no models and `fleet status` is not busy when its probe fails.
pub fn read_loadout(probes: &[Probe], runner: &dyn ProbeRunner, now: UnixSeconds) -> Loadout {
    let mut loadout = Loadout {
        read_at: now,
        ..Loadout::default()
    };
    for probe in probes {
        let result = runner.run(probe);
        let ok = result.exit_code == Some(0);
        match probe.name {
            name if LOADOUT_CLIS.contains(&name) => loadout.clis.push((name.to_string(), ok)),
            name if ok && LOADOUT_TOOLCHAINS.contains(&name) => {
                loadout.toolchains.push(name.to_string());
            }
            "llama-swap" if ok => loadout.models_loaded = running_models(&result.stdout),
            "fleet status" => loadout.busy = ok && result.stdout.contains("DOING"),
            "fleet errors" if ok => loadout.recent_errors = last_errors(&result.stdout),
            _ => {}
        }
    }
    loadout
}

/// IT-1: the CLI probes (signed in when the probe exits 0).
const LOADOUT_CLIS: [&str; 3] = ["claude", "codex", "gh"];

/// IT-1: the toolchain probes (present when `--version` exits 0). `gemini` is the Antigravity
/// CLI (`agy`), which has no auth-status command, so presence is all it reports.
const LOADOUT_TOOLCHAINS: [&str; 4] = ["cargo", "node", "python3", "gemini"];

/// IT-1: how many recent errors the loadout keeps.
const RECENT_ERRORS: usize = 5;

/// IT-1: the models in llama-swap's `{"running":[{"model":..}]}`; none when it is unreadable.
fn running_models(body: &str) -> Vec<String> {
    let Ok(answer) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    answer["running"]
        .as_array()
        .map(|running| {
            running
                .iter()
                .filter_map(|m| m["model"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// IT-1: the last `RECENT_ERRORS` non-blank lines, in the order they were printed.
fn last_errors(out: &str) -> Vec<String> {
    let lines: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    let skip = lines.len().saturating_sub(RECENT_ERRORS);
    lines.into_iter().skip(skip).collect()
}

/// IT-W: how long one probe may run before it is killed (#339). A probe that runs out of time
/// has no exit code, so the loadout shows it as not signed in, left out, or not busy.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// IT-W: the real probe runner. It runs the probe's program with its args, stdin and stderr
/// closed, and keeps the exit code and stdout; a program that cannot start, or that is still
/// running after `PROBE_TIMEOUT`, has no exit code (a slow one is killed).
pub struct SystemProbeRunner;

impl ProbeRunner for SystemProbeRunner {
    fn run(&self, probe: &Probe) -> ProbeResult {
        run_probe_within(probe, PROBE_TIMEOUT)
    }
}

/// Kill the probe's whole process group (the child is its leader). `kill` is the system
/// program, so the core needs no unsafe code or extra crate.
#[cfg(unix)]
fn kill_group(child: &std::process::Child) {
    let _ = std::process::Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn kill_group(_child: &std::process::Child) {}

/// IT-W: run `probe`, killing it when it is still going after `limit`.
pub fn run_probe_within(probe: &Probe, limit: Duration) -> ProbeResult {
    let none = || ProbeResult {
        name: probe.name.to_string(),
        exit_code: None,
        stdout: String::new(),
    };
    let mut command = std::process::Command::new(probe.program);
    command
        .args(probe.args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    // Its own process group, so a kill reaches what the probe started too (a shim script's
    // `sleep` would otherwise outlive it).
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let Ok(mut child) = command.spawn() else {
        return none();
    };
    // Read stdout on its own thread so a full pipe cannot stall the child. It is never joined
    // after a kill: a grandchild may still hold the pipe open.
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(mut out) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = std::io::Read::read_to_end(&mut out, &mut bytes);
            let _ = tx.send(bytes);
        });
    }
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                kill_group(&child);
                let _ = child.kill();
                let _ = child.wait();
                return none();
            }
        }
    };
    let bytes = rx.recv_timeout(Duration::from_secs(1)).unwrap_or_default();
    ProbeResult {
        name: probe.name.to_string(),
        exit_code: status.code(),
        stdout: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

/// IT-W: this machine's loadout: `read_loadout` over this OS's probe list.
pub fn read_this_machine(runner: &dyn ProbeRunner, now: UnixSeconds) -> Loadout {
    read_loadout(loadout_probes(current_os()), runner, now)
}

/// FD-12: the `needs` tags the loadout can't meet (empty when it meets them all).
pub fn unmet(needs: &[String], loadout: &Loadout) -> Vec<String> {
    needs
        .iter()
        .filter(|need| !is_met(need, loadout))
        .cloned()
        .collect()
}

/// FD-12: a need is met by a signed-in CLI of that name, a loaded model, or a toolchain.
fn is_met(need: &str, loadout: &Loadout) -> bool {
    loadout
        .clis
        .iter()
        .any(|(name, signed_in)| *signed_in && name == need)
        || loadout.models_loaded.iter().any(|m| m == need)
        || loadout.toolchains.iter().any(|t| t == need)
}

/// FD-12: the fleet job another machine pulls for a request this machine cannot serve.
/// None when `unmet` is empty (nothing to hold).
pub fn held_job(
    kind: &str,
    request: &str,
    unmet: &[String],
    this_machine: &str,
) -> Option<serde_json::Value> {
    if unmet.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "kind": kind,
        "request": request,
        "needs": unmet,
        "never_on": [this_machine],
    }))
}

/// FD-12: the fleet command that queues a held request (the job JSON on stdin). Until the pipeline
/// has this mode, fleet answers with an error and the press shows it.
pub const HELD_JOB_ARGS: [&str; 3] = ["attach", "--request", "-"];

/// FD-12: this machine's fleet name, read as fleet reads it: `FLEET_MACHINE`, else the first line
/// of `~/.config/software-depot/machine`, trimmed. None when neither gives a name.
pub fn this_machine(fleet_machine_env: Option<&str>, home: &Path) -> Option<String> {
    if let Some(name) = fleet_machine_env.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(name.to_string());
    }
    let path = home.join(".config").join("software-depot").join("machine");
    let text = std::fs::read_to_string(path).ok()?;
    let first = text.lines().next()?.trim();
    (!first.is_empty()).then(|| first.to_string())
}

/// FD-12: queue a held request as a fleet job another machine pulls. Exit 0 queued (the job is
/// returned); 75 not queued; any other exit or a failed call is the fleet's error.
pub fn send_held_job(
    fleet: &FleetBridge,
    kind: &str,
    request: &str,
    unmet: &[String],
    this_machine: &str,
) -> Result<serde_json::Value, CoreError> {
    let machine = this_machine.trim();
    if machine.is_empty() {
        return Err(CoreError::Refused(
            "fd-12: this machine has no fleet name (FLEET_MACHINE or the machine file)".to_string(),
        ));
    }
    let Some(job) = held_job(kind, request, unmet, machine) else {
        return Err(CoreError::Refused(
            "fd-12: nothing is unmet, so there is no job to send".to_string(),
        ));
    };
    let code = fleet
        .run_status_with_input(&HELD_JOB_ARGS, &job.to_string())
        .map_err(CoreError::Fleet)?;
    match code {
        0 => Ok(job),
        code => Err(CoreError::Refused(format!(
            "fd-12: fleet did not queue the job (exit {code})"
        ))),
    }
}

/// IT-2: one ticket per error signature; the repeat comments on it (`fleet errors file`).
pub fn ticket_title(signature: &str) -> String {
    format!("IT: {signature}")
}

/// IT-4: the `feature:` prefix of a star's label (SKY-10).
const FEATURE_PREFIX: &str = "feature:";

/// IT-4: the create-queue item for a bug in `repo` (a fleet-ghq issue item), labelled `bug` and the star's one `feature:` label.
/// `star_labels` are the labels on the star's issue. None when the star has no `feature:` label or has more than one (an orphan star is drift, SKY-10): the caller says so, a bug is not filed without its feature.
pub fn ladder_out_bug(
    repo: &str,
    star_issue: u64,
    title: &str,
    star_labels: &[String],
) -> Option<serde_json::Value> {
    let features: Vec<&str> = star_labels
        .iter()
        .filter_map(|label| label.strip_prefix(FEATURE_PREFIX))
        .filter(|id| !id.is_empty())
        .collect();
    let [feature] = features.as_slice() else {
        return None;
    };
    Some(serde_json::json!({
        "kind": "issue",
        "repo": repo,
        "title": title,
        "body": format!(
            "The job for #{star_issue} ran out of ladder: a coding error (IT-4).\n\n\
             Filed in this repo, not as an IT ticket. Feature: `{feature}`."
        ),
        "labels": ["bug", format!("feature:{feature}")],
        "milestone": null,
    }))
}

/// IT-2: the log's errors as the records `fleet errors file --records` reads: a JSON list of
/// `{command, exit_code, stderr}`, oldest first, each as it was recorded.
fn error_records(log: &FleetErrorLog) -> serde_json::Value {
    serde_json::Value::Array(
        log.errors
            .iter()
            .map(|e| {
                serde_json::json!({
                    "command": e.command,
                    "exit_code": e.exit_code,
                    "stderr": e.stderr,
                })
            })
            .collect(),
    )
}

/// IT-2: hand each recorded fleet error to `fleet errors file`; returns how many it handed over.
/// `fleet errors file --records -` owns the signature (built from each record's command and
/// stderr), the `IT: <signature>` title and the repeat comment, so the core runs it once with
/// the log's records on stdin and passes nothing the log does not hold. An empty log runs
/// nothing. A failing run is its `FleetError`; the log is only read, never changed.
pub fn file_error_tickets(bridge: &FleetBridge, log: &FleetErrorLog) -> Result<usize, FleetError> {
    if log.errors.is_empty() {
        return Ok(0);
    }
    let records = error_records(log).to_string();
    bridge.run_status_with_input(&["errors", "file", "--records", "-"], &records)?;
    Ok(log.errors.len())
}

/// IT-2: the label an IT ticket carries.
pub const IT_LABEL: &str = "it";

/// IT-2 (#375): one IT ticket as the CLI and MCP list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItTicket {
    /// `owner/name`.
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub state: String,
    pub link: String,
}

/// IT-2 (#375): the open issues labelled `it` in the stored snapshots, by repo, newest number
/// first. Tickets are filed in the pipeline repo; reading every snapshot finds them wherever
/// that repo's snapshot is, and only GitHub's state is shown, as the last refresh read it.
pub fn open_tickets(snaps: &[crate::github::RepoSnapshot]) -> Vec<ItTicket> {
    let mut tickets: Vec<ItTicket> = snaps
        .iter()
        .flat_map(|snap| {
            snap.issues
                .iter()
                .filter(|i| i.state == "open" && i.labels.iter().any(|l| l == IT_LABEL))
                .map(|i| ItTicket {
                    repo: snap.repo.clone(),
                    number: i.number,
                    title: i.title.clone(),
                    state: i.state.clone(),
                    link: format!("https://github.com/{}/issues/{}", snap.repo, i.number),
                })
        })
        .collect();
    tickets.sort_by(|a, b| a.repo.cmp(&b.repo).then(b.number.cmp(&a.number)));
    tickets
}

/// IT-3: the `it-triage` job for `ticket`, or None while the pipeline lacks `never_on` (U9).
/// The job carries `never_on: [this_machine]` so `fleet run --next` never gives it back to this
/// machine, at any hour. It never uses `not_on`, which only applies during quiet hours.
pub fn triage_job(
    ticket: u64,
    this_machine: &str,
    pipeline_has_never_on: bool,
) -> Option<serde_json::Value> {
    if !pipeline_has_never_on {
        return None;
    }
    Some(serde_json::json!({
        "kind": "it-triage",
        "ticket": ticket,
        "never_on": [this_machine],
    }))
}
