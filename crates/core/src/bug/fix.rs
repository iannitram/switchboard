//! bug-fix (#409): `bug next`, `job`, `attempts`, `lesson-check`, `signature-check` and
//! `status-row`. `next` chooses the bug, `job` writes the repro job and then the fix job (only
//! after the repro test failed on main with the reported signature, and with the repro test
//! outside `files_allowed`), `attempts` stops a bug at the third try or a repeated diff and parks
//! it with one Waiting on you item, and the three checks judge the one pitfalls line, the search
//! patterns and the status row. No dollar cap is applied anywhere here: the budget gate is
//! another step's. The side effects (running the repro command, `git grep`, the waiting items)
//! sit behind small traits or take the caller's store; the rest is plain functions.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use super::{Bug, OwnerRow};
use crate::clock::UnixSeconds;
use crate::runner::spec::Spec;
use crate::runner::waiting::{RunnerItems, WaitingItem};
use crate::CoreError;

/// The exit code of `switchboard bug next` when no bug is ready: not now.
pub const NOT_NOW_EXIT: i32 = 75;

/// The third failed try stops the bug.
pub const MAX_ATTEMPTS: usize = 3;

/// A job's brief is at most this many bytes.
pub const BRIEF_LIMIT: usize = 16_000;

/// A pitfalls entry adds at most this many words.
pub const LESSON_WORDS: usize = 60;

/// The step of the bug-loop spec that asks the owner about a parked bug.
const PARK_STEP: &str = "owner-rules-on-parked";

const SEVERITY_ORDER: [&str; 5] = ["data-loss", "privacy", "crash", "wrong-result", "cosmetic"];

// ---- bug next ----------------------------------------------------------------------------

/// `bug next`: the open bug no worker holds that comes first by severity (data-loss, privacy,
/// crash, wrong-result, cosmetic), then by the most reports, then the oldest (the earliest row of
/// `bugs`). `claimed` are the (repo, issue) pairs workers hold. `None` is exit 75.
pub fn next<'a>(bugs: &'a [Bug], claimed: &[(&str, u64)]) -> Option<&'a Bug> {
    let rank = |bug: &Bug| {
        SEVERITY_ORDER
            .iter()
            .position(|s| *s == bug.severity)
            .unwrap_or(SEVERITY_ORDER.len())
    };
    bugs.iter()
        .enumerate()
        .filter(|(_, bug)| bug.is_open() && !claimed.contains(&(bug.repo.as_str(), bug.issue)))
        .min_by_key(|(row, bug)| (rank(bug), std::cmp::Reverse(bug.reports.len()), *row))
        .map(|(_, bug)| bug)
}

// ---- bug job -----------------------------------------------------------------------------

/// What a job's verify entry expects of its command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Expect {
    #[serde(rename = "pass")]
    Pass,
    #[serde(rename = "fail-before")]
    FailBefore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verify {
    pub name: String,
    pub cmd: String,
    pub expect: Expect,
}

/// One job of the bug-loop (specs/bug-loop.json, record `job`): the repro job or the fix job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Job {
    pub id: String,
    /// `repro` or `fix`.
    pub stage: String,
    pub lane: String,
    pub verify: Vec<Verify>,
    pub files_allowed: Vec<String>,
    pub open_pr: bool,
    pub brief: String,
}

/// The repro test a bug row holds once the repro job has passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repro {
    pub test: String,
    pub cmd: String,
}

/// The output of a command run on main.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub exit: i32,
    pub output: String,
}

/// Runs the repro command on main.
pub trait Main {
    fn run(&self, cmd: &str) -> Result<Run, CoreError>;
}

/// Runs the command with `sh -c` in a clone at main.
pub struct ShellMain {
    pub clone: PathBuf,
}

impl Main for ShellMain {
    fn run(&self, cmd: &str) -> Result<Run, CoreError> {
        let out = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(&self.clone)
            .output()
            .map_err(|e| CoreError::Io(format!("{cmd}: {e}")))?;
        let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
        output.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok(Run {
            exit: out.status.code().unwrap_or(-1),
            output,
        })
    }
}

/// Why `job` wrote nothing.
#[derive(Debug)]
pub enum Refusal {
    /// The bug carries no error signature, so no failure can be the reported one.
    NoSignature,
    /// The owner row names no test command.
    NoTestCmd,
    /// The repro command passed on main, or failed without the reported signature.
    NotRed {
        exit: i32,
        signature: String,
    },
    /// No file for the fix to change was named.
    NoScope,
    /// A `files_allowed` glob of the fix would let it edit the repro test.
    TouchesRepro(String),
    /// Scope patterns support only `/` separators and `*` or `**` wildcards.
    UnsupportedGlob(String),
    /// The brief is over `BRIEF_LIMIT` bytes.
    BriefTooLong(usize),
    Run(CoreError),
}

/// What `job` reads: the bug, its owner row, the lane, the stored repro test (none: write the
/// repro job) and, for the fix job, the globs the fix may change.
pub struct Request<'a> {
    pub bug: &'a Bug,
    pub owner: &'a OwnerRow,
    pub lane: &'a str,
    pub repro: Option<&'a Repro>,
    pub scope: &'a [String],
}

fn slug(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Where the repro job writes its one test: recursive directories use zero levels, and each
/// remaining wildcard in the owner's glob becomes `bug_<id>`.
pub fn repro_path(owner: &OwnerRow, bug: &Bug) -> String {
    let name = format!("bug_{}", slug(&bug.id));
    if owner.test_dir.contains('*') {
        owner
            .test_dir
            .replace("**/", "")
            .replace("**", "*")
            .replace('*', &name)
    } else {
        format!("{}/{name}", owner.test_dir.trim_end_matches('/'))
    }
}

/// `*` matches within one path part, `**` across parts.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p {
            [] => s.is_empty(),
            [b'*', b'*', rest @ ..] => {
                (0..=s.len()).any(|i| go(rest, &s[i..]))
                    || matches!(rest, [b'/', tail @ ..] if go(tail, s))
            }
            [b'*', rest @ ..] => {
                for i in 0..=s.len() {
                    if go(rest, &s[i..]) {
                        return true;
                    }
                    if s.get(i) == Some(&b'/') || i == s.len() {
                        return false;
                    }
                }
                false
            }
            [c, rest @ ..] => s.first() == Some(c) && go(rest, &s[1..]),
        }
    }
    go(pattern.as_bytes(), path.as_bytes())
}

fn check(name: &str, cmd: &str, expect: Expect) -> Verify {
    Verify {
        name: name.to_string(),
        cmd: cmd.to_string(),
        expect,
    }
}

/// `bug job`: the repro job when the bug has no repro test, else the fix job. The fix job is
/// written only after the repro command, run on main, exited non-zero with the bug's error
/// signature in its output; it lists the repro command as `fail-before` and its `files_allowed`
/// is the request's scope, which must use only `/`, `*` and `**` glob syntax and must not match
/// the repro test.
pub fn job(req: &Request, main: &dyn Main) -> Result<Job, Refusal> {
    let bug = req.bug;
    let owner = req.owner;
    let signature = bug
        .error_signature
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(Refusal::NoSignature)?;
    if owner.test_cmd.trim().is_empty() {
        return Err(Refusal::NoTestCmd);
    }
    let lint = owner.lint_cmd.as_deref().filter(|c| !c.trim().is_empty());

    let job = match req.repro {
        None => {
            let test = repro_path(owner, bug);
            Job {
                id: format!("bug-{}-repro", slug(&bug.id)),
                stage: "repro".into(),
                lane: req.lane.into(),
                verify: lint
                    .map(|l| check("lint", l, Expect::Pass))
                    .into_iter()
                    .collect(),
                files_allowed: vec![test.clone()],
                open_pr: false,
                brief: format!(
                    "Bug {} ({}). Write one test in `{test}` that performs the reported steps \
                     and asserts the correct behaviour, so that it fails now with the error \
                     `{signature}`. Edit no other file.\n",
                    bug.id, bug.severity
                ),
            }
        }
        Some(repro) => {
            let run = main.run(&repro.cmd).map_err(Refusal::Run)?;
            if run.exit == 0 || !run.output.contains(signature) {
                return Err(Refusal::NotRed {
                    exit: run.exit,
                    signature: signature.to_string(),
                });
            }
            if req.scope.is_empty() {
                return Err(Refusal::NoScope);
            }
            if let Some(glob) = req
                .scope
                .iter()
                .find(|g| g.chars().any(|c| matches!(c, '?' | '[' | '\\')))
            {
                return Err(Refusal::UnsupportedGlob(glob.clone()));
            }
            if let Some(glob) = req.scope.iter().find(|g| glob_match(g, &repro.test)) {
                return Err(Refusal::TouchesRepro(glob.clone()));
            }
            let mut verify = vec![
                check("regression", &repro.cmd, Expect::FailBefore),
                check("tests", &owner.test_cmd, Expect::Pass),
            ];
            verify.extend(lint.map(|l| check("lint", l, Expect::Pass)));
            Job {
                id: format!("bug-{}-fix", slug(&bug.id)),
                stage: "fix".into(),
                lane: req.lane.into(),
                verify,
                files_allowed: req.scope.to_vec(),
                open_pr: false,
                brief: format!(
                    "Bug {} ({}). The test `{}` fails with `{signature}`. Change only the files \
                     in files_allowed until it and every verify command pass. Never edit the \
                     test.\n",
                    bug.id, bug.severity, repro.test
                ),
            }
        }
    };
    if job.brief.len() > BRIEF_LIMIT {
        return Err(Refusal::BriefTooLong(job.brief.len()));
    }
    Ok(job)
}

// ---- bug attempts ------------------------------------------------------------------------

/// Which stop rule ended the tries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Stop {
    /// Three tries failed.
    #[serde(rename = "attempts")]
    Attempts,
    /// The last two tries made the same diff.
    #[serde(rename = "same-diff")]
    SameDiff,
}

/// `bug attempts`: `diffs` are the diffs of the failed tries, oldest first. `None` allows another
/// try. No cost is read: a stop is by count and by repeat only.
pub fn attempts(diffs: &[String]) -> Option<Stop> {
    if diffs.len() >= MAX_ATTEMPTS {
        return Some(Stop::Attempts);
    }
    match diffs {
        [.., a, b] if a.trim() == b.trim() => Some(Stop::SameDiff),
        _ => None,
    }
}

/// What parking a bug did: the bug row to store and the one open item in Waiting on you.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parked {
    pub bug: Bug,
    pub item: WaitingItem,
}

/// Stop the bug: its status becomes `unreproduced` when it holds no repro test, else `parked`;
/// its attempts become `tries`; and the spec's one owner question is raised (a second call for
/// the same bug finds the item open and raises nothing more).
pub fn park(
    bug: &Bug,
    has_repro: bool,
    tries: usize,
    spec: &Spec,
    items: &RunnerItems,
    now: UnixSeconds,
) -> Result<Parked, CoreError> {
    let step = spec
        .steps
        .iter()
        .find(|s| s.id == PARK_STEP)
        .ok_or_else(|| CoreError::Parse(format!("spec {}: no step {PARK_STEP}", spec.id)))?;
    let ask = step
        .ask
        .as_ref()
        .ok_or_else(|| CoreError::Parse(format!("step {PARK_STEP}: no ask")))?;
    let item = items.raise(spec, step, &bug.id, ask, now)?;
    let mut bug = bug.clone();
    bug.status = if has_repro { "parked" } else { "unreproduced" }.into();
    bug.attempts = tries as u64;
    Ok(Parked { bug, item })
}

// ---- bug lesson-check --------------------------------------------------------------------

/// Why the pitfalls diff fails the lesson check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LessonError {
    /// No added line reads `bug: <id> class: <class> test: <test>`.
    NoLine,
    /// More than one does; the count.
    ManyLines(usize),
    /// The entry adds more than `LESSON_WORDS` words; the count.
    TooLong(usize),
}

/// `bug lesson-check`: `diff` is the unified diff of `docs/pitfalls.md` over the fix's range.
/// Exactly one added line must read `bug: <bug_id> class: <class_id> test: <test>`, and the added
/// lines together are at most `LESSON_WORDS` words. The class id is returned.
pub fn lesson_check(diff: &str, bug_id: &str, test: &str) -> Result<String, LessonError> {
    let added: Vec<&str> = diff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .map(|l| l[1..].trim())
        .collect();
    let lines: Vec<String> = added
        .iter()
        .filter_map(|l| {
            let rest = l.strip_prefix("bug:")?.trim_start().strip_prefix(bug_id)?;
            let rest = rest.strip_prefix(" class:")?;
            let (class, found) = rest.split_once(" test:")?;
            (found.trim() == test && !class.trim().is_empty() && !class.trim().contains(' '))
                .then(|| class.trim().to_string())
        })
        .collect();
    match lines.len() {
        0 => return Err(LessonError::NoLine),
        1 => {}
        n => return Err(LessonError::ManyLines(n)),
    }
    let words: usize = added.iter().map(|l| l.split_whitespace().count()).sum();
    if words > LESSON_WORDS {
        return Err(LessonError::TooLong(words));
    }
    Ok(lines.into_iter().next().unwrap_or_default())
}

// ---- bug signature-check -----------------------------------------------------------------

/// Searches a revision of the clone for an extended regular expression.
pub trait Grep {
    /// Whether `pattern` matches in any of `files` at `rev`.
    fn hits(&self, rev: &str, pattern: &str, files: &[String]) -> Result<bool, CoreError>;
}

/// `git grep -E` and `git diff --name-only` in one clone.
pub struct GitGrep {
    pub clone: PathBuf,
}

impl GitGrep {
    fn git(&self, args: &[&str]) -> Result<std::process::Output, CoreError> {
        Command::new("git")
            .arg("-C")
            .arg(&self.clone)
            .args(args)
            .output()
            .map_err(|e| CoreError::Io(format!("git: {e}")))
    }

    /// The files the diff `base..head` changes.
    pub fn changed(&self, base: &str, head: &str) -> Result<Vec<String>, CoreError> {
        let out = self.git(&["diff", "--name-only", "-z", base, head])?;
        if !out.status.success() {
            return Err(CoreError::Io(format!(
                "git diff: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_string)
            .collect())
    }
}

impl Grep for GitGrep {
    fn hits(&self, rev: &str, pattern: &str, files: &[String]) -> Result<bool, CoreError> {
        let mut args = vec!["grep", "-E", "-q", "-e", pattern, rev, "--"];
        args.extend(files.iter().map(String::as_str));
        let out = self.git(&args)?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(CoreError::Io(format!(
                "git grep: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))),
        }
    }
}

/// Why one search pattern, or the list, fails the signature check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureError {
    /// The list holds this many patterns, not 2 to 4.
    Count(usize),
    /// The pattern hits nothing in the fix's files at base.
    MissesBase(String),
    /// The pattern still hits in the fix's files at head.
    HitsHead(String),
}

/// `bug signature-check`: 2 to 4 patterns, each hitting a file the fix changes at `base` and
/// nothing in those files at `head`. The first failure is returned.
pub fn signature_check(
    grep: &dyn Grep,
    patterns: &[String],
    files: &[String],
    base: &str,
    head: &str,
) -> Result<Result<(), SignatureError>, CoreError> {
    if !(2..=4).contains(&patterns.len()) {
        return Ok(Err(SignatureError::Count(patterns.len())));
    }
    for pattern in patterns {
        if !grep.hits(base, pattern, files)? {
            return Ok(Err(SignatureError::MissesBase(pattern.clone())));
        }
        if grep.hits(head, pattern, files)? {
            return Ok(Err(SignatureError::HitsHead(pattern.clone())));
        }
    }
    Ok(Ok(()))
}

// ---- bug status-row ----------------------------------------------------------------------

/// One row of `docs/STATUS.md`: Area | State | Verified by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    pub area: String,
    pub state: String,
    pub verified_by: String,
}

impl StatusRow {
    /// The row of a fixed bug: its state is `Fixed`, verified by the repro test at the sha before
    /// the fix commit (`parent`), where the test fails.
    pub fn fixed(area: &str, repro_test: &str, parent: &str) -> StatusRow {
        StatusRow {
            area: area.to_string(),
            state: "Fixed".to_string(),
            verified_by: format!("`{repro_test}` (fails at {parent}, passes after)"),
        }
    }

    pub fn line(&self) -> String {
        format!("| {} | {} | {} |", self.area, self.state, self.verified_by)
    }
}

fn cells(line: &str) -> Option<Vec<&str>> {
    let line = line.trim();
    let inner = line.strip_prefix('|')?.strip_suffix('|')?;
    Some(inner.split('|').map(str::trim).collect())
}

/// `docs/STATUS.md` with `row` in it: the row of the same area is replaced in place, else the row
/// ends the table. The area's row is never doubled.
pub fn put_status_row(text: &str, row: &StatusRow) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut done = false;
    let mut last_row = None;
    for line in text.lines() {
        match cells(line) {
            Some(c) if c.len() == 3 => {
                if c[0] == row.area {
                    if !done {
                        out.push(row.line());
                        done = true;
                    }
                    last_row = Some(out.len());
                    continue;
                }
                out.push(line.to_string());
                last_row = Some(out.len());
            }
            _ => out.push(line.to_string()),
        }
    }
    if !done {
        out.insert(last_row.unwrap_or(out.len()), row.line());
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// `bug status-row` check: `docs/STATUS.md` holds exactly one row whose Verified by cell names
/// the repro test and `parent`.
pub fn status_row_ok(text: &str, repro_test: &str, parent: &str) -> bool {
    text.lines()
        .filter_map(cells)
        .filter(|c| c.len() == 3 && c[2].contains(repro_test) && c[2].contains(parent))
        .count()
        == 1
}

/// The file the status row lives in, under the owning repo's clone.
pub fn status_path(clone: &Path) -> PathBuf {
    clone.join("docs").join("STATUS.md")
}
