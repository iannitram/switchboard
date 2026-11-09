//! The abilities index: the pipeline repo's action table (`tools/fleet/actions.json`, read with
//! `fleet actions --json`), with the request kinds the front door routes by (FD-1, FD-3) and
//! the `needs` tags the loadout must match (FD-12).
//!
//! An entry with `"request": true` is a request kind; the rest are fleet job actions.
//!
//! FD-3's model for a service is the entry's `ok.<service>.model` row. Two services carry a
//! flag there instead of a row, and their models come from the pipeline's `tools/fleet/config.json`
//! (`FleetModels`, the ruling on #130): `ok.qwen: true` is this machine's
//! `machines.<machine>.qwen_model`, and a top-level `claudeapi: true` is `ok.claude`'s tier
//! (`sonnet`) mapped through `claudeapi_models`. A config that cannot be read fills neither, so
//! those services have no model and the router skips them.

use crate::budget::config_path;
use crate::fleet::FleetBridge;
use crate::CoreError;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The environment variable the fleet tools read for this machine's token (`FLEET_MACHINE`).
pub const FLEET_MACHINE_ENV: &str = "FLEET_MACHINE";

/// The file the fleet tools fall back to for this machine's token, under the home folder.
pub const MACHINE_FILE: &str = ".config/software-depot/machine";

/// FD-3: the model names `tools/fleet/config.json` supplies where `actions.json` holds a flag
/// instead of a `{model}` row. Empty (`Default`) when the config cannot be read: then neither
/// flag gets a model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FleetModels {
    /// `machines.<this machine>.qwen_model`: the model `ok.qwen: true` names. `None` when the
    /// machine is unknown, not in `machines`, or has no `qwen_model` (not Qwen-capable).
    pub qwen: Option<String>,
    /// `claudeapi_models`: a Claude tier (`sonnet`) to the API model id. A top-level
    /// `claudeapi: true` takes `ok.claude`'s tier through this map.
    pub claudeapi: BTreeMap<String, String>,
}

impl FleetModels {
    /// The models from a parsed `config.json` for `machine`. A key that is absent or of the
    /// wrong type is simply not there; nothing is an error.
    pub fn from_config(config: &Value, machine: Option<&str>) -> FleetModels {
        let qwen = machine
            .and_then(|m| config.get("machines")?.get(m)?.get("qwen_model")?.as_str())
            .map(str::to_string);
        let claudeapi = config
            .get("claudeapi_models")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(tier, model)| Some((tier.clone(), model.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        FleetModels { qwen, claudeapi }
    }

    /// Read `<root>/tools/fleet/config.json` through the bridge's root (`budget::config_path`)
    /// for `machine`. `None` when the file is missing, unreadable or not JSON: the caller takes
    /// `FleetModels::default()` and the flagged services are skipped.
    pub fn read(fleet: &FleetBridge, machine: Option<&str>) -> Option<FleetModels> {
        let text = std::fs::read_to_string(config_path(fleet)).ok()?;
        let config: Value = serde_json::from_str(&text).ok()?;
        Some(Self::from_config(&config, machine))
    }

    /// This machine's token the way the fleet tools find it (`local_machine` in the pipeline's
    /// `tools/fleet/common.py`): `FLEET_MACHINE` when set and not blank, else the stripped
    /// contents of `~/.config/software-depot/machine`, either one run through `canon_machine`;
    /// `None` without either.
    pub fn local_machine() -> Option<String> {
        let env = std::env::var_os(FLEET_MACHINE_ENV);
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        let file = home.map(|h| h.join(MACHINE_FILE));
        Self::resolve_machine(env.as_deref(), file.as_deref())
    }

    /// `local_machine` over the values it is given, as `common.py`'s `local_machine` does it: a
    /// non-blank `env` wins, else the whole of `machine_file` stripped of surrounding whitespace
    /// (Python's `read_text().strip()`, not only its first line) when it reads and is not blank.
    /// Either is passed through `canon_machine`, so an old token (`Z13`) comes back as its
    /// current ID (`strix-halo`).
    pub fn resolve_machine(env: Option<&OsStr>, machine_file: Option<&Path>) -> Option<String> {
        if let Some(value) = env.and_then(OsStr::to_str) {
            if !value.trim().is_empty() {
                return Some(canon_machine(value));
            }
        }
        let text = std::fs::read_to_string(machine_file?).ok()?;
        let token = canon_machine(&text);
        (!token.is_empty()).then_some(token)
    }
}

/// The old machine tokens and their current IDs, lower-case keys. A copy of `MACHINE_ALIASES`
/// in the pipeline's `tools/fleet/common.py`, which is the source of truth: when an entry is
/// added or changed there, change it here too. Machine files, `FLEET_MACHINE` values and rows
/// written before the 2026-10-27 rename still hold the old tokens.
pub const MACHINE_ALIASES: &[(&str, &str)] = &[
    ("hyperion", "desktop-3090ti"),
    ("z13", "strix-halo"),
    ("legiongo", "amd-handheld"),
    ("shuttlecraft6", "mac-mini"),
    ("mac", "mac-mini"),
];

/// The current machine ID for a token as typed or as recorded long ago, mirroring
/// `canon_machine` in the pipeline's `tools/fleet/common.py` (the source of truth): the token
/// is stripped, an old token (matched case-insensitively) comes back as its current ID, and
/// anything else comes back stripped and otherwise unchanged.
pub fn canon_machine(token: &str) -> String {
    let token = token.trim();
    let lower = token.to_lowercase();
    MACHINE_ALIASES
        .iter()
        .find(|(old, _)| *old == lower)
        .map_or_else(|| token.to_string(), |(_, current)| (*current).to_string())
}

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
    /// Per id, the model for each service (FD-3): `ok.<service>.model` for each service whose
    /// `ok` entry is an object with a string `model`, plus, from the `FleetModels` the index was
    /// built with, `qwen` for an `ok.qwen` of `true` and `claudeapi` for a top-level
    /// `claudeapi: true` whose `ok.claude` tier the config maps. A service whose entry is null,
    /// false or a bare string has no row, and an explicit row wins over a derived one. Every id
    /// has a map, empty when nothing gives it a row. Kept off `Ability` so an `Ability` literal
    /// still names only its six fields.
    pub models: BTreeMap<String, BTreeMap<String, String>>,
}

impl AbilitiesIndex {
    /// Parse the JSON object `fleet actions --json` prints: one `Ability` per key, the entries
    /// sorted by id (plain `String` order), never by the object's key order. A key that is
    /// absent or `null` takes its default; a key of the wrong type is a `CoreError::Parse`
    /// naming the id. The input is not changed. The model rows are the explicit
    /// `ok.<service>.model` ones only (`from_json_with` adds the fleet config's).
    pub fn from_json(value: &Value) -> Result<AbilitiesIndex, CoreError> {
        Self::from_json_with(value, &FleetModels::default())
    }

    /// `from_json`, with `fleet` filling the model rows `actions.json` leaves as flags
    /// (`ok.qwen: true`, top-level `claudeapi: true`).
    pub fn from_json_with(value: &Value, fleet: &FleetModels) -> Result<AbilitiesIndex, CoreError> {
        let map = value.as_object().ok_or_else(|| {
            CoreError::Parse("the action table must be a JSON object keyed by id".to_string())
        })?;
        let mut entries = Vec::with_capacity(map.len());
        let mut models = BTreeMap::new();
        for (id, entry) in map {
            entries.push(parse_entry(id, entry)?);
            models.insert(id.clone(), parse_models(id, entry, fleet)?);
        }
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(AbilitiesIndex { entries, models })
    }

    /// Read it through the fleet bridge (`fleet actions --json`); a failed command is a
    /// `CoreError::Fleet`. Explicit model rows only, as `from_json`; the front door loads with
    /// `load_with` so qwen and the Claude API get their models.
    pub fn load(fleet: &FleetBridge) -> Result<AbilitiesIndex, CoreError> {
        let value = fleet.run_json(&["actions"]).map_err(CoreError::Fleet)?;
        Self::from_json(&value)
    }

    /// `load`, with the model rows the pipeline's `tools/fleet/config.json` supplies for
    /// `machine` (`FleetModels::read`; `FleetModels::local_machine()` names this machine). A
    /// config that cannot be read is not an error: those services simply have no model.
    pub fn load_with(
        fleet: &FleetBridge,
        machine: Option<&str>,
    ) -> Result<AbilitiesIndex, CoreError> {
        let value = fleet.run_json(&["actions"]).map_err(CoreError::Fleet)?;
        let models = FleetModels::read(fleet, machine).unwrap_or_default();
        Self::from_json_with(&value, &models)
    }

    /// The request kinds only, in id order.
    pub fn request_kinds(&self) -> Vec<&Ability> {
        self.entries.iter().filter(|a| a.request).collect()
    }

    pub fn get(&self, id: &str) -> Option<&Ability> {
        self.entries.iter().find(|a| a.id == id)
    }

    /// The model `ok.<service>.model` names for `id`, or `None` when the entry gives that
    /// service no model row (or there is no such entry).
    pub fn model(&self, id: &str, service: &str) -> Option<&str> {
        self.models.get(id)?.get(service).map(String::as_str)
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

/// `ok.<service>.model` for each service whose `ok` entry is an object with a string `model`,
/// then the two flags `fleet` can fill: `ok.qwen: true` takes `fleet.qwen`, and a top-level
/// `claudeapi: true` takes `ok.claude`'s tier through `fleet.claudeapi` (only when no explicit
/// `ok.claudeapi` row named one). An `ok` that is absent or null gives no rows; one that is not
/// an object is a parse error.
fn parse_models(
    id: &str,
    entry: &Value,
    fleet: &FleetModels,
) -> Result<BTreeMap<String, String>, CoreError> {
    let ok = match entry.get("ok") {
        None | Some(Value::Null) => return Ok(BTreeMap::new()),
        Some(Value::Object(ok)) => ok,
        Some(_) => return Err(bad(id, "`ok` is not an object")),
    };
    let mut models: BTreeMap<String, String> = ok
        .iter()
        .filter_map(|(service, value)| {
            let model = value.get("model")?.as_str()?;
            Some((service.clone(), model.to_string()))
        })
        .collect();
    if ok.get(QWEN) == Some(&Value::Bool(true)) {
        if let Some(model) = &fleet.qwen {
            models
                .entry(QWEN.to_string())
                .or_insert_with(|| model.clone());
        }
    }
    if entry.get(CLAUDE_API) == Some(&Value::Bool(true)) {
        let tier = ok.get("claude").and_then(Value::as_str);
        if let Some(model) = tier.and_then(|tier| fleet.claudeapi.get(tier)) {
            models
                .entry(CLAUDE_API.to_string())
                .or_insert_with(|| model.clone());
        }
    }
    Ok(models)
}

/// The service whose `ok` flag `true` means "this machine's Qwen" (`FleetModels::qwen`).
const QWEN: &str = "qwen";

/// The service a top-level `claudeapi: true` turns on (`FleetModels::claudeapi`).
const CLAUDE_API: &str = "claudeapi";

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
