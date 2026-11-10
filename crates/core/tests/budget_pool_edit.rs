//! BUD-2 for every face (#340, #362): `save_pool_edit` saves the edit, its ledger row in
//! `<data>/pool-edits.jsonl` and its Waiting on you item, all or nothing; `edit_summary` says
//! what the edit set. MCP-2, MCP-3, CLI-3: `rules::guard_pool_edit` says who may edit a pool.

use serde_json::json;
use std::path::{Path, PathBuf};
use switchboard_core::budget::{edit_summary, save_pool_edit, PoolEdit};
use switchboard_core::clock::FixedClock;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::rules::{guard_pool_edit, Caller, Face, Role, Verdict};
use switchboard_core::waiting::{LocalItems, Source};

const CONFIG: &str = r#"{
  "pools": {
    "claude": { "kind": "weekly", "plan": "max-1x" },
    "deepseek": { "kind": "fixed-period", "cap_usd": 10 }
  }
}
"#;

/// A pipeline root with the fleet stub and CONFIG, and an empty data folder beside it.
fn dirs(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("sb-bud2-pool-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("pipeline");
    let fleet = root.join("tools").join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::create_dir_all(root.join("answers")).unwrap();
    let stub = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fleet-stub");
    std::fs::copy(stub.join("tools/fleet/fleet.py"), fleet.join("fleet.py")).unwrap();
    std::fs::copy(
        stub.join("answers/budget.json"),
        root.join("answers/budget.json"),
    )
    .unwrap();
    std::fs::write(fleet.join("config.json"), CONFIG).unwrap();
    let data = base.join("data");
    std::fs::create_dir_all(&data).unwrap();
    (root, data)
}

fn config_text(root: &Path) -> String {
    std::fs::read_to_string(root.join("tools/fleet/config.json")).unwrap()
}

fn cap(pool: &str, cap: f64) -> PoolEdit {
    PoolEdit {
        pool: pool.into(),
        cap: Some(cap),
        ..PoolEdit::default()
    }
}

#[test]
fn a_saved_edit_lands_the_file_the_ledger_row_and_the_waiting_item() {
    let (root, data) = dirs("ok");
    let saved = save_pool_edit(
        &FleetBridge::new(&root),
        &cap("deepseek", 7.0),
        &data,
        &FixedClock::at(5),
    )
    .unwrap();
    assert!(config_text(&root).contains("\"cap_usd\": 7.0"));
    let ledger = std::fs::read_to_string(data.join("pool-edits.jsonl")).unwrap();
    assert_eq!(ledger.lines().count(), 1);
    let items = LocalItems::new(&data).load().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].source,
        Source::PoolEdit {
            pool: "deepseek".into()
        }
    );
    assert_eq!(items[0], saved.waiting);
}

#[test]
fn a_waiting_item_that_cannot_be_added_leaves_file_and_ledger_as_they_were() {
    let (root, data) = dirs("no-waiting");
    let log = data.join("pool-edits.jsonl");
    std::fs::write(&log, "{\"old\":1}\n").unwrap();
    // `waiting/local.json` is a folder, so the item cannot be read or written.
    std::fs::create_dir_all(LocalItems::new(&data).path()).unwrap();
    let err = save_pool_edit(
        &FleetBridge::new(&root),
        &cap("deepseek", 7.0),
        &data,
        &FixedClock::at(5),
    )
    .unwrap_err();
    assert!(err.to_string().contains("local.json"), "{err}");
    assert_eq!(config_text(&root), CONFIG);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "{\"old\":1}\n");
}

#[test]
fn a_failed_reread_adds_no_waiting_item() {
    let (root, data) = dirs("reread");
    std::fs::remove_file(root.join("answers/budget.json")).unwrap();
    save_pool_edit(
        &FleetBridge::new(&root),
        &cap("deepseek", 7.0),
        &data,
        &FixedClock::at(5),
    )
    .unwrap_err();
    assert_eq!(config_text(&root), CONFIG);
    assert!(!data.join("pool-edits.jsonl").exists());
    assert!(!LocalItems::new(&data).path().exists());
}

#[test]
fn the_summary_states_each_new_value_and_the_old_one() {
    let (root, data) = dirs("summary");
    let saved = save_pool_edit(
        &FleetBridge::new(&root),
        &cap("deepseek", 7.5),
        &data,
        &FixedClock::at(5),
    )
    .unwrap();
    assert_eq!(
        edit_summary(&saved.ledger_row),
        "cap_usd is now 7.5 (was 10)"
    );
    let edit = PoolEdit {
        pool: "claude".into(),
        plan: Some("max-5x".into()),
        reset: Some("Sun 6:00am".into()),
        ..PoolEdit::default()
    };
    let saved = save_pool_edit(&FleetBridge::new(&root), &edit, &data, &FixedClock::at(6)).unwrap();
    assert_eq!(
        edit_summary(&saved.ledger_row),
        "plan is now max-5x (was max-1x), pace.reset_weekday is now sun (was not set), \
         pace.reset_hour is now 6 (was not set), pace.reset_minute is now 0 (was not set)"
    );
    assert_eq!(edit_summary(&json!({})), "nothing changed");
}

fn caller(face: Face, role: Option<Role>, in_job_worktree: bool) -> Caller {
    Caller {
        face,
        role,
        in_job_worktree,
        confirmed_in_app: false,
    }
}

#[test]
fn only_the_budget_manager_edits_a_pool_from_the_cli_or_mcp() {
    for face in [Face::Cli, Face::Mcp] {
        assert_eq!(
            guard_pool_edit(&caller(face, Some(Role::BudgetManager), false), "deepseek"),
            Verdict::Allow
        );
        for role in [None, Some(Role::ProjectManager), Some(Role::Satellite)] {
            assert_eq!(
                guard_pool_edit(&caller(face, role, false), "deepseek"),
                Verdict::Refuse(
                    "edit pool deepseek: only the budget-manager role may edit a pool".into()
                )
            );
        }
    }
    assert_eq!(
        guard_pool_edit(&caller(Face::Gui, None, false), "deepseek"),
        Verdict::Allow
    );
}

#[test]
fn no_pool_edit_inside_a_job_worktree() {
    for face in [Face::Gui, Face::Cli, Face::Mcp] {
        assert_eq!(
            guard_pool_edit(&caller(face, Some(Role::BudgetManager), true), "deepseek"),
            Verdict::Refuse("edit pool deepseek: refused inside a job worktree".into())
        );
    }
}
