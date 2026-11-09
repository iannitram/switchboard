//! AUTO-5: `weekly_due` is true once the newest Sunday 6am (local) has passed and is later than
//! `last_run`. Dates are built from civil days, so no clock or file is read.

use switchboard_core::autopilot::weekly::weekly_due;
use switchboard_core::clock::UnixSeconds;

const DAY: i64 = 86_400;
const WEEK: i64 = 7 * DAY;

/// Days from 1970-01-01 to the given civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The UTC instant of a civil date and time.
fn utc(y: i64, m: i64, d: i64, h: i64, min: i64, s: i64) -> UnixSeconds {
    days_from_civil(y, m, d) * DAY + h * 3_600 + min * 60 + s
}

// 2026-11-08 is a Sunday; 2026-11-01 is the Sunday before it.
const PACIFIC: i64 = -7 * 3_600;
const INDIA: i64 = 5 * 3_600 + 30 * 60;

#[test]
fn due_at_the_reset_moment_and_not_one_second_before() {
    // UTC: the Sunday 6am reset is 06:00:00Z.
    let reset = utc(2026, 11, 8, 6, 0, 0);
    let last = reset - WEEK;
    assert!(weekly_due(reset, 0, Some(last)));
    assert!(!weekly_due(reset - 1, 0, Some(last)));
}

#[test]
fn pacific_offset_moves_the_reset_to_1300_utc() {
    // Sunday 06:00 local at UTC-7 is 13:00Z; the previous reset is a week earlier.
    let last = utc(2026, 11, 1, 13, 0, 0);
    let before = utc(2026, 11, 8, 12, 59, 59);
    assert!(!weekly_due(before, PACIFIC, Some(last)));
    assert!(weekly_due(before + 1, PACIFIC, Some(last)));
}

#[test]
fn last_run_at_or_after_the_newest_reset_is_not_due() {
    let reset = utc(2026, 11, 8, 6, 0, 0);
    let now = reset + 1_000;
    assert!(!weekly_due(now, 0, Some(reset + 10)));
    assert!(!weekly_due(now, 0, Some(reset)));
}

#[test]
fn no_last_run_is_due() {
    let reset = utc(2026, 11, 8, 6, 0, 0);
    assert!(weekly_due(reset, 0, None));
    assert!(weekly_due(reset + 5 * DAY, PACIFIC, None));
}

#[test]
fn positive_offset_before_and_after_a_monday_to_saturday_week() {
    // UTC+5:30: Sunday 06:00 local is 00:30Z.
    let reset = utc(2026, 11, 8, 0, 30, 0);
    let last_before = reset - 1;
    let last_at = reset;

    // Wednesday, 2026-11-11 12:00Z.
    let wed = utc(2026, 11, 11, 12, 0, 0);
    assert!(weekly_due(wed, INDIA, Some(last_before)));
    assert!(!weekly_due(wed, INDIA, Some(last_at)));

    // Monday, 2026-11-09 12:00Z (17:30 local).
    let mon = utc(2026, 11, 9, 12, 0, 0);
    assert!(weekly_due(mon, INDIA, Some(last_before)));
    assert!(!weekly_due(mon, INDIA, Some(last_at)));

    // Saturday, 2026-11-14 12:00Z (17:30 local), still the week of the 8th's reset.
    let sat = utc(2026, 11, 14, 12, 0, 0);
    assert!(weekly_due(sat, INDIA, Some(last_before)));
    assert!(!weekly_due(sat, INDIA, Some(last_at)));
}
