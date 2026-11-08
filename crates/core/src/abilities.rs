//! The abilities index: the pipeline repo's action table (`tools/fleet/actions.json`, read with
//! `fleet actions --json`), with the request kinds the front door routes by (FD-1, FD-3) and
//! the `needs` tags the loadout must match (FD-12).
//!
//! An entry with `"request": true` is a request kind; the rest are fleet job actions.

use crate::fleet::FleetBridge;
use crate::CoreError;
use serde_json::Value;

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
    /// Every entry, in the table's key order.
    pub entries: Vec<Ability>,
}

impl AbilitiesIndex {
    /// Parse the JSON object `fleet actions --json` prints.
    pub fn from_json(value: &Value) -> Result<AbilitiesIndex, CoreError> {
        todo!("abilities index")
    }

    /// Read it through the fleet bridge.
    pub fn load(fleet: &FleetBridge) -> Result<AbilitiesIndex, CoreError> {
        todo!("abilities index")
    }

    /// The request kinds only, in table order.
    pub fn request_kinds(&self) -> Vec<&Ability> {
        todo!("abilities index")
    }

    pub fn get(&self, id: &str) -> Option<&Ability> {
        todo!("abilities index")
    }

    /// The request kinds whose `needs` contain `tag`, in table order (FD-1's explicit-ask step).
    pub fn kinds_needing(&self, tag: &str) -> Vec<&Ability> {
        todo!("abilities index")
    }
}
