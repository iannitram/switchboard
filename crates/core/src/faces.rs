//! The core command registry: each command's effects and the faces that expose it.
//!
//! The registry shape follows the command/tool descriptors in `storytold/craft-anything`
//! at `2e7f276` (MIT OR Apache-2.0), adapted for Switchboard's three faces and effect model.

use serde_json::{json, Value};

use crate::rules::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Read,
    WriteLocal,
    WriteGithub,
    Network,
    Spawn,
}

pub struct CommandSpec {
    pub id: &'static str,
    pub effects: &'static [Effect],
    pub tauri: Option<&'static str>,
    pub cli: Option<&'static str>,
    pub mcp: Option<&'static str>,
    pub ui_only: Option<&'static str>,
    pub mcp_roles: &'static [Role],
    pub mcp_input: Option<fn() -> Value>,
    pub mcp_description: Option<&'static str>,
    /// The tool goes through the front door (`frontdoor::call_from`), not a hand-written arm.
    pub front_door: bool,
}

impl CommandSpec {
    /// The default MCP list exposes only role-free commands with no declared write or execution effect.
    pub fn listed_without_role(&self) -> bool {
        self.mcp.is_some() && self.effects == [Effect::Read] && self.mcp_roles.is_empty()
    }
}

const READ: &[Effect] = &[Effect::Read];
const LOCAL: &[Effect] = &[Effect::WriteLocal];
const GITHUB: &[Effect] = &[Effect::WriteGithub];
const NETWORK_READ: &[Effect] = &[Effect::Read, Effect::Network];
const SPAWN: &[Effect] = &[Effect::Spawn];
const ALL_ROLES: &[Role] = &[];
const PM: &[Role] = &[Role::ProjectManager];
const BM: &[Role] = &[Role::BudgetManager];
const SATELLITE: &[Role] = &[Role::Satellite];
const PM_SATELLITE: &[Role] = &[Role::ProjectManager, Role::Satellite];

fn empty() -> Value {
    json!({"type":"object","properties":{}})
}
fn sky() -> Value {
    empty()
}
fn constellation() -> Value {
    json!({"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"}},"required":["repo"]})
}
fn star() -> Value {
    json!({"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"},"issue":{"type":"integer","minimum":1}},"required":["repo","issue"]})
}
fn waiting() -> Value {
    json!({"type":"object","properties":{"limit":{"type":"integer","minimum":1}}})
}
fn reports() -> Value {
    json!({"type":"object","properties":{"limit":{"type":"integer","minimum":1},"project":{"type":"string"},"outcome":{"type":"string"},"pool":{"type":"string"}}})
}
fn budget() -> Value {
    empty()
}
fn errors() -> Value {
    empty()
}
fn autopilot_status() -> Value {
    empty()
}
fn merge() -> Value {
    json!({"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"},"pr":{"type":"integer","minimum":1}},"required":["repo","pr"]})
}
fn request_archive() -> Value {
    repo_required()
}
fn request_restore() -> Value {
    repo_required()
}
fn repo_required() -> Value {
    json!({"type":"object","properties":{"repo":{"type":"string","description":"The repo's name or owner/name"}},"required":["repo"]})
}
fn drop_request() -> Value {
    json!({"type":"object","properties":{"kind":{"type":"string","description":"archive or restore"},"repo":{"type":"string","description":"The repo's name or owner/name"}},"required":["kind","repo"]})
}
fn set_pool_cap() -> Value {
    json!({"type":"object","properties":{"pool":{"type":"string","description":"The pool's name"},"cap":{"type":"number","minimum":0}},"required":["pool","cap"]})
}
fn pool_edit() -> Value {
    json!({"type":"object","properties":{"pool":{"type":"string","description":"The pool's name"},"plan":{"type":"string"},"cap":{"type":"number","minimum":0},"reset":{"type":"string","description":"e.g. Sun 6:00am"},"cost_usd":{"type":"number","minimum":0}},"required":["pool"]})
}
fn budget_refresh() -> Value {
    empty()
}
fn autopilot_pass() -> Value {
    json!({"type":"object","properties":{"pass":{"type":"string","enum":["roadmap","review","merge","weekly"]},"dry_run":{"type":"boolean"}},"required":["pass"]})
}
fn satellite_tick() -> Value {
    json!({"type":"object","properties":{"dry_run":{"type":"boolean"}}})
}
fn ask() -> Value {
    json!({"type":"object","properties":{"message":{"type":"string"},"thread":{"type":"string","description":"An open thread's id; none starts a thread"},"dry_run":{"type":"boolean"}},"required":["message"]})
}
fn ask_as_job() -> Value {
    json!({"type":"object","properties":{"kind":{"type":"string","description":"The request kind"},"request":{"type":"string"},"unmet":{"type":"array","items":{"type":"string"},"minItems":1,"description":"The needs this machine does not meet; left out, they are read from this machine's loadout"},"thread":{"type":"string","description":"The chat's thread id, to send the chat with its packet"}},"required":["kind","request"]})
}
fn rate() -> Value {
    json!({"type":"object","properties":{"thread":{"type":"string"},"turn":{"type":"integer","minimum":0,"description":"The answer's index among the thread's turns"},"rating":{"type":"string","enum":["up","down"]}},"required":["thread","turn","rating"]})
}

macro_rules! row {
    ($id:literal, $effects:expr, $tauri:expr, $cli:expr, $mcp:expr, $ui:expr, $roles:expr, $input:expr, $desc:expr) => {
        row!(@ false, $id, $effects, $tauri, $cli, $mcp, $ui, $roles, $input, $desc)
    };
    (@ $fd:expr, $id:literal, $effects:expr, $tauri:expr, $cli:expr, $mcp:expr, $ui:expr, $roles:expr, $input:expr, $desc:expr) => {
        CommandSpec {
            id: $id,
            effects: $effects,
            tauri: $tauri,
            cli: $cli,
            mcp: $mcp,
            ui_only: $ui,
            mcp_roles: $roles,
            mcp_input: $input,
            mcp_description: $desc,
            front_door: $fd,
        }
    };
}

/// Rows with MCP tools retain the long-standing `tools/list` order.
pub static REGISTRY: &[CommandSpec] = &[
    // MCP-1 read tools, in their established order.
    row!("sky", READ, Some("sky"), Some("sky"), Some("sky"), None, ALL_ROLES, Some(sky), Some("The sky: one constellation per repo, as the GUI shows it.")),
    row!("constellation", READ, None, None, Some("constellation"), None, ALL_ROLES, Some(constellation), Some("One repo's constellation: its stars and where they sit.")),
    row!("star", READ, None, Some("star"), Some("star"), None, ALL_ROLES, Some(star), Some("One star: an issue or PR with its labels, size, PR and review.")),
    row!("waiting", READ, Some("waiting"), Some("waiting"), Some("waiting"), None, ALL_ROLES, Some(waiting), Some("The Waiting on you list: items that need a person, one merged list.")),
    row!("reports", READ, Some("reports"), Some("reports"), Some("reports"), None, ALL_ROLES, Some(reports), Some("Reports built from job cards and handoff threads.")),
    row!("budget_rows", READ, Some("budget_rows"), Some("budget"), Some("budget"), None, ALL_ROLES, Some(budget), Some("Fleet budget: the pool rows and what is left in each.")),
    // The role reads follow the six default reads.
    row!("fleet_errors", READ, Some("fleet_errors"), Some("errors"), Some("errors"), None, SATELLITE, Some(errors), Some("The fleet errors the app recorded, newest first, and the open IT tickets.")),
    row!("autopilot_status", READ, None, Some("autopilot"), Some("autopilot_status"), None, PM_SATELLITE, Some(autopilot_status), Some("The autopilot's state: what its passes have started and seen, whether the weekly run is due, the running jobs, and the satellite's loops and holds.")),
    // Project-manager tools, preserving tools/list order.
    row!("merge", GITHUB, None, Some("merge"), Some("merge"), None, PM, Some(merge), Some("Merge a PR once its latest PASS is on its head and CI is green on that head.")),
    row!(@ true, "ask", &[Effect::Network, Effect::WriteLocal], Some("chat_route"), Some("ask"), Some("ask"), None, PM, Some(ask), Some("Route one message through the front door and answer it; dry_run returns the route and its reason and sends, stores and spends nothing.")),
    row!(@ true, "ask_as_job", &[Effect::WriteLocal, Effect::Spawn], Some("chat_send_as_job"), Some("ask_as_job"), Some("ask_as_job"), None, PM, Some(ask_as_job), Some("Send a request this machine cannot serve as a fleet job another machine pulls. With thread, the job carries that chat's packet (summary, last 6 turns, links) and the move is stored.")),
    row!(@ true, "rate", LOCAL, Some("chat_rate"), Some("rate"), Some("rate"), None, PM, Some(rate), Some("Rate one answer of a thread up or down; a second rating of a turn replaces the first.")),
    row!("request_archive", LOCAL, None, Some("archive"), Some("request_archive"), None, PM, Some(request_archive), Some("Ask to archive a repo (read-only on GitHub). This only files a Waiting on you request; the owner confirms it in the app.")),
    row!("request_restore", LOCAL, None, Some("restore"), Some("request_restore"), None, PM, Some(request_restore), Some("Ask to restore a repo's backup over the live repo. This only files a Waiting on you request; the owner confirms it in the app.")),
    row!("drop_request", LOCAL, Some("drop_guarded"), Some("drop"), Some("drop_request"), None, PM, Some(drop_request), Some("Turn down a waiting archive or restore request: it leaves Waiting on you and nothing else happens.")),
    row!("set_pool_cap", LOCAL, None, Some("set_pool_cap"), Some("set_pool_cap"), None, BM, Some(set_pool_cap), Some("Set one budget pool's dollar cap; the reply states the new cap.")),
    row!("pool_edit", LOCAL, Some("budget_save_edit"), Some("pool_edit"), Some("pool_edit"), None, BM, Some(pool_edit), Some("Edit one budget pool in place (plan, cap, reset, cost), as the Budget panel does.")),
    row!("budget_refresh", NETWORK_READ, Some("budget_refresh"), Some("budget"), Some("budget_refresh"), None, BM, Some(budget_refresh), Some("Re-read every pool that has a reader now, then return the budget rows.")),
    row!("autopilot_pass", &[Effect::WriteLocal, Effect::Spawn], None, Some("autopilot"), Some("autopilot_pass"), None, PM, Some(autopilot_pass), Some("Run one autopilot pass now, as the app's poll does, and return its report. With dry_run it starts no job, merges nothing and writes no state, and reports what it would do.")),
    row!("satellite_tick", &[Effect::WriteLocal, Effect::Spawn], None, Some("autopilot"), Some("satellite_tick"), None, SATELLITE, Some(satellite_tick), Some("Run one tick of the satellite loop on this machine and return its report; the loops it starts run on while this server lives. With dry_run it starts nothing and reports the lanes it would start.")),
    // Tauri and command-only CLI faces.
    row!("views", READ, Some("views"), None, None, Some("the window's tab order"), ALL_ROLES, None, None),
    row!("settings_missing", READ, Some("settings_missing"), Some("settings"), None, None, ALL_ROLES, None, None),
    row!("settings_get", READ, Some("settings_get"), Some("settings"), None, None, ALL_ROLES, None, None),
    row!("settings_save", LOCAL, Some("settings_save"), None, None, Some("settings are changed in the app only (ruling on #373)"), ALL_ROLES, None, None),
    row!("chat_implicit", LOCAL, Some("chat_implicit"), None, None, Some("a signal from a press in the app (FD-8, FD-9)"), ALL_ROLES, None, None),
    row!("refresh_all", NETWORK_READ, Some("refresh_all"), Some("refresh"), None, None, ALL_ROLES, None, None),
    row!("backup_repo", &[Effect::Read, Effect::WriteLocal, Effect::Network], Some("backup_repo"), Some("backup"), None, None, ALL_ROLES, None, None),
    row!("backup_all", &[Effect::Read, Effect::WriteLocal, Effect::Network], Some("backup_all"), Some("backup"), None, None, ALL_ROLES, None, None),
    row!("loadout", READ, Some("loadout"), Some("loadout"), None, None, ALL_ROLES, None, None),
    row!("confirm_guarded", GITHUB, Some("confirm_guarded"), None, None, Some("the typed confirm is in the app only (CORE-3)"), ALL_ROLES, None, None),
    row!("confirm", GITHUB, None, Some("confirm"), None, None, ALL_ROLES, None, None),
    row!("autopilot", SPAWN, None, Some("autopilot"), None, None, ALL_ROLES, None, None),
    row!("spec", READ, None, Some("spec"), None, None, ALL_ROLES, None, None),
    row!("roadmap", READ, None, Some("roadmap"), None, None, ALL_ROLES, None, None),
    row!("mcp", READ, None, Some("mcp"), None, None, ALL_ROLES, None, None),
];
