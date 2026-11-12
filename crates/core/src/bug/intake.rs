//! Bug intake (bug-intake, #408): `bug scan`, `capture`, `validate`, `owner`, `dedupe`, `record`
//! and `archive`. Each step is a plain function over files and rows the caller names; none of
//! them starts a process or touches GitHub.

use super::{Bug, OwnerRow};
use crate::lock::{self, FileLock};
use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

const SEVERITIES: [&str; 5] = ["data-loss", "privacy", "crash", "wrong-result", "cosmetic"];
const CLASSES: [&str; 4] = ["bug", "test-drop", "question", "empty"];
const SOURCES: [&str; 2] = ["drop", "issue"];

fn io(path: &Path, e: std::io::Error) -> CoreError {
    CoreError::Io(format!("{}: {e}", path.display()))
}

// ---- sha256 ------------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// The SHA-256 of `data` as 64 lowercase hex digits.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

fn sha256_file(path: &Path) -> Result<String, CoreError> {
    Ok(sha256_hex(&std::fs::read(path).map_err(|e| io(path, e))?))
}

// ---- scan --------------------------------------------------------------------------------

/// A report waiting in the inbox: its id is the sha256 of the original.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Found {
    pub id: String,
    pub path: PathBuf,
}

/// `bug scan`: every `.zip` at the top level of `inbox` and one folder down, never under
/// `_archive`, whose id no bug row already holds. Sorted by path.
pub fn scan(inbox: &Path, bugs: &[Bug]) -> Result<Vec<Found>, CoreError> {
    fn zips(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) -> Result<(), CoreError> {
        for entry in std::fs::read_dir(dir).map_err(|e| io(dir, e))? {
            let path = entry.map_err(|e| io(dir, e))?.path();
            if path.is_dir() {
                if depth == 0 && path.file_name().is_some_and(|n| n != "_archive") {
                    zips(&path, 1, out)?;
                }
            } else if path.extension().is_some_and(|x| x == "zip") {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    zips(inbox, 0, &mut paths)?;
    paths.sort();
    let mut found = Vec::new();
    for path in paths {
        let id = sha256_file(&path)?;
        if !bugs.iter().any(|b| b.reports.contains(&id)) {
            found.push(Found { id, path });
        }
    }
    Ok(found)
}

// ---- capture -----------------------------------------------------------------------------

/// `bug capture`: copy `original` into `<raw_root>/<date>-<id>/` with a `SHA256SUMS` file, and
/// answer that folder. The copy is read back and checked before it is answered.
pub fn capture(original: &Path, raw_root: &Path, date: &str) -> Result<PathBuf, CoreError> {
    let bytes = std::fs::read(original).map_err(|e| io(original, e))?;
    let id = sha256_hex(&bytes);
    let name = original
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| CoreError::Refused(format!("{} has no file name", original.display())))?;
    let dir = raw_root.join(format!("{date}-{id}"));
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    let copy = dir.join(name);
    lock::write_whole(&copy, &bytes)?;
    if sha256_file(&copy)? != id {
        return Err(CoreError::Refused(format!(
            "raw copy of {name} does not match"
        )));
    }
    lock::write_whole(
        &dir.join("SHA256SUMS"),
        format!("{id}  {name}\n").as_bytes(),
    )?;
    Ok(dir)
}

// ---- validate ----------------------------------------------------------------------------

/// The report form (specs/bug-loop.json, record `report`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub id: String,
    pub source: String,
    pub app: String,
    #[serde(default)]
    pub version: Option<String>,
    pub report_class: String,
    pub symptom: String,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub error_signature: Option<String>,
    pub severity: String,
    pub raw_copy: String,
}

/// The private word `text` contains, matched whole-word and ignoring case.
fn private_word<'a>(text: &str, words: &'a [String]) -> Option<&'a str> {
    let lower = text.to_lowercase();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    words
        .iter()
        .map(String::as_str)
        .filter(|w| !w.is_empty())
        .find(|w| {
            let w = w.to_lowercase();
            lower.match_indices(&w).any(|(at, _)| {
                !lower[..at].chars().next_back().is_some_and(is_word)
                    && !lower[at + w.len()..].chars().next().is_some_and(is_word)
            })
        })
}

/// Refuse `text` if it holds a private word. The refusal does not repeat the word.
pub fn scan_private(text: &str, words: &[String]) -> Result<(), CoreError> {
    match private_word(text, words) {
        Some(_) => Err(CoreError::Refused(
            "the report contains a private word; nothing was written".into(),
        )),
        None => Ok(()),
    }
}

/// `bug validate`: the private-word scan of the whole text first, then the report form: every
/// required field present and non-empty, and `source`, `report_class` and `severity` one of
/// their listed values.
pub fn validate(text: &str, private_words: &[String]) -> Result<Report, CoreError> {
    scan_private(text, private_words)?;
    let report: Report =
        serde_json::from_str(text).map_err(|e| CoreError::Parse(format!("report: {e}")))?;
    let bad = |field: &str| CoreError::Refused(format!("report field `{field}` is not valid"));
    for (field, value) in [
        ("id", &report.id),
        ("app", &report.app),
        ("symptom", &report.symptom),
        ("raw_copy", &report.raw_copy),
    ] {
        if value.trim().is_empty() {
            return Err(bad(field));
        }
    }
    for (field, value, allowed) in [
        ("source", &report.source, &SOURCES[..]),
        ("report_class", &report.report_class, &CLASSES[..]),
        ("severity", &report.severity, &SEVERITIES[..]),
    ] {
        if !allowed.contains(&value.as_str()) {
            return Err(bad(field));
        }
    }
    Ok(report)
}

// ---- owner -------------------------------------------------------------------------------

/// `bug owner`: the owner row for `app`. No row is refused with the app named.
pub fn owner<'a>(owners: &'a [OwnerRow], app: &str) -> Result<&'a OwnerRow, CoreError> {
    owners
        .iter()
        .find(|o| o.app == app)
        .ok_or_else(|| CoreError::Refused(format!("no owner row for app `{app}`")))
}

// ---- dedupe ------------------------------------------------------------------------------

/// A signature with case and runs of whitespace flattened, so a reflowed log line matches.
pub fn normalise_signature(sig: &str) -> String {
    sig.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `bug dedupe`: the open bug in `repo` whose signature equals `signature`, if any. A report
/// with no signature never matches.
pub fn dedupe<'a>(bugs: &'a [Bug], repo: &str, signature: Option<&str>) -> Option<&'a Bug> {
    let want = normalise_signature(signature?);
    if want.is_empty() {
        return None;
    }
    bugs.iter().find(|b| {
        b.is_open()
            && b.repo == repo
            && b.error_signature
                .as_deref()
                .map(normalise_signature)
                .as_deref()
                == Some(&want)
    })
}

// ---- record ------------------------------------------------------------------------------

/// The scrubbed issue text for a report: a fixed template over its fields. The private-word scan
/// runs first, so a refused report yields no text.
pub fn issue_text(
    report: &Report,
    private_words: &[String],
) -> Result<(String, String), CoreError> {
    let whole = serde_json::to_string(report).unwrap_or_default();
    scan_private(&whole, private_words)?;
    let mut body = format!(
        "**Symptom:** {}\n\n**Severity:** {}\n",
        report.symptom, report.severity
    );
    if let Some(v) = &report.version {
        body.push_str(&format!("**Version:** {v}\n"));
    }
    if !report.steps.is_empty() {
        body.push_str("\n**Steps:**\n");
        for (i, s) in report.steps.iter().enumerate() {
            body.push_str(&format!("{}. {s}\n", i + 1));
        }
    }
    if let Some(sig) = &report.error_signature {
        body.push_str(&format!("\n**Error:** `{sig}`\n"));
    }
    Ok((report.symptom.clone(), body))
}

/// Read `bugs.jsonl`; a missing file is no rows, a line that does not parse is an error.
pub fn read_bugs(ledger: &Path) -> Result<Vec<Bug>, CoreError> {
    let text = match std::fs::read_to_string(ledger) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io(ledger, e)),
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| CoreError::Parse(format!("bugs.jsonl: {e}"))))
        .collect()
}

/// Read, change and write the ledger whole under its lock.
fn edit_ledger<T>(
    ledger: &Path,
    change: impl FnOnce(&mut Vec<Bug>) -> Result<T, CoreError>,
) -> Result<T, CoreError> {
    let _lock = FileLock::take(ledger, Duration::from_secs(10))?;
    let mut bugs = read_bugs(ledger)?;
    let out = change(&mut bugs)?;
    let mut text = String::new();
    for b in &bugs {
        text.push_str(&serde_json::to_string(b).map_err(|e| CoreError::Parse(e.to_string()))?);
        text.push('\n');
    }
    lock::write_whole(ledger, text.as_bytes())?;
    Ok(out)
}

/// `bug dedupe`, the link: add the report's id to the bug it repeats. Linking twice adds it once.
pub fn link_duplicate(ledger: &Path, bug_id: &str, report_id: &str) -> Result<(), CoreError> {
    edit_ledger(ledger, |bugs| {
        let bug = bugs
            .iter_mut()
            .find(|b| b.id == bug_id)
            .ok_or_else(|| CoreError::Refused(format!("no bug `{bug_id}`")))?;
        if !bug.reports.iter().any(|r| r == report_id) {
            bug.reports.push(report_id.to_string());
        }
        Ok(())
    })
}

/// `bug record`: write the bug row for a report filed as `repo` issue `issue`, status open. A
/// report already on a row answers that row and writes nothing.
pub fn record(ledger: &Path, report: &Report, repo: &str, issue: u64) -> Result<Bug, CoreError> {
    edit_ledger(ledger, |bugs| {
        if let Some(b) = bugs.iter().find(|b| b.reports.contains(&report.id)) {
            return Ok(b.clone());
        }
        let bug = Bug {
            id: format!("{}#{issue}", repo.rsplit('/').next().unwrap_or(repo)),
            repo: repo.to_string(),
            issue,
            reports: vec![report.id.clone()],
            error_signature: report.error_signature.clone(),
            severity: report.severity.clone(),
            status: "open".into(),
            attempts: 0,
        };
        bugs.push(bug.clone());
        Ok(bug)
    })
}

// ---- archive -----------------------------------------------------------------------------

/// `bug archive`: move `original` to `<archive_root>/<date>/`, but only when the raw copy in
/// `raw_copy` (same file name) has the same sha256. A mismatch or a missing copy is refused and
/// the original stays where it is. Answers the archived path.
pub fn archive(
    original: &Path,
    raw_copy: &Path,
    archive_root: &Path,
    date: &str,
) -> Result<PathBuf, CoreError> {
    let name = original
        .file_name()
        .ok_or_else(|| CoreError::Refused(format!("{} has no file name", original.display())))?;
    let copy = raw_copy.join(name);
    if sha256_file(original)? != sha256_file(&copy)? {
        return Err(CoreError::Refused(format!(
            "raw copy {} does not match the original; the original stays in the inbox",
            copy.display()
        )));
    }
    let dir = archive_root.join(date);
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    let to = dir.join(name);
    if lock::rename_retrying(original, &to).is_err() {
        std::fs::copy(original, &to).map_err(|e| io(&to, e))?;
        if sha256_file(&to)? != sha256_file(&copy)? {
            let _ = std::fs::remove_file(&to);
            return Err(CoreError::Refused("archived copy does not match".into()));
        }
        std::fs::remove_file(original).map_err(|e| io(original, e))?;
    }
    Ok(to)
}
