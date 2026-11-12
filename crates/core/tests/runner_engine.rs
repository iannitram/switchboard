use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::PathBuf;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::runner::engine::{BasicChecks, Engine, Outcome, StepReport, StepRunner};
use switchboard_core::runner::spec::{self, Check, RunKind, Spec, Step};
use switchboard_core::runner::state::{Halt, Last, StateStore};
use switchboard_core::CoreError;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn stub() -> FleetBridge {
    FleetBridge::with_root(&root().join("tests/fixtures/fleet-stub"))
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-runner-engine-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Two steps over the real draft spec's types: a -> b -> c, failing back to `f`.
fn spec_with(retry: u64, then: &str, home: &str, command: &str) -> Spec {
    let mut spec = spec::load(&root().join("../../specs/roadmap-run.json")).unwrap();
    let mut step: Step = spec.steps[2].clone();
    for id in ["a", "f", "b", "c"] {
        let mut state = spec.states[0].clone();
        state.id = id.into();
        spec.states.push(state);
    }
    step.id = "one".into();
    step.from_state = "a".into();
    step.to_state_ok = "b".into();
    step.to_state_fail = "f".into();
    step.run.kind = RunKind::Command;
    step.run.home = home.into();
    step.run.command = command.into();
    step.done_when = vec![Check {
        check: "exit 0 and JSON .errors == []".into(),
        exists: false,
    }];
    step.on_fail.retry = retry;
    step.on_fail.then = then.into();
    step.on_fail.escalate_via = "revise".into();
    let mut second = step.clone();
    second.id = "two".into();
    second.from_state = "b".into();
    second.to_state_ok = "c".into();
    spec.states
        .retain(|s| ["a", "b", "c", "f"].contains(&s.id.as_str()));
    spec.steps = vec![step, second];
    spec
}

struct Fake(RefCell<Vec<Outcome>>, RefCell<u32>);

impl Fake {
    fn new(mut outcomes: Vec<Outcome>) -> Self {
        outcomes.reverse();
        Fake(RefCell::new(outcomes), RefCell::new(0))
    }
}

impl StepRunner for Fake {
    fn run(&self, _step: &Step, _unit: &str) -> Result<Outcome, CoreError> {
        *self.1.borrow_mut() += 1;
        Ok(self.0.borrow_mut().pop().unwrap())
    }
}

fn ok(json: Value) -> Outcome {
    Outcome::Exit {
        code: 0,
        json: Some(json),
    }
}

fn engine<'a>(f: &'a Fake, s: &'a StateStore, b: &'a FleetBridge) -> Engine<'a> {
    Engine {
        bridge: b,
        runner: f,
        checks: &BasicChecks,
        store: s,
    }
}

#[test]
fn exit_75_leaves_the_state_and_records_not_now() {
    let spec = spec_with(0, "fail", "switchboard", "x");
    let (store, bridge) = (StateStore::new(&tmp("n75")), stub());
    let fake = Fake::new(vec![Outcome::NotNow]);
    let report = engine(&fake, &store, &bridge).step(&spec, "u").unwrap();
    assert_eq!(report, StepReport::NotNow);
    let run = store.load(&spec.id, "u").unwrap().unwrap();
    assert_eq!((run.state.as_str(), run.note.as_str()), ("a", "not now"));
    assert_eq!(run.last, Some(Last::NotNow));
}

#[test]
fn success_moves_on_and_a_restart_continues_from_the_saved_state() {
    let spec = spec_with(0, "fail", "switchboard", "x");
    let dir = tmp("resume");
    let (store, bridge) = (StateStore::new(&dir), stub());
    let fake = Fake::new(vec![ok(json!({"errors": []})), ok(json!({"errors": []}))]);
    let r = engine(&fake, &store, &bridge).step(&spec, "u").unwrap();
    assert_eq!(
        r,
        StepReport::Done {
            step: "one".into(),
            state: "b".into()
        }
    );
    // A new engine and a new store over the same folder: the restart.
    let store2 = StateStore::new(&dir);
    let r = engine(&fake, &store2, &bridge).step(&spec, "u").unwrap();
    assert_eq!(
        r,
        StepReport::Done {
            step: "two".into(),
            state: "c".into()
        }
    );
    assert_eq!(
        engine(&fake, &store2, &bridge).step(&spec, "u").unwrap(),
        StepReport::Idle
    );
    assert_eq!(*fake.1.borrow(), 2);
}

#[test]
fn a_failed_check_moves_to_fail_retries_then_applies_on_fail() {
    for (then, halt) in [
        ("escalate", Halt::Escalate("revise".into())),
        ("park", Halt::Park),
        ("ask-owner", Halt::AskOwner),
        ("fail", Halt::Fail),
    ] {
        let spec = spec_with(1, then, "switchboard", "x");
        let (store, bridge) = (StateStore::new(&tmp(then)), stub());
        let bad = || ok(json!({"errors": ["x"]}));
        let fake = Fake::new(vec![bad(), bad()]);
        let e = engine(&fake, &store, &bridge);
        assert_eq!(
            e.step(&spec, "u").unwrap(),
            StepReport::Retry {
                step: "one".into(),
                state: "f".into(),
                attempt: 1
            }
        );
        assert_eq!(
            e.step(&spec, "u").unwrap(),
            StepReport::Halted {
                step: "one".into(),
                state: "f".into(),
                halt: halt.clone()
            }
        );
        let run = store.load(&spec.id, "u").unwrap().unwrap();
        assert!(run.note.starts_with("check failed"));
        assert_eq!(e.step(&spec, "u").unwrap(), StepReport::Idle);
        assert_eq!(*fake.1.borrow(), 2);
    }
}

#[test]
fn a_nonzero_exit_fails_the_step() {
    let spec = spec_with(0, "park", "switchboard", "x");
    let (store, bridge) = (StateStore::new(&tmp("exit2")), stub());
    let fake = Fake::new(vec![Outcome::Exit {
        code: 2,
        json: None,
    }]);
    let r = engine(&fake, &store, &bridge).step(&spec, "u").unwrap();
    assert!(matches!(
        r,
        StepReport::Halted {
            halt: Halt::Park,
            ..
        }
    ));
    assert_eq!(store.load(&spec.id, "u").unwrap().unwrap().note, "exit 2");
}

#[test]
fn a_pipeline_step_goes_through_the_fleet_bridge_not_the_runner() {
    let cmd = "python3 -I tools/fleet/fleet.py actions --json";
    let spec = spec_with(0, "fail", "pipeline", cmd);
    let store = StateStore::new(&tmp("bridge"));
    let fake = Fake::new(vec![]);
    let mut spec_ok = spec.clone();
    spec_ok.steps[0].done_when[0].check = "exit 0 with JSON on stdout".into();
    let r = engine(&fake, &store, &stub()).step(&spec_ok, "u").unwrap();
    assert!(matches!(r, StepReport::Done { .. }));
    assert_eq!(*fake.1.borrow(), 0);
    // Exit 75 from the fleet is "not now".
    let busy = stub().with_env("FLEET_STUB_EXIT", "75");
    let r = engine(&fake, &store, &busy).step(&spec, "v").unwrap();
    assert_eq!(r, StepReport::NotNow);
}
