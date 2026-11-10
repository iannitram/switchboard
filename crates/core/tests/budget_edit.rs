//! BUD-2 (#25): save a pool edit to config.json with a ledger row and a Waiting on you item.
//! Locked tests (R28): the builder never edits this file.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use switchboard_core::budget::{config_path, parse_reset, rows_from_json, save_edit, PoolEdit};
use switchboard_core::clock::FixedClock;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::waiting::Source;
use switchboard_core::CoreError;

const NOW: i64 = 1_794_146_400;

const CONFIG: &str = r#"{
  "version": 1,
  "pools": {
    "claude": {
      "label": "Claude",
      "kind": "weekly",
      "pace": {
        "reset_weekday": "sun",
        "reset_hour": 6,
        "day_pct": {
          "mon": 18
        }
      }
    },
    "deepseek": {
      "label": "DeepSeek",
      "kind": "fixed-period",
      "cap_usd": 10,
      "note": "keep me"
    }
  },
  "lanes": {
    "claude": {
      "pool": "claude"
    }
  }
}
"#;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn budget_fixture() -> Value {
    let p = manifest().join("tests/fixtures/fleet-stub/answers/budget.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// A pipeline root with the fleet stub, its budget answer and CONFIG as tools/fleet/config.json.
fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sb-bud2-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let fleet = root.join("tools").join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::create_dir_all(root.join("answers")).unwrap();
    let stub = manifest().join("tests/fixtures/fleet-stub");
    std::fs::copy(stub.join("tools/fleet/fleet.py"), fleet.join("fleet.py")).unwrap();
    std::fs::copy(
        stub.join("answers/budget.json"),
        root.join("answers").join("budget.json"),
    )
    .unwrap();
    std::fs::write(fleet.join("config.json"), CONFIG).unwrap();
    root
}

fn config(root: &Path) -> Value {
    let p = root.join("tools").join("fleet").join("config.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

fn cap(pool: &str, cap: f64) -> PoolEdit {
    PoolEdit {
        pool: pool.into(),
        cap: Some(cap),
        ..PoolEdit::default()
    }
}

#[test]
fn reset_parses_day_and_twelve_hour_time() {
    assert_eq!(
        parse_reset("Sun 6:00am").unwrap(),
        ("sun".to_string(), 6, 0)
    );
    assert_eq!(
        parse_reset("Mon 3:16pm").unwrap(),
        ("mon".to_string(), 15, 16)
    );
    assert_eq!(parse_reset("fri 12am").unwrap(), ("fri".to_string(), 0, 0));
    assert_eq!(
        parse_reset("Sat 12:30pm").unwrap(),
        ("sat".to_string(), 12, 30)
    );
    assert_eq!(parse_reset(" WED 7AM ").unwrap(), ("wed".to_string(), 7, 0));
}

#[test]
fn a_bad_reset_is_a_parse_error() {
    for bad in [
        "Sun 13:00pm",
        "Sunday 6am",
        "Sun 6:00",
        "Sun 6:7am",
        "Sun 0am",
        "6am",
    ] {
        assert_eq!(
            parse_reset(bad),
            Err(CoreError::Parse(format!(
                "reset must look like Sun 6:00am, got {bad}"
            ))),
            "{bad}"
        );
    }
}

#[test]
fn config_path_is_next_to_fleet_py() {
    let r = root("path");
    assert_eq!(
        config_path(&FleetBridge::new(&r)),
        r.join("tools").join("fleet").join("config.json")
    );
}

#[test]
fn a_cap_edit_writes_cap_usd_and_keeps_everything_else_in_order() {
    let r = root("cap");
    let log = r.join("app").join("pool-edits.jsonl");
    save_edit(
        &FleetBridge::new(&r),
        &cap("deepseek", 25.0),
        &log,
        &FixedClock::at(NOW),
    )
    .unwrap();
    let c = config(&r);
    assert_eq!(c["pools"]["deepseek"]["cap_usd"].as_f64(), Some(25.0));
    assert_eq!(c["pools"]["deepseek"]["note"], "keep me");
    assert_eq!(keys(&c), ["version", "pools", "lanes"]);
    assert_eq!(keys(&c["pools"]), ["claude", "deepseek"]);
    assert_eq!(
        keys(&c["pools"]["deepseek"]),
        ["label", "kind", "cap_usd", "note"]
    );
    let mut want: Value = serde_json::from_str(CONFIG).unwrap();
    want["pools"]["deepseek"]["cap_usd"] = json!(25.0);
    assert_eq!(c, want);
    let text = std::fs::read_to_string(r.join("tools/fleet/config.json")).unwrap();
    assert!(text.ends_with("}\n"));
    assert!(text.contains("\n  \"pools\": {\n"), "two-space pretty JSON");
}

#[test]
fn a_reset_edit_writes_weekday_hour_and_minute() {
    let r = root("reset");
    let edit = PoolEdit {
        pool: "claude".into(),
        reset: Some("Mon 3:16pm".into()),
        ..PoolEdit::default()
    };
    let log = r.join("pool-edits.jsonl");
    save_edit(&FleetBridge::new(&r), &edit, &log, &FixedClock::at(NOW)).unwrap();
    let pace = &config(&r)["pools"]["claude"]["pace"];
    assert_eq!(pace["reset_weekday"], "mon");
    assert_eq!(pace["reset_hour"], 15);
    assert_eq!(pace["reset_minute"], 16);
    assert_eq!(pace["day_pct"], json!({"mon": 18}));
}

#[test]
fn a_reset_on_a_pool_with_no_pace_creates_it() {
    let r = root("newpace");
    let edit = PoolEdit {
        pool: "deepseek".into(),
        reset: Some("Sun 6:00am".into()),
        ..PoolEdit::default()
    };
    save_edit(
        &FleetBridge::new(&r),
        &edit,
        &r.join("e.jsonl"),
        &FixedClock::at(NOW),
    )
    .unwrap();
    assert_eq!(
        config(&r)["pools"]["deepseek"]["pace"],
        json!({"reset_weekday": "sun", "reset_hour": 6, "reset_minute": 0})
    );
}

#[test]
fn plan_and_cost_write_their_own_keys() {
    let r = root("plan");
    let edit = PoolEdit {
        pool: "claude".into(),
        plan: Some("Max 20x".into()),
        cost_usd: Some(200.0),
        ..PoolEdit::default()
    };
    save_edit(
        &FleetBridge::new(&r),
        &edit,
        &r.join("e.jsonl"),
        &FixedClock::at(NOW),
    )
    .unwrap();
    let claude = &config(&r)["pools"]["claude"];
    assert_eq!(claude["plan"], "Max 20x");
    assert_eq!(claude["cost_usd"].as_f64(), Some(200.0));
}

#[test]
fn each_save_appends_one_ledger_row_with_before_and_after() {
    let r = root("ledger");
    let log = r.join("deep").join("er").join("pool-edits.jsonl");
    let fleet = FleetBridge::new(&r);
    let saved = save_edit(&fleet, &cap("deepseek", 25.0), &log, &FixedClock::at(NOW)).unwrap();
    let want =
        json!({"at": NOW, "pool": "deepseek", "set": {"cap_usd": 25.0}, "before": {"cap_usd": 10}});
    assert_eq!(saved.ledger_row, want);
    save_edit(
        &fleet,
        &cap("deepseek", 5.0),
        &log,
        &FixedClock::at(NOW + 60),
    )
    .unwrap();
    let text = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], want);
    assert_eq!(
        lines[1],
        json!({"at": NOW + 60, "pool": "deepseek", "set": {"cap_usd": 5.0}, "before": {"cap_usd": 25.0}})
    );
}

#[test]
fn a_reset_ledger_row_names_the_pace_keys() {
    let r = root("ledgerreset");
    let edit = PoolEdit {
        pool: "claude".into(),
        reset: Some("Mon 3:16pm".into()),
        ..PoolEdit::default()
    };
    let saved = save_edit(
        &FleetBridge::new(&r),
        &edit,
        &r.join("e.jsonl"),
        &FixedClock::at(NOW),
    )
    .unwrap();
    assert_eq!(
        saved.ledger_row["set"],
        json!({"pace.reset_weekday": "mon", "pace.reset_hour": 15, "pace.reset_minute": 16})
    );
    assert_eq!(
        saved.ledger_row["before"],
        json!({"pace.reset_weekday": "sun", "pace.reset_hour": 6, "pace.reset_minute": null})
    );
}

#[test]
fn the_save_puts_a_pool_edit_item_on_waiting() {
    let r = root("waiting");
    let fleet = FleetBridge::new(&r);
    let saved = save_edit(
        &fleet,
        &cap("deepseek", 25.0),
        &r.join("e.jsonl"),
        &FixedClock::at(NOW),
    )
    .unwrap();
    let item = saved.waiting;
    assert_eq!(item.what, "Land the deepseek pool edit");
    assert_eq!(
        item.why,
        "A pool edit is saved in the local pipeline clone only; it lands through review (U5)."
    );
    assert_eq!(
        item.source,
        Source::PoolEdit {
            pool: "deepseek".into()
        }
    );
    assert_eq!(item.link, config_path(&fleet).display().to_string());
    assert_eq!(item.action, "Open config.json");
    assert_eq!(item.since, NOW);
}

#[test]
fn the_save_re_reads_the_budget() {
    let r = root("reread");
    let saved = save_edit(
        &FleetBridge::new(&r),
        &cap("deepseek", 25.0),
        &r.join("e.jsonl"),
        &FixedClock::at(NOW),
    )
    .unwrap();
    assert_eq!(saved.rows, rows_from_json(&budget_fixture()).unwrap());
}

fn assert_refused(name: &str, edit: PoolEdit, why: &str) {
    let r = root(name);
    let log = r.join("e.jsonl");
    let got = save_edit(&FleetBridge::new(&r), &edit, &log, &FixedClock::at(NOW));
    assert_eq!(got.err(), Some(CoreError::Parse(why.into())));
    let text = std::fs::read_to_string(r.join("tools/fleet/config.json")).unwrap();
    assert_eq!(text, CONFIG, "config.json untouched");
    assert!(!log.exists(), "no ledger row");
}

#[test]
fn an_edit_that_changes_nothing_is_refused() {
    let edit = PoolEdit {
        pool: "deepseek".into(),
        ..PoolEdit::default()
    };
    assert_refused("nothing", edit, "pool edit for deepseek changes nothing");
}

#[test]
fn an_unknown_pool_is_refused() {
    assert_refused(
        "unknown",
        cap("gemini", 5.0),
        "no pool gemini in config.json",
    );
}

#[test]
fn a_negative_cap_or_cost_is_refused() {
    assert_refused(
        "negcap",
        cap("deepseek", -1.0),
        "cap must be a number 0 or more",
    );
    let edit = PoolEdit {
        pool: "deepseek".into(),
        cost_usd: Some(f64::NAN),
        ..PoolEdit::default()
    };
    assert_refused("nancost", edit, "cost_usd must be a number 0 or more");
}

#[test]
fn a_bad_reset_is_refused_before_anything_is_written() {
    let edit = PoolEdit {
        pool: "claude".into(),
        reset: Some("Sunday".into()),
        ..PoolEdit::default()
    };
    assert_refused(
        "badreset",
        edit,
        "reset must look like Sun 6:00am, got Sunday",
    );
}

#[test]
fn the_edit_is_not_changed() {
    let r = root("frozen");
    let edit = cap("deepseek", 25.0);
    let before = edit.clone();
    save_edit(
        &FleetBridge::new(&r),
        &edit,
        &r.join("e.jsonl"),
        &FixedClock::at(NOW),
    )
    .unwrap();
    assert_eq!(edit, before);
}

#[test]
fn the_code_map_marks_the_save_present() {
    let map = std::fs::read_to_string(manifest().join("../../docs/code-map.md")).unwrap();
    assert!(map.contains("- BUD-2 save (`save_edit`, `parse_reset`, `budget_save_edit`): present."));
}
