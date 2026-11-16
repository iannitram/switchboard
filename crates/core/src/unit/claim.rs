//! unit-claim (#403): a claim file is the lock; GitHub only mirrors it. `claim` re-checks the
//! unit's blockers, creates `claims/<issue>.json` on the fleet-state branch as one pushed
//! transaction (research/fleet-state-write.md: check the path absent in base B, build a commit
//! whose only parent is B, push it with an explicit lease on B), then mirrors the claim to the
//! assignee and the in-progress label. `release` drops the file and the mirror; `reap` releases
//! the claims older than the stale limit.

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

/// A claim older than this (seconds) is stale: the reaper releases it.
pub const STALE_LIMIT: i64 = 6 * 60 * 60;

/// The branch the claim files live on.
const BRANCH: &str = "fleet-state";

/// How many times a lost race for the branch is retried before giving up.
const ATTEMPTS: u32 = 8;

/// The GitHub side of a claim: the blocker check and the mirror.
pub trait Mirror {
    /// The unit's blockers that are not closed with a resolution record, read fresh.
    fn open_blockers(&self, issue: u64) -> Result<Vec<u64>, CoreError>;
    fn assign(&self, issue: u64, worker: &str) -> Result<(), CoreError>;
    fn unassign(&self, issue: u64, worker: &str) -> Result<(), CoreError>;
    fn label(&self, issue: u64, label: &str) -> Result<(), CoreError>;
    fn unlabel(&self, issue: u64, label: &str) -> Result<(), CoreError>;
}

/// The claim file's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub issue: u64,
    pub worker: String,
    /// Unique to one acquisition attempt: it tells our claim from another worker's.
    pub token: String,
    pub claimed_at: UnixSeconds,
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

/// Where the claim files are kept: the pipeline remote (a URL or path) with its fleet-state
/// branch, and a directory for the private repositories of one attempt each.
pub struct Store {
    pub remote: String,
    pub scratch: PathBuf,
}

fn claim_path(issue: u64) -> String {
    format!("claims/{issue}.json")
}

impl Claim {
    fn to_json(&self) -> String {
        serde_json::json!({
            "issue": self.issue,
            "worker": self.worker,
            "token": self.token,
            "claimed_at": self.claimed_at,
        })
        .to_string()
    }

    fn parse(text: &str) -> Result<Claim, CoreError> {
        let bad = |why: &str| CoreError::Parse(format!("claim file: {why}"));
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| bad(&e.to_string()))?;
        Ok(Claim {
            issue: v["issue"].as_u64().ok_or_else(|| bad("no issue"))?,
            worker: v["worker"]
                .as_str()
                .ok_or_else(|| bad("no worker"))?
                .to_string(),
            token: v["token"]
                .as_str()
                .ok_or_else(|| bad("no token"))?
                .to_string(),
            claimed_at: v["claimed_at"]
                .as_i64()
                .ok_or_else(|| bad("no claimed_at"))?,
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
    fn read(&self, attempt: &Attempt, base: &str, issue: u64) -> Result<Option<Claim>, CoreError> {
        let spec = format!("{base}:{}", claim_path(issue));
        if attempt.git(&["cat-file", "-e", &spec], None, &[]).is_err() {
            return Ok(None);
        }
        Claim::parse(&attempt.git(&["cat-file", "-p", &spec], None, &[])?).map(Some)
    }

    /// Commit `base` plus (or minus) one claim file and push it with a lease on `base`.
    /// `Ok(false)` when the branch moved: the caller looks again.
    fn publish(
        &self,
        attempt: &Attempt,
        base: &str,
        issue: u64,
        add: Option<&Claim>,
        message: &str,
    ) -> Result<bool, CoreError> {
        let index = attempt.dir.join("index");
        let env = [("GIT_INDEX_FILE", index.to_str().unwrap_or_default())];
        attempt.git(&["read-tree", base], None, &env)?;
        let path = claim_path(issue);
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

    /// Create the claim file unless one exists.
    fn create(&self, issue: u64, worker: &str, now: UnixSeconds) -> Result<Outcome, CoreError> {
        let claim = Claim {
            issue,
            worker: worker.to_string(),
            token: run_tag(),
            claimed_at: now,
        };
        for attempt_no in 0..ATTEMPTS {
            let (attempt, base) = self.begin()?;
            if let Some(held) = self.read(&attempt, &base, issue)? {
                // A lost reply: the file may be ours.
                return Ok(if held.token == claim.token {
                    Outcome::Claimed(claim)
                } else {
                    Outcome::Taken
                });
            }
            let message = format!("claim #{issue} for {worker}");
            if self.publish(&attempt, &base, issue, Some(&claim), &message)? {
                return Ok(Outcome::Claimed(claim));
            }
            std::thread::sleep(Duration::from_millis(20 * u64::from(attempt_no + 1)));
        }
        Err(CoreError::Io(format!(
            "claim #{issue}: the {BRANCH} branch kept moving"
        )))
    }

    /// Remove the claim file if there is one; the claim it held.
    fn remove(&self, issue: u64) -> Result<Option<Claim>, CoreError> {
        for attempt_no in 0..ATTEMPTS {
            let (attempt, base) = self.begin()?;
            let Some(held) = self.read(&attempt, &base, issue)? else {
                return Ok(None);
            };
            if self.publish(&attempt, &base, issue, None, &format!("release #{issue}"))? {
                return Ok(Some(held));
            }
            std::thread::sleep(Duration::from_millis(20 * u64::from(attempt_no + 1)));
        }
        Err(CoreError::Io(format!(
            "release #{issue}: the {BRANCH} branch kept moving"
        )))
    }

    /// Every claim file on the branch.
    fn list(&self) -> Result<Vec<Claim>, CoreError> {
        let (attempt, base) = self.begin()?;
        let names = attempt.git(&["ls-tree", "--name-only", &base, "claims/"], None, &[])?;
        names
            .lines()
            .filter_map(|name| {
                name.strip_prefix("claims/")?
                    .strip_suffix(".json")?
                    .parse::<u64>()
                    .ok()
            })
            .filter_map(|issue| self.read(&attempt, &base, issue).transpose())
            .collect()
    }
}

/// Claim `issue` for `worker`: refuse while a blocker is open, create the claim file, then
/// mirror it. The file is the lock; if the mirror fails the error is returned and the claim
/// stands until `release` drops it.
pub fn claim(
    store: &Store,
    mirror: &dyn Mirror,
    clock: &dyn Clock,
    issue: u64,
    worker: &str,
) -> Result<Outcome, CoreError> {
    let open = mirror.open_blockers(issue)?;
    if !open.is_empty() {
        return Ok(Outcome::Blocked(open));
    }
    let outcome = store.create(issue, worker, clock.now())?;
    if matches!(outcome, Outcome::Claimed(_)) {
        mirror.assign(issue, worker)?;
        mirror.label(issue, IN_PROGRESS)?;
    }
    Ok(outcome)
}

/// Drop the claim on `issue`: the file first (the lock), then the assignee and the label.
/// `false` when there was no claim file.
pub fn release(store: &Store, mirror: &dyn Mirror, issue: u64) -> Result<bool, CoreError> {
    let Some(held) = store.remove(issue)? else {
        return Ok(false);
    };
    mirror.unassign(issue, &held.worker)?;
    mirror.unlabel(issue, IN_PROGRESS)?;
    Ok(true)
}

/// Release every claim older than `limit` seconds; the issues released.
pub fn reap(
    store: &Store,
    mirror: &dyn Mirror,
    clock: &dyn Clock,
    limit: i64,
) -> Result<Vec<u64>, CoreError> {
    let now = clock.now();
    let mut released = Vec::new();
    for held in store.list()? {
        if now - held.claimed_at > limit && release(store, mirror, held.issue)? {
            released.push(held.issue);
        }
    }
    Ok(released)
}
