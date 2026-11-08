//! The abilities index: the pipeline repo's action table (`tools/fleet/actions.json`, read with
//! `fleet actions --json`), with the request kinds the front door routes by (FD-1, FD-3) and
//! the `needs` tags the loadout must match (FD-12).
//!
//! An entry with `"request": true` is a request kind; the rest are fleet job actions.

use crate::fleet::FleetBridge;
use crate::CoreError;
use serde_json::{Map, Value};

/// One action or request kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ability {
    /// The table key, e.g. `quick-lookup`.
    pub id: String,
    pub label: String,
    /// `needs` tags, e.g. `["web"]`; empty when it needs nothing.
    pub needs: Vec<String>,
    /// `true` for a front-door request kind (`"request": true`).
    pub request: bool,
    /// The services to try, in order (`first`); empty when the entry has none.
    pub first: Vec<String>,
    pub writes: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AbilitiesIndex {
    /// Every entry, in id order.
    pub entries: Vec<Ability>,
}

impl AbilitiesIndex {
    /// Parse the JSON object `fleet actions --json` prints: one `Ability` per key, the entries
    /// sorted by id (plain `String` order), never by the object's key order. A key that is
    /// absent or `null` takes its default; a key of the wrong type is a `CoreError::Parse`
    /// naming the id. The input is not changed.
    pub fn from_json(value: &Value) -> Result<AbilitiesIndex, CoreError> {
        let map = value.as_object().ok_or_else(|| {
            CoreError::Parse("the action table must be a JSON object keyed by id".to_string())
        })?;
        let mut entries = Vec::with_capacity(map.len());
        for (id, entry) in map {
            entries.push(parse_entry(id, entry)?);
        }
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(AbilitiesIndex { entries })
    }

    /// Read it through the fleet bridge (`fleet actions --json`); a failed command is a
    /// `CoreError::Fleet`.
    pub fn load(fleet: &FleetBridge) -> Result<AbilitiesIndex, CoreError> {
        let value = fleet.run_json(&["actions"]).map_err(CoreError::Fleet)?;
        Self::from_json(&value)
    }

    /// The request kinds only, in id order.
    pub fn request_kinds(&self) -> Vec<&Ability> {
        self.entries.iter().filter(|a| a.request).collect()
    }

    pub fn get(&self, id: &str) -> Option<&Ability> {
        self.entries.iter().find(|a| a.id == id)
    }

    /// The request kinds whose `needs` contain `tag`, in id order (FD-1's explicit-ask step).
    /// Job actions are never listed.
    pub fn kinds_needing(&self, tag: &str) -> Vec<&Ability> {
        self.request_kinds()
            .into_iter()
            .filter(|a| a.needs.iter().any(|need| need == tag))
            .collect()
    }
}

fn bad(id: &str, what: &str) -> CoreError {
    CoreError::Parse(format!("ability '{id}': {what}"))
}

fn parse_entry(id: &str, entry: &Value) -> Result<Ability, CoreError> {
    let entry = match entry.as_object() {
        Some(entry) => entry,
        None => return Err(bad(id, "the entry is not an object")),
    };
    Ok(Ability {
        id: id.to_string(),
        label: field_str(entry, id, "label")?,
        needs: field_strs(entry, id, "needs")?,
        request: field_bool(entry, id, "request", false)?,
        first: field_strs(entry, id, "first")?,
        writes: field_bool(entry, id, "writes", false)?,
    })
}

fn field_str(entry: &Map<String, Value>, id: &str, key: &str) -> Result<String, CoreError> {
    match entry.get(key) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(bad(id, &format!("`{key}` is not a string"))),
    }
}

fn field_strs(entry: &Map<String, Value>, id: &str, key: &str) -> Result<Vec<String>, CoreError> {
    match entry.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item.as_str() {
                Some(s) => Ok(s.to_string()),
                None => Err(bad(id, &format!("`{key}` is not an array of strings"))),
            })
            .collect(),
        Some(_) => Err(bad(id, &format!("`{key}` is not an array of strings"))),
    }
}

fn field_bool(
    entry: &Map<String, Value>,
    id: &str,
    key: &str,
    default: bool,
) -> Result<bool, CoreError> {
    match entry.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(bad(id, &format!("`{key}` is not a bool"))),
    }
}
