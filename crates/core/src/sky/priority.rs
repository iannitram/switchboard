//! SKY-9: the priority copy. The priority board lives in a claude.ai Artifact database a Claude
//! session can read; until U4 is answered, a copy of the order sits in fleet-state, refreshed by
//! a Claude session at the Sunday drift clear and whenever I re-rank.
//!
//! The copy: JSON `{"updated_at": "<RFC 3339>", "order": ["owner/repo", ...]}`.

use crate::clock::UnixSeconds;
use crate::CoreError;

/// Older than this and the copy becomes a Waiting on you item.
pub const STALE_DAYS: i64 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityCopy {
    pub updated_at: UnixSeconds,
    pub order: Vec<String>,
}

/// Read the copy. Bad JSON, a missing or mistyped field, or a time that is not RFC 3339 is a
/// `CoreError::Parse`.
pub fn parse(json: &str) -> Result<PriorityCopy, CoreError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| CoreError::Parse(format!("priority copy: {e}")))?;
    let time = value
        .get("updated_at")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::Parse("priority copy: updated_at must be a string".into()))?;
    let order = value
        .get("order")
        .and_then(|v| v.as_array())
        .ok_or_else(|| CoreError::Parse("priority copy: order must be an array".into()))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| CoreError::Parse("priority copy: order must hold strings".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PriorityCopy {
        updated_at: parse_rfc3339(time)?,
        order,
    })
}

/// Age in whole days at `now`.
pub fn age_days(copy: &PriorityCopy, now: UnixSeconds) -> i64 {
    (now - copy.updated_at).max(0).div_euclid(86_400)
}

/// SKY-9 (#345): the line `sky` prints for the copy's age in days (`None` without a copy):
/// `priority copy: <n> days old`, or `priority copy: none`.
pub fn age_line(age_days: Option<i64>) -> String {
    match age_days {
        Some(1) => "priority copy: 1 day old\n".to_string(),
        Some(days) => format!("priority copy: {days} days old\n"),
        None => "priority copy: none\n".to_string(),
    }
}

pub fn is_stale(copy: &PriorityCopy, now: UnixSeconds) -> bool {
    now - copy.updated_at > STALE_DAYS * 86_400
}

/// `YYYY-MM-DDTHH:MM:SS`, optional `.digits` (dropped), then `Z` or `+HH:MM` / `-HH:MM`.
/// Returns Unix seconds in UTC.
fn parse_rfc3339(s: &str) -> Result<UnixSeconds, CoreError> {
    let bad = || CoreError::Parse(format!("priority copy: updated_at is not RFC 3339: {s}"));
    let b = s.as_bytes();
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return Err(bad());
    }
    let (Some(y), Some(m), Some(d), Some(h), Some(mi), Some(sec)) = (
        digits(&b[0..4]),
        digits(&b[5..7]),
        digits(&b[8..10]),
        digits(&b[11..13]),
        digits(&b[14..16]),
        digits(&b[17..19]),
    ) else {
        return Err(bad());
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 59 {
        return Err(bad());
    }

    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return Err(bad());
        }
    }

    // The offset in seconds east of UTC; the local time minus it is UTC.
    let offset: i64 = match &b[i..] {
        b"Z" => 0,
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2] => {
            match (digits(&[*h1, *h2]), digits(&[*m1, *m2])) {
                (Some(oh), Some(om)) if oh <= 23 && om <= 59 => {
                    let secs = oh * 3_600 + om * 60;
                    if *sign == b'+' {
                        secs
                    } else {
                        -secs
                    }
                }
                _ => return Err(bad()),
            }
        }
        _ => return Err(bad()),
    };

    let local = days_from_civil(y, m, d) * 86_400 + h * 3_600 + mi * 60 + sec;
    Ok(local - offset)
}

/// All-ASCII-digit bytes as a number; `None` for anything else.
fn digits(b: &[u8]) -> Option<i64> {
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(b.iter().fold(0, |acc, d| acc * 10 + i64::from(d - b'0')))
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
