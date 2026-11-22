//! `roadmap-run-live` (#407): wire `specs/roadmap-run.json` through `Engine` and `OwnerSteps`
//! so one project runs from an approved roadmap to merged units, with every owner step in
//! Waiting on you (`RunnerItems`).

use super::engine::{Checker, Engine, StepReport, StepRunner};
use super::spec::{Actor, Spec};
use super::state::{Halt, StateStore};
use super::waiting::{OwnerSteps, RunnerItems};
use crate::clock::Clock;
use crate::fleet::FleetBridge;
use crate::github::Issue;
use crate::roadmap::graph::Graph;
use crate::unit::frontier::{self, Inputs};
use crate::CoreError;

/// Maximum build tries before a unit is parked for owner ruling.
pub const MAX_BUILD_ATTEMPTS: usize = 3;

/// Result of driving one unit through `specs/roadmap-run.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitOutcome {
    /// Unit reached `done` (or another terminal state).
    Done { final_state: String, steps: usize },
    /// Unit is waiting on an owner item in `RunnerItems`.
    Waiting { state: String, steps: usize },
    /// Unit was parked after repeated build failures and raised an owner waiting item.
    Parked { state: String, steps: usize },
    /// Engine hit a non-parked halt or idle state.
    Stopped { state: String, steps: usize },
}

/// Shared runner dependencies for driving `specs/roadmap-run.json`.
pub struct RunEnv<'a> {
    pub bridge: &'a FleetBridge,
    pub runner: &'a dyn StepRunner,
    pub checks: &'a dyn Checker,
    pub store: &'a StateStore,
    pub waiting: &'a RunnerItems,
    pub clock: &'a dyn Clock,
}

/// Drive a single `unit` through `spec` using `Engine` + `OwnerSteps` until it completes,
/// waits on an owner item, parks after `MAX_BUILD_ATTEMPTS` build failures, or stops.
pub fn drive_unit(
    spec: &Spec,
    env: &RunEnv<'_>,
    unit: &str,
    max_steps: usize,
) -> Result<UnitOutcome, CoreError> {
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
    let mut build_failures = 0usize;

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

        // Stop if the unit is already in a terminal state or waiting on `parked` after raising.
        if spec
            .states
            .iter()
            .any(|s| s.id == current_state && s.terminal)
        {
            return Ok(UnitOutcome::Done {
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
                    .unwrap_or_else(|| super::state::RunState::new(&spec.id, unit, &current_state));
                let unmet = step
                    .done_when
                    .iter()
                    .find(|c| !env.checks.holds(c, &outcome));
                if matches!(
                    (&outcome, unmet),
                    (super::engine::Outcome::Exit { code: 0, .. }, None)
                ) {
                    run.state = step.to_state_ok.clone();
                    env.store.save(&run)?;
                    steps_run += 1;
                    if spec.states.iter().any(|s| s.id == run.state && s.terminal) {
                        return Ok(UnitOutcome::Done {
                            final_state: run.state,
                            steps: steps_run,
                        });
                    }
                    continue;
                }
            }
        }

        let report = engine.step(spec, unit)?;
        steps_run += 1;

        match report {
            StepReport::Done { step, state } => {
                if spec.states.iter().any(|s| s.id == state && s.terminal) {
                    return Ok(UnitOutcome::Done {
                        final_state: state,
                        steps: steps_run,
                    });
                }
                if step == "run-build" && state == "built" {
                    build_failures = 0;
                }
            }
            StepReport::Retry { step, state, .. } => {
                if step == "run-build" || state == "build-failed" {
                    build_failures += 1;
                    if build_failures >= MAX_BUILD_ATTEMPTS {
                        return park_unit(spec, &engine, env.store, unit, steps_run);
                    }
                }
            }
            StepReport::Halted { state, halt, .. } => {
                if state == "build-failed" && matches!(halt, Halt::Escalate(_)) {
                    build_failures += 1;
                    if build_failures >= MAX_BUILD_ATTEMPTS {
                        return park_unit(spec, &engine, env.store, unit, steps_run);
                    }
                    if let Some(mut run) = env.store.load(&spec.id, unit)? {
                        run.halt = None;
                        env.store.save(&run)?;
                    }
                    continue;
                }
                if state == "parked" {
                    let _ = engine.step(spec, unit)?;
                    return Ok(UnitOutcome::Parked {
                        state: "parked".into(),
                        steps: steps_run,
                    });
                }
                return Ok(UnitOutcome::Stopped {
                    state,
                    steps: steps_run,
                });
            }
            StepReport::NotNow => {
                let state = env
                    .store
                    .load(&spec.id, unit)?
                    .map(|r| r.state)
                    .unwrap_or(current_state);
                let is_human = spec
                    .steps
                    .iter()
                    .any(|s| s.from_state == state && s.actor == Actor::Human);
                if state == "parked" {
                    return Ok(UnitOutcome::Parked {
                        state,
                        steps: steps_run,
                    });
                }
                if is_human {
                    return Ok(UnitOutcome::Waiting {
                        state,
                        steps: steps_run,
                    });
                }
                return Ok(UnitOutcome::Stopped {
                    state,
                    steps: steps_run,
                });
            }
            StepReport::Idle => {
                return Ok(UnitOutcome::Stopped {
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
    Ok(UnitOutcome::Stopped {
        state: final_state,
        steps: steps_run,
    })
}

fn park_unit(
    spec: &Spec,
    engine: &Engine<'_>,
    store: &StateStore,
    unit: &str,
    steps_run: usize,
) -> Result<UnitOutcome, CoreError> {
    if let Some(mut run) = store.load(&spec.id, unit)? {
        run.state = "parked".into();
        run.retry_step = None;
        run.halt = None;
        store.save(&run)?;
    }
    // Trigger the `owner-rules-on-parked` human step so it raises one WaitingItem.
    let _ = engine.step(spec, unit)?;
    Ok(UnitOutcome::Parked {
        state: "parked".into(),
        steps: steps_run,
    })
}

/// Summary of running a project's frontier units in dependency order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRun {
    /// Units started and completed in the exact order they were chosen.
    pub order: Vec<String>,
    /// Final outcome per unit id.
    pub outcomes: Vec<(String, UnitOutcome)>,
}

/// Run ready units of `graph` in dependency order using `unit::frontier::next`. Each unit that
/// finishes (`UnitOutcome::Done`) is marked closed and resolved so downstream units become ready.
pub fn run_frontier(
    spec: &Spec,
    graph: &Graph,
    issues: &mut [Issue],
    links: &[(u64, u64)],
    env: &RunEnv<'_>,
    start_state: &str,
    max_steps_per_unit: usize,
) -> Result<ProjectRun, CoreError> {
    let mut resolved: Vec<u64> = issues
        .iter()
        .filter(|i| i.state == "closed")
        .map(|i| i.number)
        .collect();
    let mut claimed: Vec<u64> = Vec::new();
    let mut order = Vec::new();
    let mut outcomes = Vec::new();

    loop {
        let next = frontier::next(&Inputs {
            graph,
            issues,
            links,
            resolved: &resolved,
            claimed: &claimed,
        });
        let Some(unit) = next else {
            break;
        };

        claimed.push(unit.number);
        let unit_key = format!("{}#{}", unit.node_id, unit.number);
        if env.store.load(&spec.id, &unit_key)?.is_none() {
            let initial = super::state::RunState::new(&spec.id, &unit_key, start_state);
            env.store.save(&initial)?;
        }

        let outcome = drive_unit(spec, env, &unit_key, max_steps_per_unit)?;
        order.push(unit.node_id.clone());

        let done = matches!(outcome, UnitOutcome::Done { .. });
        outcomes.push((unit.node_id.clone(), outcome));
        if done {
            if let Some(issue) = issues.iter_mut().find(|i| i.number == unit.number) {
                issue.state = "closed".into();
            }
            resolved.push(unit.number);
        }
    }

    Ok(ProjectRun { order, outcomes })
}
