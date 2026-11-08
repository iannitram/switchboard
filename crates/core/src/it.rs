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

/// IT-1: read the loadout from the platform's probe list. A probe that did not run (no exit
/// code) or exited non-zero is an answer ("absent", "not signed in", "no models"), never an
/// error. `fleet errors` prints oldest first, so its newest five lines are the last five.
pub fn read_loadout(probes: &[Probe], runner: &dyn ProbeRunner, now: UnixSeconds) -> Loadout {
    let mut loadout = Loadout {
        read_at: now,
        ..Loadout::default()
    };
    for probe in probes {
        let result = runner.run(probe);
        let ok = result.exit_code == Some(0);
        match probe.name {
            // A CLI that ran at all is present; its auth probe decides signed in.
            "claude" | "codex" | "gemini" | "gh" => {
                if result.exit_code.is_some() {
                    loadout.clis.push((probe.name.to_string(), ok));
                }
            }
            "llama-swap" if ok => loadout.models_loaded = loaded_models(&result.stdout),
            "cargo" | "node" | "python3" if ok => loadout.toolchains.push(probe.name.to_string()),
            "fleet status" => loadout.busy = ok && result.stdout.contains("running"),
            "fleet errors" if ok => loadout.recent_errors = newest_lines(&result.stdout, 5),
            _ => {}
        }
    }
    loadout
}

/// The model names in llama-swap's running answer: the `model` of each entry in its JSON
/// `running` list, or else one name per line.
fn loaded_models(stdout: &str) -> Vec<String> {
    let json: Option<serde_json::Value> = serde_json::from_str(stdout).ok();
    match json
        .as_ref()
        .and_then(|value| value.get("running"))
        .and_then(|list| list.as_array())
    {
        Some(running) => running
            .iter()
            .filter_map(|entry| entry.get("model")?.as_str())
            .map(str::to_string)
            .collect(),
        None => non_empty_lines(stdout),
    }
}

/// The last `count` non-empty lines of `text`.
fn newest_lines(text: &str, count: usize) -> Vec<String> {
    let lines = non_empty_lines(text);
    let skip = lines.len().saturating_sub(count);
    lines.into_iter().skip(skip).collect()
}

fn non_empty_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// FD-12: the `needs` tags the loadout can't meet (empty when it meets them all).
pub fn unmet(needs: &[String], loadout: &Loadout) -> Vec<String> {
    todo!("FD-12")
}

/// IT-2: one ticket per error signature; the repeat comments on it (`fleet errors file`).
pub fn ticket_title(signature: &str) -> String {
    format!("IT: {signature}")
}
