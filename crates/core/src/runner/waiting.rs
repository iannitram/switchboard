//! Owner steps: a step with actor `human` raises one waiting item in Waiting on you and waits. The
//! item carries the step's `ask` (needs, why, choices, recommended), the recommended choice first.
//! Running the step again while its item is open raises nothing more. An answer moves the unit to
//! the state it maps to: the recommended choice to the step's `to_state_ok`, any other choice to
//! `to_state_fail`. Items are kept in `<data>/waiting/runner-items.json`, one change at a time under
//! that file's lock; an answered item stays there as the record of the answer.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::engine::{Outcome, StepRunner};
use super::spec::{Actor, Ask, Spec, Step};
use super::state::{Last, StateStore};
use crate::clock::UnixSeconds;
use crate::lock::FileLock;
use crate::waiting::LOCAL_LOCK_WAIT;
use crate::CoreError;

/// What the lock on `runner-items.json` says holds it.
const HOLDER: &str = "a change to a runner waiting item";

/// One question to the owner, from a human step of a unit's run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitingItem {
    pub id: String,
    pub spec: String,
    pub step: String,
    pub unit: String,
    pub needs: String,
    pub why: String,
    /// The recommended choice first, then the rest in the spec's order.
    pub choices: Vec<String>,
    pub recommended: String,
    pub opened: UnixSeconds,
    pub answer: Option<String>,
    pub answered: Option<UnixSeconds>,
}

impl WaitingItem {
    pub fn is_open(&self) -> bool {
        self.answer.is_none()
    }
}

/// The runner's waiting items, one JSON file under the data folder.
pub struct RunnerItems {
    path: PathBuf,
}

impl RunnerItems {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("waiting").join("runner-items.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// No file is no items; a file that does not parse is a `CoreError::Parse` naming it.
    pub fn load(&self) -> Result<Vec<WaitingItem>, CoreError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(CoreError::Io(format!("{}: {e}", self.path.display()))),
        };
        serde_json::from_str(&text)
            .map_err(|e| CoreError::Parse(format!("{}: {e}", self.path.display())))
    }

    /// The open items, oldest first (`switchboard waiting list`).
    pub fn list(&self) -> Result<Vec<WaitingItem>, CoreError> {
        let mut open: Vec<WaitingItem> = self
            .load()?
            .into_iter()
            .filter(WaitingItem::is_open)
            .collect();
        open.sort_by_key(|item| item.opened);
        Ok(open)
    }

    /// The open item of this step and unit, or a new one when there is none. Under the lock, so
    /// two runs at once still raise one item.
    pub fn raise(
        &self,
        spec: &Spec,
        step: &Step,
        unit: &str,
        ask: &Ask,
        now: UnixSeconds,
    ) -> Result<WaitingItem, CoreError> {
        if !ask.choices.contains(&ask.recommended) {
            return Err(CoreError::Parse(format!(
                "step {}: recommended `{}` is not one of its choices",
                step.id, ask.recommended
            )));
        }
        let mut choices = vec![ask.recommended.clone()];
        choices.extend(
            ask.choices
                .iter()
                .filter(|c| **c != ask.recommended)
                .cloned(),
        );
        self.change(|items| {
            if let Some(open) = items
                .iter()
                .find(|i| i.is_open() && i.spec == spec.id && i.step == step.id && i.unit == unit)
            {
                return Ok((open.clone(), false));
            }
            let item = WaitingItem {
                id: format!("wi-{}", items.len() + 1),
                spec: spec.id.clone(),
                step: step.id.clone(),
                unit: unit.to_string(),
                needs: ask.needs.clone(),
                why: ask.why.clone(),
                choices: choices.clone(),
                recommended: ask.recommended.clone(),
                opened: now,
                answer: None,
                answered: None,
            };
            items.push(item.clone());
            Ok((item, true))
        })
    }

    /// Record the owner's answer to an open item and move its unit to the state the answer maps
    /// to. Refused, with nothing changed, for an answer that is not a choice, an item already
    /// answered, or a unit that is no longer at the item's step.
    pub fn answer(
        &self,
        spec: &Spec,
        store: &StateStore,
        id: &str,
        answer: &str,
        now: UnixSeconds,
    ) -> Result<WaitingItem, CoreError> {
        self.change(|items| {
            let item = items
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or_else(|| CoreError::Parse(format!("no waiting item `{id}`")))?;
            if !item.is_open() {
                return Err(CoreError::Parse(format!(
                    "waiting item `{id}` is answered already"
                )));
            }
            if item.spec != spec.id {
                return Err(CoreError::Parse(format!(
                    "waiting item `{id}` belongs to spec {}, not {}",
                    item.spec, spec.id
                )));
            }
            if !item.choices.iter().any(|c| c == answer) {
                return Err(CoreError::Parse(format!(
                    "`{answer}` is not a choice of waiting item `{id}`: {}",
                    item.choices.join(", ")
                )));
            }
            let step = spec
                .steps
                .iter()
                .find(|s| s.id == item.step)
                .ok_or_else(|| {
                    CoreError::Parse(format!("spec {}: no step {}", spec.id, item.step))
                })?;
            let mut run = store
                .load(&spec.id, &item.unit)?
                .ok_or_else(|| CoreError::Parse(format!("no run state for {}", item.unit)))?;
            if run.state != step.from_state {
                return Err(CoreError::Parse(format!(
                    "{} is at {}, not at {}: the answer is out of date",
                    item.unit, run.state, step.from_state
                )));
            }
            let ok = answer == item.recommended;
            run.state = if ok {
                step.to_state_ok.clone()
            } else {
                step.to_state_fail.clone()
            };
            run.retry_step = None;
            run.last_step = Some(step.id.clone());
            run.last = ok.then_some(Last::Done);
            run.note = format!("answered: {answer}");
            store.save(&run)?;
            item.answer = Some(answer.to_string());
            item.answered = Some(now);
            Ok((item.clone(), true))
        })
    }

    /// Change the items under the file's lock: read them, let `change` change them, and write them
    /// back when it says it changed something. The lock is held through `change`, so a state
    /// written there and the item that records it are one step to every other writer.
    fn change<T>(
        &self,
        change: impl FnOnce(&mut Vec<WaitingItem>) -> Result<(T, bool), CoreError>,
    ) -> Result<T, CoreError> {
        let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", self.path.display()));
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let _held = FileLock::take_for(&self.path, LOCAL_LOCK_WAIT, HOLDER)?;
        let mut items = self.load()?;
        let (out, changed) = change(&mut items)?;
        if changed {
            let mut text = serde_json::to_string_pretty(&items)
                .map_err(|e| CoreError::Parse(format!("{}: {e}", self.path.display())))?;
            text.push('\n');
            crate::lock::write_whole(&self.path, text.as_bytes())?;
        }
        Ok(out)
    }
}

/// The `StepRunner` of one spec's run: a human step raises its item (or finds the open one) and
/// reports not now, so the unit stays where it is; every other step goes to `inner`.
pub struct OwnerSteps<'a> {
    pub spec: &'a Spec,
    pub items: &'a RunnerItems,
    pub now: UnixSeconds,
    pub inner: &'a dyn StepRunner,
}

impl StepRunner for OwnerSteps<'_> {
    fn run(&self, step: &Step, unit: &str) -> Result<Outcome, CoreError> {
        if step.actor != Actor::Human {
            return self.inner.run(step, unit);
        }
        let ask = step.ask.as_ref().ok_or_else(|| {
            CoreError::Parse(format!("step {}: a human step needs `ask`", step.id))
        })?;
        self.items.raise(self.spec, step, unit, ask, self.now)?;
        Ok(Outcome::NotNow)
    }
}
