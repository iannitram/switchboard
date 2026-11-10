//! SKY-4, SKY-8: the fleet jobs in flight, read from the fleet ledger's events, so the sky, the
//! constellation and the star card name the same job (#343).

use std::collections::BTreeMap;

use serde_json::Value;

use crate::sky::model::StarJob;

/// The jobs in flight in `events` (the ledger, oldest first): for each `<repo>#<n>` id, the last
/// `start` with no `end` after it from the same machine (an event that names no machine matches
/// any). The repo in an id is `owner/name` or the bare name; a bare name is taken as the repo
/// in `repos` it ends (`owner/name`), when there is one. The job reads
/// `<lane> <model> on <machine>, attempt <n>`, each part only when the event has it.
pub fn in_flight(events: &[Value], repos: &[String]) -> Vec<StarJob> {
    let mut running: BTreeMap<(String, u64), Option<&Value>> = BTreeMap::new();
    for event in events {
        let Some(key) = field(event, "id").and_then(|id| star_of(&id, repos)) else {
            continue;
        };
        match event.get("event").and_then(Value::as_str) {
            Some("start") => {
                running.insert(key, Some(event));
            }
            Some("end") => {
                if let Some(slot) = running.get_mut(&key) {
                    let same_machine = slot.is_some_and(|start| {
                        field(start, "machine").is_none()
                            || field(event, "machine").is_none()
                            || field(start, "machine") == field(event, "machine")
                    });
                    if same_machine {
                        *slot = None;
                    }
                }
            }
            _ => {}
        }
    }
    running
        .into_iter()
        .filter_map(|((repo, number), start)| {
            Some(StarJob {
                repo,
                number,
                job: job_text(start?),
            })
        })
        .collect()
}

/// `<repo>#<n>` as (`owner/name`, n); `None` when it is not one.
fn star_of(id: &str, repos: &[String]) -> Option<(String, u64)> {
    let (repo, n) = id.rsplit_once('#')?;
    let number = n.parse::<u64>().ok()?;
    if repo.is_empty() {
        return None;
    }
    let suffix = format!("/{repo}");
    let full = if repo.contains('/') {
        repo.to_string()
    } else {
        repos
            .iter()
            .find(|r| r.ends_with(suffix.as_str()))
            .cloned()
            .unwrap_or_else(|| repo.to_string())
    };
    Some((full, number))
}

/// A string or number field as text.
fn field(event: &Value, key: &str) -> Option<String> {
    match event.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `<lane> <model> on <machine>, attempt <n>`; `a job` when it names neither lane nor model.
fn job_text(start: &Value) -> String {
    let mut text = [field(start, "lane"), field(start, "model")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        text = "a job".to_string();
    }
    if let Some(machine) = field(start, "machine") {
        text.push_str(&format!(" on {machine}"));
    }
    if let Some(attempt) = field(start, "attempt") {
        text.push_str(&format!(", attempt {attempt}"));
    }
    text
}
