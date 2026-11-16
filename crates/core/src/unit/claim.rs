//! unit-claim (#403): a claim file is the lock; GitHub only mirrors it. `claim` re-checks the
//! unit's blockers, creates `claims/<repo>/<issue>.json` on the fleet-state branch as one pushed
//! transaction (research/fleet-state-write.md: check the path absent in base B, build a commit
//! whose only parent is B, push it with an explicit lease on B), then mirrors the claim to the
//! assignee and the in-progress label. `release` clears the mirror before deleting the same
//! token's file; `reap` releases stale claims with no running job or open PR.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::clock::{Clock, UnixSeconds};
use crate::error::CoreError;
use crate::lock::run_tag;

/// The exit code of `switchboard unit claim` when the unit is taken or blocked: not now.
pub const NOT_NOW_EXIT: i32 = 75;

/// The label that mirrors a claim.
pub const IN_PROGRESS: &str = "in-progress";

/// Default stale age (seconds); running jobs and open PRs remain exempt.
pub const STALE_LIMIT: i64 = 6 * 60 * 60;

/// The branch the claim files live on.
const BRANCH: &str = "fleet-state";

/// How many times a lost race for the branch is retried before giving up.
const ATTEMPTS: u32 = 8;

/// The GitHub side of a claim: the blocker check and the mirror.
pub trait Mirror {
    /// The unit's blockers that are not closed with a resolution record, read fresh.
    fn open_blockers(&self, repo: &str, issue: u64) -> Result<Vec<u64>, CoreError>;
    fn assign(&self, repo: &str, issue: u64, worker: &str) -> Result<(), CoreError>;
    /// Idempotent: an already absent assignee is success, including after a partial cleanup.
    fn unassign(&self, repo: &str, issue: u64, worker: &str) -> Result<(), CoreError>;
    fn label(&self, repo: &str, issue: u64, label: &str) -> Result<(), CoreError>;
    /// Idempotent: an already absent label is success, including after a partial cleanup.
    fn unlabel(&self, repo: &str, issue: u64, label: &str) -> Result<(), CoreError>;
}

/// Fresh activity checks supplied by the caller. A failed check must keep the claim.
pub trait Activity {
    fn running_job(&self, repo: &str, issue: u64) -> Result<bool, CoreError>;
    fn open_pr(&self, repo: &str, issue: u64) -> Result<bool, CoreError>;
}

/// The claim file's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub issue: u64,
    /// The repository and issue, `owner/name#number`.
    pub unit: String,
    pub machine: String,
    /// Unique to one acquisition attempt: it tells our claim from another worker's.
    pub token: String,
    pub claimed_at: UnixSeconds,
    /// The project revision the caller will build, supplied at acquisition.
    pub base_sha: String,
}

/// What `claim` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// This worker holds the claim (exit 0).
    Claimed(Claim),
    /// Another worker's claim file exists (exit 75).
    Taken,
    /// These blockers are still open (exit 75).
    Blocked(Vec<u64>),
}

impl Outcome {
    pub fn exit_code(&self) -> i32 {
        match self {
            Outcome::Claimed(_) => 0,
            Outcome::Taken | Outcome::Blocked(_) => NOT_NOW_EXIT,
        }
    }
}

/// A failure to acquire or mirror a claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimError {
    /// Acquisition failed before the mirror writes.
    Acquisition(CoreError),
    /// The file was pushed, but a mirror write failed. Release using `claim.token`.
    Mirror { claim: Box<Claim>, error: CoreError },
}

impl From<CoreError> for ClaimError {
    fn from(error: CoreError) -> Self {
        Self::Acquisition(error)
    }
}

impl std::fmt::Display for ClaimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Acquisition(error) => write!(f, "{error}"),
            Self::Mirror { claim, error } => {
                write!(
                    f,
                    "claim {} is held but mirroring failed: {error}",
                    claim.unit
                )
            }
        }
    }
}

impl std::error::Error for ClaimError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Acquisition(error) | Self::Mirror { error, .. } => error,
        })
    }
}

/// Where the claim files are kept: the pipeline remote (a URL or path) with its fleet-state
/// branch, and a directory for the private repositories of one attempt each.
pub struct Store {
    pub remote: String,
    pub scratch: PathBuf,
}

fn validate_repo(repo: &str) -> Result<(), CoreError> {
    let parts: Vec<_> = repo.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        return Err(CoreError::Parse("claim repo must be owner/name".into()));
    }
    Ok(())
}

fn claim_path(repo: &str, issue: u64) -> String {
    format!("claims/{repo}/{issue}.json")
}

impl Claim {
    fn to_json(&self) -> String {
        serde_json::json!({
            "unit": self.unit,
            "machine": self.machine,
            "token": self.token,
            "claimed_at": self.claimed_at,
            "base_sha": self.base_sha,
        })
        .to_string()
    }

    fn parse(text: &str, repo: &str, issue: u64) -> Result<Claim, CoreError> {
        let bad = |why: &str| CoreError::Parse(format!("claim file: {why}"));
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| bad(&e.to_string()))?;
        let unit = v["unit"].as_str().ok_or_else(|| bad("no unit"))?;
        if unit != format!("{repo}#{issue}") {
            return Err(bad("unit does not match claim path"));
        }
        Ok(Claim {
            issue,
            unit: unit.to_string(),
            machine: v["machine"]
                .as_str()
                .ok_or_else(|| bad("no machine"))?
                .to_string(),
            token: v["token"]
                .as_str()
                .ok_or_else(|| bad("no token"))?
                .to_string(),
            claimed_at: v["claimed_at"]
                .as_i64()
                .ok_or_else(|| bad("no claimed_at"))?,
            base_sha: v["base_sha"]
                .as_str()
                .ok_or_else(|| bad("no base_sha"))?
                .to_string(),
        })
    }
}

/// One attempt's private bare repository, removed when dropped.
struct Attempt {
    dir: PathBuf,
}

impl Drop for Attempt {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Attempt {
    fn git(
        &self,
        args: &[&str],
        stdin: Option<&str>,
        env: &[(&str, &str)],
    ) -> Result<String, CoreError> {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.dir).args(args);
        cmd.env("GIT_DIR", &self.dir);
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| CoreError::Io(format!("git: {e}")))?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            use std::io::Write;
            pipe.write_all(text.as_bytes())
                .map_err(|e| CoreError::Io(format!("git: {e}")))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| CoreError::Io(format!("git: {e}")))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(CoreError::Io(format!(
                "git {}: {}",
                args.first().copied().unwrap_or(""),
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }
}

impl Store {
    /// Fetch the branch into a fresh private repository; the commit B it is at.
    fn begin(&self) -> Result<(Attempt, String), CoreError> {
        let dir = self.scratch.join(format!("claim-{}", run_tag()));
        std::fs::create_dir_all(&dir)
            .map_err(|e| CoreError::Io(format!("{}: {e}", dir.display())))?;
        let attempt = Attempt { dir };
        attempt.git(&["init", "--bare", "--quiet"], None, &[])?;
        let spec = format!("+refs/heads/{BRANCH}:refs/remotes/origin/{BRANCH}");
        attempt.git(
            &["fetch", "--quiet", "--no-tags", &self.remote, &spec],
            None,
            &[],
        )?;
        let base = attempt
            .git(
                &["rev-parse", &format!("refs/remotes/origin/{BRANCH}")],
                None,
                &[],
            )?
            .trim()
            .to_string();
        Ok((attempt, base))
    }

    /// The claim file in base `base`, if there is one.
    fn read(
        &self,
        attempt: &Attempt,
        base: &str,
        repo: &str,
        issue: u64,
    ) -> Result<Option<Claim>, CoreError> {
        let spec = format!("{base}:{}", claim_path(repo, issue));
        if attempt.git(&["cat-file", "-e", &spec], None, &[]).is_err() {
            return Ok(None);
        }
        Claim::parse(
            &attempt.git(&["cat-file", "-p", &spec], None, &[])?,
            repo,
            issue,
        )
        .map(Some)
    }

    /// Commit `base` plus (or minus) one claim file and push it with a lease on `base`.
    /// `Ok(false)` when the branch moved: the caller looks again.
    fn publish(
        &self,
        attempt: &Attempt,
        base: &str,
        repo: &str,
        issue: u64,
        add: Option<&Claim>,
        message: &str,
    ) -> Result<bool, CoreError> {
        let index = attempt.dir.join("index");
        let env = [("GIT_INDEX_FILE", index.to_str().unwrap_or_default())];
        attempt.git(&["read-tree", base], None, &env)?;
        let path = claim_path(repo, issue);
        match add {
            Some(claim) => {
                let blob = attempt.git(
                    &["hash-object", "-w", "--stdin"],
                    Some(&claim.to_json()),
                    &[],
                )?;
                let entry = format!("100644,{},{path}", blob.trim());
                attempt.git(
                    &["update-index", "--add", "--cacheinfo", &entry],
                    None,
                    &env,
                )?;
            }
            None => {
                // Mode 0 with the null id removes the entry; `--force-remove` needs a work tree.
                let entry = format!("0 {}\t{path}\n", "0".repeat(40));
                attempt.git(&["update-index", "--index-info"], Some(&entry), &env)?;
            }
        }
        let tree = attempt.git(&["write-tree"], None, &env)?;
        let who = [
            ("GIT_AUTHOR_NAME", "switchboard"),
            ("GIT_AUTHOR_EMAIL", "switchboard@users.noreply.github.com"),
            ("GIT_COMMITTER_NAME", "switchboard"),
            (
                "GIT_COMMITTER_EMAIL",
                "switchboard@users.noreply.github.com",
            ),
        ];
        let commit = attempt.git(
            &["commit-tree", tree.trim(), "-p", base, "-m", message],
            None,
            &who,
        )?;
        let lease = format!("--force-with-lease=refs/heads/{BRANCH}:{base}");
        let refspec = format!("{}:refs/heads/{BRANCH}", commit.trim());
        let pushed = attempt.git(
            &["push", "--porcelain", &lease, &self.remote, &refspec],
            None,
            &[],
        );
        // A pushed ref line starts with a space (fast-forward); `=` is up to date, not ours.
        Ok(pushed.is_ok_and(|out| {
            out.lines()
                .any(|l| l.starts_with(' ') || l.starts_with("*"))
        }))
    }

    /// Create the claim file unless one exists. The flag says whether this call must mirror the
    /// claim: `false` only for the holder's own re-claim, which writes nothing.
    fn create(
        &self,
        repo: &str,
        issue: u64,
        worker: &str,
        now: UnixSeconds,
        base_sha: &str,
    ) -> Result<(Outcome, bool), CoreError> {
        let claim = Claim {
            issue,
            unit: format!("{repo}#{issue}"),
            machine: worker.to_string(),
            token: run_tag(),
            claimed_at: now,
            base_sha: base_sha.to_string(),
        };
        for attempt_no in 0..ATTEMPTS {
            let (attempt, base) = self.begin()?;
            if let Some(held) = self.read(&attempt, &base, repo, issue)? {
                // A lost reply: the file may be ours.
                if held.token == claim.token {
                    return Ok((Outcome::Claimed(claim), true));
                }
                // The holder's own re-claim: no write, and the first claim's token and time stand.
                if held.machine == worker {
                    return Ok((Outcome::Claimed(held), false));
                }
                return Ok((Outcome::Taken, false));
            }
            let message = format!("claim {repo}#{issue}");
            if self.publish(&attempt, &base, repo, issue, Some(&claim), &message)? {
                return Ok((Outcome::Claimed(claim), true));
            }
            std::thread::sleep(Duration::from_millis(20 * u64::from(attempt_no + 1)));
        }
        Err(CoreError::Io(format!(
            "claim #{issue}: the {BRANCH} branch kept moving"
        )))
    }

    /// Clear the mirror while the lock still exists, then remove only the expected token.
    /// Every retry checks both ownership and eligibility in the freshly fetched base.
    fn remove(
        &self,
        mirror: &dyn Mirror,
        repo: &str,
        issue: u64,
        token: &str,
        eligible: impl Fn(&Claim) -> bool,
    ) -> Result<bool, CoreError> {
        for attempt_no in 0..ATTEMPTS {
            let (attempt, base) = self.begin()?;
            let Some(held) = self.read(&attempt, &base, repo, issue)? else {
                return Ok(false);
            };
            if held.token != token || !eligible(&held) {
                return Ok(false);
            }
            mirror.unassign(repo, issue, &held.machine)?;
            mirror.unlabel(repo, issue, IN_PROGRESS)?;
            if self.publish(
                &attempt,
                &base,
                repo,
                issue,
                None,
                &format!("release {repo}#{issue}"),
            )? {
                return Ok(true);
            }
            std::thread::sleep(Duration::from_millis(20 * u64::from(attempt_no + 1)));
        }
        Err(CoreError::Io(format!(
            "release #{issue}: the {BRANCH} branch kept moving"
        )))
    }

    /// Every claim file for this repository on the branch, including the owner namespace.
    fn list(&self, repo: &str) -> Result<Vec<Claim>, CoreError> {
        let (attempt, base) = self.begin()?;
        let prefix = format!("claims/{repo}/");
        let names = attempt.git(&["ls-tree", "-r", "--name-only", &base, &prefix], None, &[])?;
        names
            .lines()
            .filter_map(|name| {
                name.strip_prefix(&prefix)?
                    .strip_suffix(".json")?
                    .parse::<u64>()
                    .ok()
            })
            .filter_map(|issue| self.read(&attempt, &base, repo, issue).transpose())
            .collect()
    }
}

/// Claim `issue` for `worker`: refuse while a blocker is open, create the claim file, then
/// mirror it. A mirror failure returns `ClaimError::Mirror` with the pushed `Claim` and the
/// original error. The file stays locked so cleanup can be retried even if GitHub remains
/// unavailable: call `release(store, mirror, repo, issue, &claim.token)` with the returned
/// claim, retrying release on cleanup errors. `base_sha` is the project revision to build.
pub fn claim(
    store: &Store,
    mirror: &dyn Mirror,
    clock: &dyn Clock,
    repo: &str,
    issue: u64,
    worker: &str,
    base_sha: &str,
) -> Result<Outcome, ClaimError> {
    validate_repo(repo)?;
    let open = mirror.open_blockers(repo, issue)?;
    if !open.is_empty() {
        return Ok(Outcome::Blocked(open));
    }
    let (outcome, write_mirror) = store.create(repo, issue, worker, clock.now(), base_sha)?;
    match outcome {
        Outcome::Claimed(claim) if write_mirror => {
            if let Err(error) = mirror
                .assign(repo, issue, worker)
                .and_then(|()| mirror.label(repo, issue, IN_PROGRESS))
            {
                return Err(ClaimError::Mirror {
                    claim: Box::new(claim),
                    error,
                });
            }
            Ok(Outcome::Claimed(claim))
        }
        other => Ok(other),
    }
}

/// Drop this acquisition's claim: clear the mirror first, then token-check the file deletion.
/// A mirror error keeps the claim for a retry. `false` means the token no longer holds it;
/// a replaced claim's mirror is untouched. Use the token returned by `claim`.
pub fn release(
    store: &Store,
    mirror: &dyn Mirror,
    repo: &str,
    issue: u64,
    token: &str,
) -> Result<bool, CoreError> {
    validate_repo(repo)?;
    store.remove(mirror, repo, issue, token, |_| true)
}

fn stale(held: &Claim, clock: &dyn Clock, limit: i64) -> bool {
    clock.now().saturating_sub(held.claimed_at) > limit
}

fn inactive(activity: &dyn Activity, repo: &str, issue: u64) -> bool {
    matches!(activity.running_job(repo, issue), Ok(false))
        && matches!(activity.open_pr(repo, issue), Ok(false))
}

/// Release stale claims in `repo` with no running job or open PR; the issues released.
/// Any activity-check error keeps that claim. Token, expiry and activity are checked again
/// for each fetched deletion base, so a replacement is never reaped by an old listing.
pub fn reap(
    store: &Store,
    mirror: &dyn Mirror,
    activity: &dyn Activity,
    clock: &dyn Clock,
    repo: &str,
    limit: i64,
) -> Result<Vec<u64>, CoreError> {
    validate_repo(repo)?;
    let mut released = Vec::new();
    for held in store.list(repo)? {
        if stale(&held, clock, limit)
            && inactive(activity, repo, held.issue)
            && store.remove(mirror, repo, held.issue, &held.token, |current| {
                stale(current, clock, limit) && inactive(activity, repo, current.issue)
            })?
        {
            released.push(held.issue);
        }
    }
    Ok(released)
}
