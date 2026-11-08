//! AUTO-9: a job's failure is classed by the done card's `outcome` and `failure_mode`, never by
//! the exit code: `fleet errors classify --json` decides, the core reads its answer.

use crate::fleet::FleetBridge;
use crate::CoreError;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureClass {
    Passed,
    /// The fleet's (rate limit, lock timeout, push failed...): an IT ticket (IT-2).
    Fleet {
        signature: String,
    },
    /// The code's: the ladder climbs; out of ladder, a bug (IT-4).
    Coding,
    /// The brief's or the job's: nothing filed, the Project Manager fixes it.
    Author,
}

/// Read one answer of `fleet errors classify --json` for a card. The fleet's `class` decides:
/// `none` passed, `fleet` (with its `signature`), `coding` or `author`. Anything else is an
/// error, never a guessed class.
pub fn from_json(value: &Value) -> Result<FailureClass, CoreError> {
    let answer = value
        .as_object()
        .ok_or_else(|| CoreError::Parse("classify: expected an object".into()))?;
    let class = answer
        .get("class")
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::Parse("classify: no string class".into()))?;
    match class {
        "none" => Ok(FailureClass::Passed),
        "fleet" => {
            let signature = answer
                .get("signature")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    CoreError::Parse("classify: fleet answer has no string signature".into())
                })?;
            Ok(FailureClass::Fleet {
                signature: signature.to_string(),
            })
        }
        "coding" => Ok(FailureClass::Coding),
        "author" => Ok(FailureClass::Author),
        other => Err(CoreError::Parse(format!("classify: unknown class {other}"))),
    }
}

/// AUTO-9: run `fleet errors classify --json <card>` and read its answer. The exit code is
/// not read here: a failed run is the bridge's `FleetError`, as `budget::read` passes it on.
pub fn classify(fleet: &FleetBridge, card: &Path) -> Result<FailureClass, CoreError> {
    let card_arg = card
        .to_str()
        .ok_or_else(|| CoreError::Io(format!("card path is not UTF-8: {}", card.display())))?;
    let value = fleet
        .run_json(&["errors", "classify", "--json", card_arg])
        .map_err(CoreError::Fleet)?;
    from_json(&value)
}
