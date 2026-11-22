//! `run-scoring` (#412): sample finished runs at each scorer's `sample_rate`, record one
//! `ScoreRow` per `(sampled_run, scorer)`, batch failed ratings into one proposed spec diff
//! opened as a PR (never editing a spec on `main` directly), and raise one `WaitingItem` in
//! `RunnerItems` for the owner (`owner-merges-spec-diff`).

use serde::{Deserialize, Serialize};

use super::spec::{Scorer, Spec};
use super::state::RunState;
use super::waiting::{RunnerItems, WaitingItem};
use crate::clock::UnixSeconds;
use crate::CoreError;

/// One rating row written for a sampled finished run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreRow {
    pub unit: String,
    pub spec: String,
    pub scorer: String,
    pub label: String,
    pub score: f64,
    pub passed: bool,
}

/// Grades one run against one scorer.
pub trait Grader {
    fn grade(
        &self,
        spec: &Spec,
        run: &RunState,
        scorer: &Scorer,
    ) -> Result<(String, f64), CoreError>;
}

/// Opens a pull request for a batched spec diff (never editing `main` directly).
pub trait SpecProposer {
    /// Open one PR proposing changes for `failed` ratings; returns the PR number.
    fn open_spec_pr(&self, spec: &Spec, failed: &[ScoreRow]) -> Result<u64, CoreError>;
}

/// Outcome of running `score_sample`.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleReport {
    pub rows: Vec<ScoreRow>,
    pub failed: Vec<ScoreRow>,
    pub pr: Option<u64>,
    pub waiting: Option<WaitingItem>,
}

/// Deterministic unit-hash fraction in `[0.0, 1.0)` for sampling.
pub fn sample_bucket(unit: &str, scorer_id: &str) -> f64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in unit.bytes().chain([0xff]).chain(scorer_id.bytes()) {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    (hash as f64) / ((u64::MAX as f64) + 1.0)
}

/// Whether `run` is selected for `scorer` at `scorer.sample_rate`.
pub fn should_sample(run: &RunState, scorer: &Scorer) -> bool {
    if scorer.sample_rate <= 0.0 {
        return false;
    }
    if scorer.sample_rate >= 1.0 {
        return true;
    }
    sample_bucket(&run.unit, &scorer.id) < scorer.sample_rate
}

/// Grade the sampled subset of finished `runs` against each scorer in `spec`, batch failed
/// ratings into one PR via `proposer`, and raise one `WaitingItem` in `waiting` for the
/// `owner-merges-spec-diff` step.
pub fn score_sample(
    spec: &Spec,
    runs: &[RunState],
    grader: &dyn Grader,
    proposer: &dyn SpecProposer,
    waiting: &RunnerItems,
    now: UnixSeconds,
) -> Result<SampleReport, CoreError> {
    let mut rows = Vec::new();
    let mut failed = Vec::new();

    for run in runs {
        let terminal = spec.states.iter().any(|s| s.id == run.state && s.terminal);
        if !terminal {
            continue;
        }
        for scorer in &spec.scorers {
            if !should_sample(run, scorer) {
                continue;
            }
            let (label, score) = grader.grade(spec, run, scorer)?;
            let passed = score >= scorer.pass_threshold;
            let row = ScoreRow {
                unit: run.unit.clone(),
                spec: spec.id.clone(),
                scorer: scorer.id.clone(),
                label,
                score,
                passed,
            };
            if !passed {
                failed.push(row.clone());
            }
            rows.push(row);
        }
    }

    if failed.is_empty() {
        return Ok(SampleReport {
            rows,
            failed,
            pr: None,
            waiting: None,
        });
    }

    let pr = proposer.open_spec_pr(spec, &failed)?;
    let step = spec
        .steps
        .iter()
        .find(|s| s.id == "owner-merges-spec-diff")
        .ok_or_else(|| {
            CoreError::Parse(format!("spec {}: missing owner-merges-spec-diff", spec.id))
        })?;
    let ask = step.ask.as_ref().ok_or_else(|| {
        CoreError::Parse(format!(
            "spec {}: owner-merges-spec-diff has no ask",
            spec.id
        ))
    })?;
    let item = waiting.raise(spec, step, &format!("pr#{pr}"), ask, now)?;

    Ok(SampleReport {
        rows,
        failed,
        pr: Some(pr),
        waiting: Some(item),
    })
}
