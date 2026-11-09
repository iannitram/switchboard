//! `it`: this machine's loadout (IT-1), fleet errors as IT tickets (IT-2), `it-triage` jobs for
//! another machine (IT-3), and ladder-out coding errors as bugs in the project (IT-4).

use crate::clock::UnixSeconds;
use crate::fleet::{FleetBridge, FleetError, FleetErrorLog};
use crate::platform::Probe;

/// IT-1: what this machine can do right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
const LOADOUT_CLIS: [&str; 4] = ["claude", "codex", "gemini", "gh"];

/// IT-1: the toolchain probes (present when `--version` exits 0).
const LOADOUT_TOOLCHAINS: [&str; 3] = ["cargo", "node", "python3"];

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
