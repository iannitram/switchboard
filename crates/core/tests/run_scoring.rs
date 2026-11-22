//! `run-scoring` (#412): sample finished runs, write rating rows, batch failed ratings into
//! one proposed spec PR, and raise one Waiting on you item.

use std::path::PathBuf;
use std::sync::Mutex;

use switchboard_core::runner::score::{score_sample, Grader, ScoreRow, SpecProposer};
use switchboard_core::runner::spec::{self, Scorer, Spec};
use switchboard_core::runner::state::RunState;
use switchboard_core::runner::waiting::RunnerItems;
use switchboard_core::CoreError;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-run-scoring-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct StubGrader {
    fail_unit_prefix: String,
}

impl Grader for StubGrader {
    fn grade(
        &self,
        _spec: &Spec,
        run: &RunState,
        scorer: &Scorer,
    ) -> Result<(String, f64), CoreError> {
        if run.unit.starts_with(&self.fail_unit_prefix) {
            Ok(("symptom-masked-not-fixed".into(), 0.5))
        } else {
            Ok((
                scorer
                    .labels
                    .first()
                    .map(|l| l.label.clone())
                    .unwrap_or_else(|| "ok".into()),
                1.0,
            ))
        }
    }
}

struct StubProposer {
    prs: Mutex<Vec<(String, usize)>>,
}

impl SpecProposer for StubProposer {
    fn open_spec_pr(&self, spec: &Spec, failed: &[ScoreRow]) -> Result<u64, CoreError> {
        let mut prs = self.prs.lock().unwrap();
        prs.push((spec.id.clone(), failed.len()));
        Ok(prs.len() as u64 + 700)
    }
}

#[test]
fn sample_rate_grades_about_one_in_four_and_writes_one_row_per_scorer() {
    let dir = tmp("rate");
    let spec_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../specs/roadmap-run.json");
    let spec = spec::load(&spec_path).unwrap();
    let scorer_count = spec.scorers.len();

    let runs: Vec<RunState> = (0..100)
        .map(|i| RunState::new(&spec.id, &format!("unit-{i}"), "done"))
        .collect();
    let grader = StubGrader {
        fail_unit_prefix: "none".into(),
    };
    let proposer = StubProposer {
        prs: Mutex::new(Vec::new()),
    };
    let waiting = RunnerItems::new(&dir);

    let report = score_sample(&spec, &runs, &grader, &proposer, &waiting, 1_000).unwrap();
    assert!(!report.rows.is_empty());
    // With 100 runs * N scorers at sample_rate 0.25, count should be roughly 25 * N.
    let expected = 25 * scorer_count;
    assert!(
        (report.rows.len() as isize - expected as isize).abs() <= (15 * scorer_count as isize),
        "got {} rows, expected ~{}",
        report.rows.len(),
        expected
    );
    assert!(report.pr.is_none());
    assert!(waiting.list().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn failed_ratings_batch_into_one_pr_and_one_waiting_item() {
    let dir = tmp("batch");
    let spec_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../specs/roadmap-run.json");
    let mut spec = spec::load(&spec_path).unwrap();
    // Set sample_rate to 1.0 for deterministic failure batching in this test.
    for s in &mut spec.scorers {
        s.sample_rate = 1.0;
    }

    let runs = vec![
        RunState::new(&spec.id, "bad-1", "done"),
        RunState::new(&spec.id, "bad-2", "done"),
        RunState::new(&spec.id, "good-1", "done"),
        RunState::new(&spec.id, "in-progress-1", "built"), // non-terminal: skipped
    ];
    let grader = StubGrader {
        fail_unit_prefix: "bad-".into(),
    };
    let proposer = StubProposer {
        prs: Mutex::new(Vec::new()),
    };
    let waiting = RunnerItems::new(&dir);

    let report = score_sample(&spec, &runs, &grader, &proposer, &waiting, 2_000).unwrap();
    assert_eq!(report.rows.len(), 3 * spec.scorers.len());
    assert_eq!(report.failed.len(), 2 * spec.scorers.len());
    assert_eq!(report.pr, Some(701));
    assert_eq!(proposer.prs.lock().unwrap().len(), 1);

    let items = waiting.list().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].step, "owner-merges-spec-diff");
    assert_eq!(items[0].unit, "pr#701");
    let _ = std::fs::remove_dir_all(&dir);
}
