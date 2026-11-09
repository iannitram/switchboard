//! BUD-1 (#23) review probe (R28): behaviour the issue's brief states that the locked
//! `budget_rows.rs` does not cover. Explicit JSON nulls read as `None`, and a null
//! `stale` reads as `false`.

use serde_json::json;
use switchboard_core::budget::rows_from_json;

#[test]
fn explicit_nulls_read_as_none_and_a_null_stale_is_false() {
    let v = json!({
        "p": {
            "pct": 1.0,
            "expected": null,
            "delta": null,
            "reading_age_min": null,
            "stale": null,
            "verdict": null,
            "why": null
        }
    });
    let rows = rows_from_json(&v).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].pct, Some(1.0));
    assert_eq!(rows[0].pace, None);
    assert_eq!(rows[0].delta, None);
    assert_eq!(rows[0].reading_age_min, None);
    assert!(!rows[0].stale);
    assert_eq!(rows[0].verdict, "not recorded");
    assert_eq!(rows[0].why, "");
}
