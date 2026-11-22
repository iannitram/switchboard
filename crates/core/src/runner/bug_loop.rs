//! `bug-loop-live` (#411): wire `specs/bug-loop.json` through `Engine` and `OwnerSteps`
//! so a bug report runs from intake (`idle` -> `report-archived`) through fix and release
//! (`open` -> `closed` or `parked`).

use super::engine::{Checker, Engine, Outcome, StepReport, StepRunner};
use super::spec::{Actor, Spec};
use super::state::{RunState, StateStore};
use super::waiting::{OwnerSteps, RunnerItems};
use crate::clock::Clock;
use crate::fleet::FleetBridge;
use crate::CoreError;

/// Shared runner dependencies for driving `specs/bug-loop.json`.
pub struct BugEnv<'a> {
    pub bridge: &'a FleetBridge,
    pub runner: &'a dyn StepRunner,
    pub checks: &'a dyn Checker,
    pub store: &'a StateStore,
    pub waiting: &'a RunnerItems,
    pub clock: &'a dyn Clock,
}

/// Result of driving one bug report or tracked bug through `specs/bug-loop.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BugOutcome {
    /// Reached a terminal state (`closed`, `report-archived`, `wont-fix`, `spec-current`).
    Done { final_state: String, steps: usize },
    /// Waiting on a release tag (`swept`) or another `NotNow` condition.
    WaitingRelease { state: String, steps: usize },
    /// Waiting on an owner item in `RunnerItems` (`parked`, etc.).
    WaitingOwner { state: String, steps: usize },
    /// Engine hit a halt or idle state.
    Stopped { state: String, steps: usize },
}

/// Drive `unit` through `spec` from its current state (or `start_state` when no state is saved
/// yet) until it completes, waits on a release tag (`check-release` -> `NotNow`), waits on an
/// owner step, or stops.
pub fn drive_bug(
    spec: &Spec,
    env: &BugEnv<'_>,
    unit: &str,
    start_state: Option<&str>,
    max_steps: usize,
) -> Result<BugOutcome, CoreError> {
    if let Some(state) = start_state {
        if env.store.load(&spec.id, unit)?.is_none() {
            env.store.save(&RunState::new(&spec.id, unit, state))?;
        }
    }

    let owner_runner = OwnerSteps {
        spec,
        inner: env.runner,
        items: env.waiting,
        now: env.clock.now(),
    };
    let engine = Engine {
        bridge: env.bridge,
        runner: &owner_runner,
        checks: env.checks,
        store: env.store,
    };

    let mut steps_run = 0usize;
    while steps_run < max_steps {
        let current_state = env
            .store
            .load(&spec.id, unit)?
            .map(|r| r.state)
            .unwrap_or_else(|| {
                spec.states
                    .first()
                    .map(|s| s.id.clone())
                    .unwrap_or_default()
            });

        if spec
            .states
            .iter()
            .any(|s| s.id == current_state && s.terminal)
        {
            return Ok(BugOutcome::Done {
                final_state: current_state,
                steps: steps_run,
            });
        }

        // `Engine::start` only runs `fleet.py` for `home == "pipeline"`; other pipeline scripts
        // (such as `merge_pr.py` in `merge-check` and `merge`) go through `env.runner`.
        if let Some(step) = spec.steps.iter().find(|s| s.from_state == current_state) {
            if step.run.home == "pipeline" && !step.run.command.contains("fleet.py") {
                let outcome = env.runner.run(step, unit)?;
                let mut run = env
                    .store
                    .load(&spec.id, unit)?
                    .unwrap_or_else(|| RunState::new(&spec.id, unit, &current_state));
                let unmet = step
                    .done_when
                    .iter()
                    .find(|c| !env.checks.holds(c, &outcome));
                if matches!((&outcome, unmet), (Outcome::Exit { code: 0, .. }, None)) {
                    run.state = step.to_state_ok.clone();
                    env.store.save(&run)?;
                    steps_run += 1;
                    if spec.states.iter().any(|s| s.id == run.state && s.terminal) {
                        return Ok(BugOutcome::Done {
                            final_state: run.state,
                            steps: steps_run,
                        });
                    }
                    continue;
                }
            }
        }

        match engine.step(spec, unit)? {
            StepReport::Done { state, .. } => {
                steps_run += 1;
                if spec.states.iter().any(|s| s.id == state && s.terminal) {
                    return Ok(BugOutcome::Done {
                        final_state: state,
                        steps: steps_run,
                    });
                }
            }
            StepReport::Retry { .. } => {
                steps_run += 1;
            }
            StepReport::NotNow => {
                steps_run += 1;
                let state = env
                    .store
                    .load(&spec.id, unit)?
                    .map(|r| r.state)
                    .unwrap_or(current_state);
                if state == "swept" {
                    return Ok(BugOutcome::WaitingRelease {
                        state,
                        steps: steps_run,
                    });
                }
                let is_human = spec
                    .steps
                    .iter()
                    .any(|s| s.from_state == state && s.actor == Actor::Human);
                if is_human {
                    return Ok(BugOutcome::WaitingOwner {
                        state,
                        steps: steps_run,
                    });
                }
                return Ok(BugOutcome::Stopped {
                    state,
                    steps: steps_run,
                });
            }
            StepReport::Halted { state, .. } => {
                steps_run += 1;
                return Ok(BugOutcome::Stopped {
                    state,
                    steps: steps_run,
                });
            }
            StepReport::Idle => {
                return Ok(BugOutcome::Stopped {
                    state: current_state,
                    steps: steps_run,
                });
            }
        }
    }

    let final_state = env
        .store
        .load(&spec.id, unit)?
        .map(|r| r.state)
        .unwrap_or_default();
    Ok(BugOutcome::Stopped {
        state: final_state,
        steps: steps_run,
    })
}

/// Run a bug report end-to-end: first intake (`idle` -> `report-archived`), then the fix/release
/// loop (`open` -> `closed` or `WaitingRelease` when waiting on a release tag).
pub fn run_end_to_end(
    spec: &Spec,
    env: &BugEnv<'_>,
    report_unit: &str,
    bug_unit: &str,
    max_steps: usize,
) -> Result<(BugOutcome, BugOutcome), CoreError> {
    let intake = drive_bug(spec, env, report_unit, Some("idle"), max_steps)?;
    if !matches!(intake, BugOutcome::Done { ref final_state, .. } if final_state == "report-archived")
    {
        return Ok((
            intake.clone(),
            BugOutcome::Stopped {
                state: "idle".into(),
                steps: 0,
            },
        ));
    }
    let fix = drive_bug(spec, env, bug_unit, Some("open"), max_steps)?;
    Ok((intake, fix))
}
