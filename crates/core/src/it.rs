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
pub fn read_loadout(probes: &[Probe], runner: &dyn ProbeRunner, now: UnixSeconds) -> Loadout {
    todo!("IT-1")
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
