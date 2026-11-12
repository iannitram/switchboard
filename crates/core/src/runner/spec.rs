//! Typed workflow documents and validation. Loading a spec never runs its commands.

use std::collections::HashSet;
use std::path::Path;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use crate::CoreError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec {
    pub id: String,
    pub version: String,
    pub purpose: String,
    pub sources: Vec<Source>,
    pub records: Vec<Record>,
    pub states: Vec<State>,
    pub steps: Vec<Step>,
    pub scorers: Vec<Scorer>,
    pub invariants: Vec<Invariant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub name: String,
    #[serde(rename = "ref")]
    pub reference: String,
}

/// A record schema describes stored workflow data; its type expressions stay literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    pub lives_in: String,
    pub fields: Vec<Field>,
    pub written_by: String,
    pub read_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub id: String,
    pub meaning: String,
    pub terminal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Actor {
    Code,
    Small,
    Strong,
    Human,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub from_state: String,
    pub to_state_ok: String,
    pub to_state_fail: String,
    pub actor: Actor,
    pub origin: String,
    pub origin_note: String,
    pub inputs: Vec<Input>,
    pub run: Run,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask: Option<Ask>,
    pub outputs: Vec<Output>,
    pub done_when: Vec<Check>,
    pub on_fail: OnFail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input {
    pub name: String,
    pub from: String,
    #[serde(rename = "type")]
    pub value_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    Command,
    ModelTask,
    Question,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub kind: RunKind,
    pub command: String,
    pub home: String,
    pub exists: bool,
    pub task: String,
    pub max_context_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    pub needs: String,
    pub why: String,
    pub choices: Vec<String>,
    pub recommended: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub check: String,
    pub exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnFail {
    pub retry: u64,
    pub then: String,
    pub escalate_via: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scorer {
    pub id: String,
    pub targets: Vec<String>,
    pub labels: Vec<ScoreLabel>,
    pub pass_threshold: f64,
    pub sample_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreLabel {
    pub label: String,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invariant {
    pub id: String,
    pub statement: String,
    // This may name an external command, not just a step in this document.
    pub checked_by: String,
    pub exists: bool,
}

/// Read a UTF-8 JSON file, refusing malformed data or invalid references.
pub fn load(path: &Path) -> Result<Spec, CoreError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CoreError::Io(format!("{}: {error}", path.display())))?;
    parse(&text)
}

/// Decode step keys separately so serde errors retain the step id and offending key.
pub fn parse(text: &str) -> Result<Spec, CoreError> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|error| CoreError::Parse(format!("spec: {error}")))?;
    let steps = value
        .get_mut("steps")
        .ok_or_else(|| CoreError::Parse("spec: missing field `steps`".into()))?
        .take();
    let steps = steps
        .as_array()
        .ok_or_else(|| CoreError::Parse("spec: steps must be an array".into()))?;
    value["steps"] = serde_json::json!([]);
    let mut spec: Spec = serde_json::from_value(value)
        .map_err(|error| CoreError::Parse(format!("spec: {error}")))?;
    for (index, step) in steps.iter().enumerate() {
        let context = step
            .get("id")
            .and_then(Value::as_str)
            .map(|id| format!("step {id}"))
            .unwrap_or_else(|| format!("steps[{index}]"));
        spec.steps.push(Step {
            id: step_key(step, &context, "id")?,
            from_state: step_key(step, &context, "from_state")?,
            to_state_ok: step_key(step, &context, "to_state_ok")?,
            to_state_fail: step_key(step, &context, "to_state_fail")?,
            actor: step_key(step, &context, "actor")?,
            origin: step_key(step, &context, "origin")?,
            origin_note: step_key(step, &context, "origin_note")?,
            inputs: step_key(step, &context, "inputs")?,
            run: step_key(step, &context, "run")?,
            ask: if step.get("ask").is_some() {
                Some(step_key(step, &context, "ask")?)
            } else {
                None
            },
            outputs: step_key(step, &context, "outputs")?,
            done_when: step_key(step, &context, "done_when")?,
            on_fail: step_key(step, &context, "on_fail")?,
        });
    }
    let errors = spec.validate();
    if errors.is_empty() {
        Ok(spec)
    } else {
        Err(CoreError::Parse(errors.join("; ")))
    }
}

fn step_key<T: DeserializeOwned>(step: &Value, context: &str, key: &str) -> Result<T, CoreError> {
    let value = step
        .get(key)
        .ok_or_else(|| CoreError::Parse(format!("{context}: missing key `{key}`")))?;
    serde_json::from_value(value.clone())
        .map_err(|error| CoreError::Parse(format!("{context}: key `{key}`: {error}")))
}

impl Spec {
    /// Check references without executing commands or requiring the declared commands to exist.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let states: HashSet<&str> = self.states.iter().map(|state| state.id.as_str()).collect();
        for step in &self.steps {
            for (key, state) in [
                ("from_state", &step.from_state),
                ("to_state_ok", &step.to_state_ok),
                ("to_state_fail", &step.to_state_fail),
            ] {
                if !states.contains(state.as_str()) {
                    errors.push(format!(
                        "step {}: key `{key}`: unknown state id `{state}`",
                        step.id
                    ));
                }
            }
        }
        errors
    }
}
