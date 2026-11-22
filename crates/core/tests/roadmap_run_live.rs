//! `roadmap-run-live` (#407): wire `specs/roadmap-run.json` through `Engine` and `OwnerSteps`.

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::json;
use switchboard_core::clock::{Clock, FixedClock};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::Issue;
use switchboard_core::roadmap::graph;
use switchboard_core::runner::engine::{Checker, Outcome, StepRunner};
use switchboard_core::runner::roadmap_run::{drive_unit, run_frontier, RunEnv, UnitOutcome};
use switchboard_core::runner::spec::{self, Check, Step};
use switchboard_core::runner::state::{RunState, StateStore};
use switchboard_core::runner::waiting::RunnerItems;
use switchboard_core::CoreError;

fn tmp(name: &str, fail_build: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-roadmap-run-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tools").join("fleet")).unwrap();
    let py = if fail_build {
        "import sys\nif len(sys.argv) > 2 and sys.argv[1] == 'run' and sys.argv[2] == '<job.json>':\n    sys.exit(1)\nprint('{\"ok\": true}')\n"
    } else {
        "print('{\"ok\": true}')\n"
    };
    std::fs::write(dir.join("tools").join("fleet").join("fleet.py"), py).unwrap();
    dir
}

struct AllPass;
impl Checker for AllPass {
    fn holds(&self, _: &Check, outcome: &Outcome) -> bool {
        matches!(outcome, Outcome::Exit { code: 0, .. })
    }
}

struct StubRunner {
    fail_unit: Option<String>,
    events: Mutex<Vec<String>>,
}

impl StepRunner for StubRunner {
    fn run(&self, step: &Step, unit: &str) -> Result<Outcome, CoreError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("{unit}:{}", step.id));
        if step.id == "run-build"
            && self
                .fail_unit
                .as_deref()
                .is_some_and(|u| unit.starts_with(u))
        {
            return Ok(Outcome::Exit {
                code: 1,
                json: Some(json!({"outcome": "failed"})),
            });
        }
        let payload = match step.id.as_str() {
            "route-by-type" => Some(json!({"type": "build"})),
            "check-roadmap-complete" => Some(json!({"complete": true})),
            _ => Some(json!({"ok": true})),
        };
        Ok(Outcome::Exit {
            code: 0,
            json: payload,
        })
    }
}

fn issue(number: u64, node_id: &str) -> Issue {
    Issue {
        number,
        title: node_id.into(),
        state: "open".into(),
        labels: vec![format!("node:{node_id}"), "size:easy".into()],
        milestone: None,
        body: String::new(),
        closed_by_pr: None,
    }
}

#[test]
fn three_dependent_units_build_in_dependency_order_with_no_manual_step() {
    let dir = tmp("chain", false);
    let spec_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../specs/roadmap-run.json");
    let spec = spec::load(&spec_path).unwrap();
    let roadmap_md = "\
features:
- core: Core (release: 1.0)

nodes:
- u1: First (feature: core, type: build, size: easy, after: none)
- u2: Second (feature: core, type: build, size: easy, after: u1)
- u3: Third (feature: core, type: build, size: easy, after: u2)
";
    let graph = graph::parse(roadmap_md).unwrap();
    let mut issues = vec![issue(3, "u3"), issue(1, "u1"), issue(2, "u2")];
    let links = vec![(2, 1), (3, 2)];

    let bridge = FleetBridge::new(&dir);
    let runner = StubRunner {
        fail_unit: None,
        events: Mutex::new(Vec::new()),
    };
    let store = StateStore::new(&dir);
    let waiting = RunnerItems::new(&dir);
    let clock = FixedClock::at(1_000);

    let env = RunEnv {
        bridge: &bridge,
        runner: &runner,
        checks: &AllPass,
        store: &store,
        waiting: &waiting,
        clock: &clock,
    };
    let run = run_frontier(&spec, &graph, &mut issues, &links, &env, "in-sync", 50).unwrap();

    assert_eq!(run.order, vec!["u1", "u2", "u3"]);
    assert!(run
        .outcomes
        .iter()
        .all(|(_, o)| matches!(o, UnitOutcome::Done { final_state, .. } if final_state == "done")));
    assert!(waiting.list().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn three_failed_builds_park_the_unit_and_raise_one_waiting_item() {
    let dir = tmp("park", true);
    let spec_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../specs/roadmap-run.json");
    let spec = spec::load(&spec_path).unwrap();

    let bridge = FleetBridge::new(&dir);
    let runner = StubRunner {
        fail_unit: Some("u1".into()),
        events: Mutex::new(Vec::new()),
    };
    let store = StateStore::new(&dir);
    let waiting = RunnerItems::new(&dir);
    let clock = FixedClock::at(2_000);

    store
        .save(&RunState::new(&spec.id, "u1#1", "in-sync"))
        .unwrap();

    let env = RunEnv {
        bridge: &bridge,
        runner: &runner,
        checks: &AllPass,
        store: &store,
        waiting: &waiting,
        clock: &clock,
    };
    let outcome = drive_unit(&spec, &env, "u1#1", 50).unwrap();

    assert!(matches!(outcome, UnitOutcome::Parked { state, .. } if state == "parked"));
    let items = waiting.list().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].step, "owner-rules-on-parked");
    assert_eq!(items[0].unit, "u1#1");

    // Answering with the recommended choice resumes to `needs-revision`.
    waiting
        .answer(
            &spec,
            &store,
            &items[0].id,
            &items[0].recommended,
            clock.now(),
        )
        .unwrap();
    let resumed = store.load(&spec.id, "u1#1").unwrap().unwrap();
    assert_eq!(resumed.state, "needs-revision");
    let _ = std::fs::remove_dir_all(&dir);
}
