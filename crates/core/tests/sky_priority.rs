//! Locked tests for #34 (SKY-9: read the priority copy and flag it stale). Written before the
//! build (R28); the builder never edits this file.

use switchboard_core::sky::priority::{age_days, is_stale, parse, PriorityCopy, STALE_DAYS};
use switchboard_core::CoreError;

const DAY: i64 = 86_400;
/// 2026-11-08T07:00:00Z
const NOV_8_7AM: i64 = 1_794_121_200;

fn copy_at(t: i64) -> PriorityCopy {
    PriorityCopy {
        updated_at: t,
        order: vec!["iannitram/switchboard".to_string()],
    }
}

fn is_parse_error(json: &str) -> bool {
    matches!(parse(json), Err(CoreError::Parse(_)))
}

#[test]
fn stale_days_is_seven() {
    assert_eq!(STALE_DAYS, 7);
}

#[test]
fn parses_the_time_and_the_order_as_given() {
    let copy = parse(
        r#"{"updated_at": "2026-11-08T07:00:00Z",
            "order": ["iannitram/switchboard", "iannitram/nc-outliner", "iannitram/wavcraft"]}"#,
    )
    .unwrap();
    assert_eq!(
        copy,
        PriorityCopy {
            updated_at: NOV_8_7AM,
            order: vec![
                "iannitram/switchboard".to_string(),
                "iannitram/nc-outliner".to_string(),
                "iannitram/wavcraft".to_string(),
            ],
        }
    );
}

#[test]
fn the_epoch_is_zero() {
    let copy = parse(r#"{"updated_at": "1970-01-01T00:00:00Z", "order": []}"#).unwrap();
    assert_eq!(copy.updated_at, 0);
    assert!(copy.order.is_empty());
}

#[test]
fn a_leap_day_counts() {
    let copy = parse(r#"{"updated_at": "2024-02-29T00:00:00Z", "order": []}"#).unwrap();
    assert_eq!(copy.updated_at, 1_709_164_800);
}

#[test]
fn an_offset_is_taken_away_to_give_utc() {
    // 4:45:30pm at UTC-5 is 9:45:30pm UTC.
    let copy = parse(r#"{"updated_at": "2026-11-01T16:45:30-05:00", "order": []}"#).unwrap();
    assert_eq!(copy.updated_at, 1_793_569_530);
    let plus = parse(r#"{"updated_at": "2026-11-08T09:00:00+02:00", "order": []}"#).unwrap();
    assert_eq!(plus.updated_at, NOV_8_7AM);
}

#[test]
fn fractional_seconds_are_dropped() {
    let copy = parse(r#"{"updated_at": "2026-11-08T07:00:00.250Z", "order": []}"#).unwrap();
    assert_eq!(copy.updated_at, NOV_8_7AM);
}

#[test]
fn bad_json_or_missing_or_wrong_fields_are_parse_errors() {
    assert!(is_parse_error("not json"));
    assert!(is_parse_error(r#"{"order": []}"#));
    assert!(is_parse_error(r#"{"updated_at": "2026-11-08T07:00:00Z"}"#));
    assert!(is_parse_error(r#"{"updated_at": 5, "order": []}"#));
    assert!(is_parse_error(
        r#"{"updated_at": "2026-11-08T07:00:00Z", "order": [1]}"#
    ));
}

#[test]
fn a_time_that_is_not_rfc_3339_is_a_parse_error() {
    for bad in [
        "2026-11-08",
        "2026-11-08 07:00:00Z",
        "2026-11-08T07:00:00",
        "2026-13-08T07:00:00Z",
        "2026-11-32T07:00:00Z",
        "2026-11-08T24:00:00Z",
        "2026-11-08T07:00Z",
    ] {
        let json = format!(r#"{{"updated_at": "{bad}", "order": []}}"#);
        assert!(is_parse_error(&json), "{bad} should not parse");
    }
}

#[test]
fn age_is_whole_days_rounded_down() {
    let copy = copy_at(NOV_8_7AM);
    assert_eq!(age_days(&copy, NOV_8_7AM), 0);
    assert_eq!(age_days(&copy, NOV_8_7AM + DAY - 1), 0);
    assert_eq!(age_days(&copy, NOV_8_7AM + DAY), 1);
    assert_eq!(age_days(&copy, NOV_8_7AM + 10 * DAY + 5), 10);
}

#[test]
fn a_copy_dated_in_the_future_is_zero_days_old() {
    let copy = copy_at(NOV_8_7AM);
    assert_eq!(age_days(&copy, NOV_8_7AM - 3 * DAY), 0);
    assert!(!is_stale(&copy, NOV_8_7AM - 3 * DAY));
}

#[test]
fn stale_only_when_more_than_seven_days_old() {
    let copy = copy_at(NOV_8_7AM);
    assert!(!is_stale(&copy, NOV_8_7AM + 7 * DAY));
    assert!(is_stale(&copy, NOV_8_7AM + 7 * DAY + 1));
    assert!(is_stale(&copy, NOV_8_7AM + 30 * DAY));
}
