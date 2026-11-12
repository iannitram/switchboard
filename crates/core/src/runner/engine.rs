//! The step engine: runs one step of a loaded spec for one unit and moves its saved run state.
//!
//! A step whose home is the pipeline and that has a command goes through the fleet bridge, whatever
//! its kind. Every other step goes to the `StepRunner` the caller gives; the engine starts no
//! process of its own. A placeholder such as `<repo>` or `<issue>` in a pipeline command reaches
//! `fleet.py` unchanged; filling in unit values belongs to a later node. After a retryable failure
//! the saved state is already `to_state_fail`, but the pending retry pins the failed step, so any
//! step that leaves `to_state_fail` waits until the retries are used up.

use serde_json::Value;

use super::spec::{Check, Spec, Step};
use super::state::{Halt, Last, RunState, StateStore};
use crate::fleet::FleetBridge;
use crate::CoreError;

/// What a step's command or task produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// It finished with this exit code; `json` is its stdout when that parsed as JSON.
    Exit { code: i32, json: Option<Value> },
    /// Exit 75: not now.
    NotNow,
}

/// Starts the command, model task or question of a step for one unit.
pub trait StepRunner {
    fn run(&self, step: &Step, unit: &str) -> Result<Outcome, CoreError>;
}

/// Decides whether one done_when check holds for an outcome.
pub trait Checker {
    fn holds(&self, check: &Check, outcome: &Outcome) -> bool;
}

/// Understands exactly `exit 0`, `exit 0 with JSON on stdout` and `exit 0 and JSON .path == <json
/// literal>`. Fails closed: a check text the grammar does not consume entirely (prose after
/// `exit 0`, a `:` or `;` tail, another JSON phrasing) is unmet, never guessed to hold.
pub struct BasicChecks;

impl Checker for BasicChecks {
    fn holds(&self, check: &Check, outcome: &Outcome) -> bool {
        let Outcome::Exit { code: 0, json } = outcome else {
            return false;
        };
        let Some(rest) = check.check.trim().strip_prefix("exit 0") else {
            return false;
        };
        if rest.is_empty() {
            return true;
        }
        if rest == " with JSON on stdout" {
            return json.is_some();
        }
        let Some(rest) = rest.strip_prefix(" and JSON .") else {
            return false;
        };
        let Some((path, literal)) = rest.split_once(" == ") else {
            return false;
        };
        let Ok(want) = serde_json::from_str::<Value>(literal) else {
            return false;
        };
        if path.is_empty() || path.contains(char::is_whitespace) {
            return false;
        }
        let Some(mut value) = json.as_ref() else {
            return false;
        };
        for key in path.split('.') {
            match value.get(key) {
                Some(next) => value = next,
                None => return false,
            }
        }
        *value == want
    }
}

/// What one call did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepReport {
    /// No step starts from the current state, or the run is halted.
    Idle,
    /// Exit 75: the state is unchanged.
    NotNow,
    /// Moved to `to_state_ok`.
    Done { step: String, state: String },
    /// A check or the exit failed and the run moved to `to_state_fail`; retries are left.
    Retry {
        step: String,
        state: String,
        attempt: u64,
    },
    /// A failure with no retries left: moved to `to_state_fail` and halted.
    Halted {
        step: String,
        state: String,
        halt: Halt,
    },
}

pub struct Engine<'a> {
    pub bridge: &'a FleetBridge,
    pub runner: &'a dyn StepRunner,
    pub checks: &'a dyn Checker,
    pub store: &'a StateStore,
}

impl Engine<'_> {
    /// Run the next step of `spec` for `unit` and save the run state. A unit that has not started
    /// begins at the spec's first state; one with saved state continues from it.
    pub fn step(&self, spec: &Spec, unit: &str) -> Result<StepReport, CoreError> {
        let mut run = match self.store.load(&spec.id, unit)? {
            Some(run) => run,
            None => {
                let first = spec
                    .states
                    .first()
                    .ok_or_else(|| CoreError::Parse(format!("spec {}: no states", spec.id)))?;
                RunState::new(&spec.id, unit, &first.id)
            }
        };
        if run.halt.is_some() {
            return Ok(StepReport::Idle);
        }
        let step = match &run.retry_step {
            Some(id) => spec.steps.iter().find(|s| &s.id == id),
            None => spec.steps.iter().find(|s| s.from_state == run.state),
        };
        let Some(step) = step else {
            return Ok(StepReport::Idle);
        };
        run.last_step = Some(step.id.clone());
        let outcome = self.start(step, unit)?;
        if outcome == Outcome::NotNow {
            run.last = Some(Last::NotNow);
            run.note = "not now".into();
            self.store.save(&run)?;
            return Ok(StepReport::NotNow);
        }
        let unmet = step
            .done_when
            .iter()
            .find(|check| !self.checks.holds(check, &outcome));
        let report = match (&outcome, unmet) {
            (Outcome::Exit { code: 0, .. }, None) => {
                run.state = step.to_state_ok.clone();
                run.failures.remove(&step.id);
                run.retry_step = None;
                run.last = Some(Last::Done);
                run.note.clear();
                StepReport::Done {
                    step: step.id.clone(),
                    state: run.state.clone(),
                }
            }
            _ => {
                run.note = match (&outcome, unmet) {
                    (Outcome::Exit { code, .. }, _) if *code != 0 => format!("exit {code}"),
                    (_, Some(check)) => format!("check failed: {}", check.check),
                    _ => "failed".into(),
                };
                run.state = step.to_state_fail.clone();
                let attempt = run.failures.entry(step.id.clone()).or_insert(0);
                *attempt += 1;
                let attempt = *attempt;
                if attempt <= step.on_fail.retry {
                    run.retry_step = Some(step.id.clone());
                    run.last = Some(Last::Retry);
                    StepReport::Retry {
                        step: step.id.clone(),
                        state: run.state.clone(),
                        attempt,
                    }
                } else {
                    let halt = match step.on_fail.then.as_str() {
                        "escalate" => Halt::Escalate(step.on_fail.escalate_via.clone()),
                        "park" => Halt::Park,
                        "ask-owner" => Halt::AskOwner,
                        _ => Halt::Fail,
                    };
                    run.retry_step = None;
                    run.halt = Some(halt.clone());
                    run.last = Some(Last::Halted);
                    StepReport::Halted {
                        step: step.id.clone(),
                        state: run.state.clone(),
                        halt,
                    }
                }
            }
        };
        self.store.save(&run)?;
        Ok(report)
    }

    /// Start the step: a step with home `pipeline` and a command, of any kind, through the fleet bridge, anything else through the
    /// runner.
    fn start(&self, step: &Step, unit: &str) -> Result<Outcome, CoreError> {
        if step.run.home != "pipeline" || step.run.command.trim().is_empty() {
            return self.runner.run(step, unit);
        }
        let words: Vec<&str> = step
            .run
            .command
            .split_whitespace()
            .skip_while(|w| !w.ends_with("fleet.py"))
            .skip(1)
            .filter(|w| *w != "--json")
            .collect();
        if words.is_empty() {
            return Err(CoreError::Parse(format!(
                "step {}: a pipeline command must run fleet.py",
                step.id
            )));
        }
        Ok(match self.bridge.run_json(&words) {
            Ok(json) => Outcome::Exit {
                code: 0,
                json: Some(json),
            },
            Err(e) if e.exit_code == Some(75) => Outcome::NotNow,
            Err(e) => match (e.exit_code, &e.reason) {
                (Some(0), _) => Outcome::Exit {
                    code: 0,
                    json: None,
                },
                (Some(code), _) => Outcome::Exit { code, json: None },
                (None, _) => return Err(CoreError::Fleet(e)),
            },
        })
    }
}
