//! BUD-1 (#23): pool rows from `fleet budget --json`. Locked tests (R28): the builder never
//! edits this file.

use serde_json::{json, Value};
use std::path::PathBuf;
use switchboard_core::budget::{read, rows_from_json, PoolRow};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::CoreError;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture() -> Value {
    let p = manifest().join("tests/fixtures/fleet-stub/answers/budget.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

const CHATGPT_WHY: &str = "chatgpt 98.6% vs pace 81.4% (delta +17.2) via csv; STALE: reading is 138 min old (limit 120): re-read before relying on push; window ends Mon Nov 9 3:15pm (from the reading)";

const LOGGING_FLEET: &str = r#"import pathlib, sys
root = pathlib.Path(__file__).resolve().parent.parent.parent
with open(root / "calls.log", "a", encoding="utf-8") as fh:
    fh.write(" ".join(sys.argv[1:]) + "\n")
answer = root / "answers" / "budget.json"
if not answer.exists():
    sys.stderr.write("stub: no answer for budget\n")
    sys.exit(2)
sys.stdout.write(answer.read_text(encoding="utf-8"))
"#;

/// A pipeline root whose fleet.py logs its arguments to calls.log and prints the budget fixture.
fn logging_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sb-bud1-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tools").join("fleet")).unwrap();
    std::fs::create_dir_all(root.join("answers")).unwrap();
    std::fs::write(
        root.join("tools").join("fleet").join("fleet.py"),
        LOGGING_FLEET,
    )
    .unwrap();
    root
}

#[test]
fn rows_are_sorted_by_pool_name() {
    let rows = rows_from_json(&fixture()).unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.pool.as_str()).collect();
    assert_eq!(names, ["chatgpt", "claude", "claudeapi"]);
}

#[test]
fn a_row_carries_pct_pace_delta_verdict_age_stale_and_why() {
    let rows = rows_from_json(&fixture()).unwrap();
    assert_eq!(
        rows[0],
        PoolRow {
            pool: "chatgpt".into(),
            pct: Some(98.6),
            pace: Some(81.4),
            delta: Some(17.2),
            verdict: "brake".into(),
            reading_age_min: Some(138),
            stale: true,
            why: CHATGPT_WHY.into(),
        }
    );
}

#[test]
fn a_row_under_pace_has_a_negative_delta_and_is_not_stale() {
    let rows = rows_from_json(&fixture()).unwrap();
    let claude = &rows[1];
    assert_eq!(claude.pct, Some(2.0));
    assert_eq!(claude.pace, Some(3.0));
    assert_eq!(claude.delta, Some(-1.0));
    assert_eq!(claude.verdict, "on-pace");
    assert_eq!(claude.reading_age_min, Some(65));
    assert!(!claude.stale);
}

#[test]
fn pace_comes_from_expected_not_pace_5h() {
    let rows = rows_from_json(&fixture()).unwrap();
    assert_eq!(rows[2].pool, "claudeapi");
    assert_eq!(rows[2].pace, Some(74.04));
    assert_eq!(rows[2].pct, Some(71.78));
    assert_eq!(rows[2].delta, Some(-2.26));
}

#[test]
fn null_or_missing_numbers_are_none() {
    let v = json!({"qwen": {"verdict": "on-pace", "pct": null, "why": "no reading"}});
    let rows = rows_from_json(&v).unwrap();
    assert_eq!(
        rows,
        vec![PoolRow {
            pool: "qwen".into(),
            pct: None,
            pace: None,
            delta: None,
            verdict: "on-pace".into(),
            reading_age_min: None,
            stale: false,
            why: "no reading".into(),
        }]
    );
}

#[test]
fn a_missing_verdict_reads_not_recorded_and_a_missing_why_is_empty() {
    let rows = rows_from_json(&json!({"gemini": {"pct": 10.0}})).unwrap();
    assert_eq!(rows[0].verdict, "not recorded");
    assert_eq!(rows[0].why, "");
    assert!(!rows[0].stale);
}

#[test]
fn integer_numbers_read_as_floats_and_a_float_age_is_floored() {
    let v = json!({"x": {"pct": 50, "expected": 40, "delta": 10, "reading_age_min": 65.7, "verdict": "push"}});
    let rows = rows_from_json(&v).unwrap();
    assert_eq!(rows[0].pct, Some(50.0));
    assert_eq!(rows[0].pace, Some(40.0));
    assert_eq!(rows[0].delta, Some(10.0));
    assert_eq!(rows[0].reading_age_min, Some(65));
}

#[test]
fn an_empty_object_gives_no_rows() {
    assert_eq!(rows_from_json(&json!({})).unwrap(), Vec::<PoolRow>::new());
}

#[test]
fn a_top_level_that_is_not_an_object_is_a_parse_error() {
    assert_eq!(
        rows_from_json(&json!([1, 2])),
        Err(CoreError::Parse(
            "budget: expected an object keyed by pool".into()
        ))
    );
}

#[test]
fn a_pool_that_is_not_an_object_is_a_parse_error() {
    assert_eq!(
        rows_from_json(&json!({"claude": 5})),
        Err(CoreError::Parse(
            "budget: pool claude is not an object".into()
        ))
    );
}

#[test]
fn the_input_is_not_changed() {
    let v = fixture();
    let before = v.clone();
    let _ = rows_from_json(&v).unwrap();
    assert_eq!(v, before);
}

#[test]
fn a_row_serializes_with_its_field_names() {
    let rows = rows_from_json(&fixture()).unwrap();
    let v = serde_json::to_value(&rows[0]).unwrap();
    assert_eq!(
        v,
        json!({
            "pool": "chatgpt", "pct": 98.6, "pace": 81.4, "delta": 17.2, "verdict": "brake",
            "reading_age_min": 138, "stale": true, "why": CHATGPT_WHY
        })
    );
}

#[test]
fn read_runs_budget_json_and_parses_it() {
    let root = logging_root("read");
    std::fs::copy(
        manifest().join("tests/fixtures/fleet-stub/answers/budget.json"),
        root.join("answers").join("budget.json"),
    )
    .unwrap();
    let rows = read(&FleetBridge::new(&root)).unwrap();
    assert_eq!(rows, rows_from_json(&fixture()).unwrap());
    let calls = std::fs::read_to_string(root.join("calls.log")).unwrap();
    assert_eq!(calls.lines().collect::<Vec<_>>(), ["budget --json"]);
}

#[test]
fn read_passes_a_fleet_failure_through() {
    let root = logging_root("fail");
    match read(&FleetBridge::new(&root)) {
        Err(CoreError::Fleet(e)) => {
            assert_eq!(e.exit_code, Some(2));
            assert!(e.stderr.contains("no answer for budget"));
        }
        other => panic!("expected a fleet error, got {other:?}"),
    }
}

#[test]
fn the_code_map_marks_the_rows_present() {
    let map = std::fs::read_to_string(manifest().join("../../docs/code-map.md")).unwrap();
    assert!(map.contains("- BUD-1 rows (`rows_from_json`, `read`): present."));
}
