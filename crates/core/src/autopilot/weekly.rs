//! AUTO-2, AUTO-5: jobs the core starts on its own: a routed Project Manager job when a
//! ROADMAP changes, and the weekly drift clear and backup once Sunday's 6am reset has passed.

use crate::clock::UnixSeconds;

/// AUTO-5: true when the newest Sunday 6am (local, `utc_offset_secs`) at or before `now` is
/// later than `last_run`.
pub fn weekly_due(now: UnixSeconds, utc_offset_secs: i64, last_run: Option<UnixSeconds>) -> bool {
    const DAY: i64 = 86_400;
    const SIX_AM: i64 = 6 * 3_600;
    // Local wall-clock seconds since the epoch; days count from Thursday 1970-01-01.
    let local = now + utc_offset_secs;
    let day = local.div_euclid(DAY);
    // 0 is Sunday: 1970-01-01 was a Thursday (4).
    let days_since_sunday = (day + 4).rem_euclid(7);
    let mut sunday = day - days_since_sunday;
    if sunday * DAY + SIX_AM > local {
        // Today is Sunday before 6am: the newest reset is last Sunday's.
        sunday -= 7;
    }
    let reset = sunday * DAY + SIX_AM - utc_offset_secs;
    match last_run {
        None => true,
        Some(last) => last < reset,
    }
}

/// AUTO-2: the repos whose ROADMAP.md text changed between two snapshots (by repo name).
pub fn roadmap_changed(
    previous: &[(String, Option<String>)],
    next: &[(String, Option<String>)],
) -> Vec<String> {
    let mut seen: Vec<&str> = Vec::new();
    let mut changed = Vec::new();
    for (repo, text) in next {
        if seen.contains(&repo.as_str()) {
            continue;
        }
        seen.push(repo.as_str());
        let Some(text) = text else { continue };
        let before = previous
            .iter()
            .find(|(name, _)| name == repo)
            .and_then(|(_, t)| t.as_deref());
        if before != Some(text.as_str()) {
            changed.push(repo.clone());
        }
    }
    changed
}

/// AUTO-5: the UTC offset in seconds from `date +%z`'s output (`-0700`, `+0530`, surrounding
/// whitespace allowed). Anything else, or minutes over 59, is `None`.
pub fn parse_utc_offset(text: &str) -> Option<i64> {
    let (sign_text, rest) = text.trim().split_at_checked(1)?;
    let sign = match sign_text {
        "+" => 1,
        "-" => -1,
        _ => return None,
    };
    if rest.len() != 4 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = rest[..2].parse().ok()?;
    let minutes: i64 = rest[2..].parse().ok()?;
    if minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3_600 + minutes * 60))
}

/// AUTO-5: this computer's UTC offset in seconds: `date +%z` on Linux and macOS, PowerShell's
/// `zzz` on Windows. `None` when neither answers; the weekly run then waits.
pub fn local_utc_offset() -> Option<i64> {
    let out = if cfg!(windows) {
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-Date).ToString('zzz').Replace(':','')",
            ])
            .output()
    } else {
        std::process::Command::new("date").arg("+%z").output()
    }
    .ok()?;
    parse_utc_offset(&String::from_utf8_lossy(&out.stdout))
}
