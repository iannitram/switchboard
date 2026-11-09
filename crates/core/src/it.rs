//! `it`: this machine's loadout (IT-1), fleet errors as IT tickets (IT-2), `it-triage` jobs for
//! another machine (IT-3), and ladder-out coding errors as bugs in the project (IT-4).

use crate::clock::UnixSeconds;
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
