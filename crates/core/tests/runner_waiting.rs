use std::cell::RefCell;
use std::path::PathBuf;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::runner::engine::{BasicChecks, Engine, Outcome, StepReport, StepRunner};
use switchboard_core::runner::spec::{self, Actor, Spec, Step};
use switchboard_core::runner::state::{Last, RunState, StateStore};
use switchboard_core::runner::waiting::{OwnerSteps, RunnerItems};
use switchboard_core::CoreError;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn stub() -> FleetBridge {
    FleetBridge::with_root(&root().join("tests/fixtures/fleet-stub"))
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-runner-waiting-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn draft() -> Spec {
    spec::load(&root().join("../../specs/roadmap-run.json")).unwrap()
}

/// Stands in for the code, model and question steps: counts its calls.
struct Counting(RefCell<u32>);

impl StepRunner for Counting {
    fn run(&self, _step: &Step, _unit: &str) -> Result<Outcome, CoreError> {
        *self.0.borrow_mut() += 1;
        Ok(Outcome::NotNow)
    }
}

/// Run the engine once for `unit` with owner steps raised at `now`.
fn step_at(
    spec: &Spec,
    store: &StateStore,
    items: &RunnerItems,
    counting: &Counting,
    unit: &str,
    now: i64,
) -> Result<StepReport, CoreError> {
    let bridge = stub();
    let owner = OwnerSteps {
        spec,
        items,
        now,
        inner: counting,
    };
    let engine = Engine {
        bridge: &bridge,
        runner: &owner,
        checks: &BasicChecks,
        store,
    };
    engine.step(spec, unit)
}

fn park_at(store: &StateStore, spec: &Spec, unit: &str, state: &str) {
    store.save(&RunState::new(&spec.id, unit, state)).unwrap();
}

fn state_of(store: &StateStore, spec: &Spec, unit: &str) -> RunState {
    store.load(&spec.id, unit).unwrap().unwrap()
}

#[test]
fn a_human_step_raises_one_open_item_and_running_it_again_raises_no_second() {
    let spec = draft();
    let dir = tmp("once");
    let (store, items) = (StateStore::new(&dir), RunnerItems::new(&dir));
    let counting = Counting(RefCell::new(0));
    assert_eq!(
        step_at(&spec, &store, &items, &counting, "owner/demo", 100).unwrap(),
        StepReport::NotNow
    );
    assert_eq!(
        step_at(&spec, &store, &items, &counting, "owner/demo", 200).unwrap(),
        StepReport::NotNow
    );
    let open = items.list().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].step, "agree-spec");
    assert_eq!(open[0].unit, "owner/demo");
    assert_eq!(open[0].opened, 100);
    assert_eq!(
        *counting.0.borrow(),
        0,
        "a human step never reaches the code"
    );
    assert_eq!(state_of(&store, &spec, "owner/demo").state, "idea");
}

#[test]
fn the_item_shows_needs_why_and_the_recommended_choice_first() {
    let mut spec = draft();
    let step = spec
        .steps
        .iter_mut()
        .find(|s| s.id == "agree-spec")
        .unwrap();
    let ask = step.ask.as_mut().unwrap();
    ask.choices = vec!["Needs changes".into(), "Approve".into()];
    ask.recommended = "Approve".into();
    let (needs, why) = (ask.needs.clone(), ask.why.clone());
    let dir = tmp("shows");
    let (store, items) = (StateStore::new(&dir), RunnerItems::new(&dir));
    let counting = Counting(RefCell::new(0));
    step_at(&spec, &store, &items, &counting, "owner/demo", 100).unwrap();
    let open = items.list().unwrap();
    assert_eq!(open[0].needs, needs);
    assert_eq!(open[0].why, why);
    assert_eq!(open[0].choices, vec!["Approve", "Needs changes"]);
    assert_eq!(open[0].recommended, "Approve");
}

#[test]
fn the_recommended_answer_moves_the_unit_to_to_state_ok() {
    let spec = draft();
    let dir = tmp("ok");
    let (store, items) = (StateStore::new(&dir), RunnerItems::new(&dir));
    let counting = Counting(RefCell::new(0));
    step_at(&spec, &store, &items, &counting, "owner/demo", 100).unwrap();
    let id = items.list().unwrap()[0].id.clone();
    let answered = items.answer(&spec, &store, &id, "Approve", 150).unwrap();
    assert_eq!(answered.answer.as_deref(), Some("Approve"));
    assert_eq!(answered.answered, Some(150));
    let run = state_of(&store, &spec, "owner/demo");
    assert_eq!(run.state, "spec-agreed");
    assert_eq!(run.last, Some(Last::Done));
    assert!(
        items.list().unwrap().is_empty(),
        "an answered item is no longer open"
    );
}

#[test]
fn another_choice_moves_the_unit_to_to_state_fail() {
    let spec = draft();
    let dir = tmp("fail");
    let (store, items) = (StateStore::new(&dir), RunnerItems::new(&dir));
    let counting = Counting(RefCell::new(0));
    park_at(&store, &spec, "owner/demo", "parked");
    step_at(&spec, &store, &items, &counting, "owner/demo", 100).unwrap();
    let open = items.list().unwrap();
    assert_eq!(open[0].step, "owner-rules-on-parked");
    assert_eq!(open[0].choices, vec!["Change the plan", "Drop it"]);
    let id = open[0].id.clone();
    items.answer(&spec, &store, &id, "Drop it", 150).unwrap();
    assert_eq!(state_of(&store, &spec, "owner/demo").state, "out-of-scope");
}

#[test]
fn an_answer_that_is_not_a_choice_is_refused_and_changes_nothing() {
    let spec = draft();
    let dir = tmp("refused");
    let (store, items) = (StateStore::new(&dir), RunnerItems::new(&dir));
    let counting = Counting(RefCell::new(0));
    step_at(&spec, &store, &items, &counting, "owner/demo", 100).unwrap();
    let id = items.list().unwrap()[0].id.clone();
    assert!(items.answer(&spec, &store, &id, "Maybe", 150).is_err());
    assert_eq!(items.list().unwrap().len(), 1);
    assert_eq!(state_of(&store, &spec, "owner/demo").state, "idea");
}

#[test]
fn after_a_rejected_answer_the_step_raises_a_new_item() {
    let spec = draft();
    let dir = tmp("again");
    let (store, items) = (StateStore::new(&dir), RunnerItems::new(&dir));
    let counting = Counting(RefCell::new(0));
    step_at(&spec, &store, &items, &counting, "owner/demo", 100).unwrap();
    let id = items.list().unwrap()[0].id.clone();
    items
        .answer(&spec, &store, &id, "Needs changes", 150)
        .unwrap();
    assert_eq!(state_of(&store, &spec, "owner/demo").state, "idea");
    step_at(&spec, &store, &items, &counting, "owner/demo", 200).unwrap();
    let open = items.list().unwrap();
    assert_eq!(open.len(), 1);
    assert_ne!(open[0].id, id);
    assert_eq!(open[0].opened, 200);
}

#[test]
fn the_open_items_are_listed_oldest_first() {
    let spec = draft();
    let dir = tmp("order");
    let (store, items) = (StateStore::new(&dir), RunnerItems::new(&dir));
    let counting = Counting(RefCell::new(0));
    step_at(&spec, &store, &items, &counting, "owner/newer", 300).unwrap();
    step_at(&spec, &store, &items, &counting, "owner/older", 100).unwrap();
    let units: Vec<String> = items.list().unwrap().into_iter().map(|i| i.unit).collect();
    assert_eq!(units, vec!["owner/older", "owner/newer"]);
}

#[test]
fn a_step_that_is_not_human_goes_to_the_code_and_raises_nothing() {
    let mut spec = draft();
    spec.steps
        .iter_mut()
        .find(|s| s.id == "agree-spec")
        .unwrap()
        .actor = Actor::Code;
    let dir = tmp("code");
    let (store, items) = (StateStore::new(&dir), RunnerItems::new(&dir));
    let counting = Counting(RefCell::new(0));
    step_at(&spec, &store, &items, &counting, "owner/demo", 100).unwrap();
    assert_eq!(*counting.0.borrow(), 1);
    assert!(items.list().unwrap().is_empty());
}

#[test]
fn a_human_step_without_an_ask_is_an_error_and_raises_nothing() {
    let mut spec = draft();
    spec.steps
        .iter_mut()
        .find(|s| s.id == "agree-spec")
        .unwrap()
        .ask = None;
    let dir = tmp("noask");
    let (store, items) = (StateStore::new(&dir), RunnerItems::new(&dir));
    let counting = Counting(RefCell::new(0));
    assert!(step_at(&spec, &store, &items, &counting, "owner/demo", 100).is_err());
    assert!(items.list().unwrap().is_empty());
}
