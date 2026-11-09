//! AUTO-3: the autopilot's passes after each poll. `after_poll` loads the snapshots and the
//! autopilot's small state file, runs each pass and saves the state. The first pass,
//! `review_pass`, starts the one R25 review of each new PR this machine's satellite opened, on
//! another service than the builder's, after AUTO-1's `may_spend`. Later passes (AUTO-2, AUTO-4)
//! hang off `run_passes`.

use super::review::{first_rung, review_job, Rung};
use crate::fleet::{FleetBridge, FLEET_ROOT_ENV};
use crate::github::snapshot::{RepoSnapshot, SnapshotStore};
use crate::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The autopilot's state file, in the app's data folder.
pub const STATE_FILE: &str = "autopilot.json";

/// The services a review may run on, in the order they are asked. The builder's own service is
/// skipped (R25).
pub const REVIEW_LANES: [&str; 3] = ["chatgpt", "gemini", "claudeapi"];

/// What the autopilot remembers between polls.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutopilotState {
    /// Every PR whose review was started, as `owner/name#pr`, oldest first.
    pub reviews_started: Vec<String>,
}

impl AutopilotState {
    /// The state in `data_dir`. A missing file is the default; text that does not parse is a
    /// parse error that names the file.
    pub fn load(data_dir: &Path) -> Result<Self, CoreError> {
        let path = data_dir.join(STATE_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
        };
        serde_json::from_str(&text)
            .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))
    }

    /// Write the state to `data_dir` as pretty JSON, making the folder.
    pub fn save(&self, data_dir: &Path) -> Result<(), CoreError> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| CoreError::Io(format!("{}: {e}", data_dir.display())))?;
        let path = data_dir.join(STATE_FILE);
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))?;
        std::fs::write(&path, text).map_err(|e| CoreError::Io(format!("{}: {e}", path.display())))
    }
}

/// The seam between a pass and the fleet: the tests use a fake, the app `FleetStarter`.
pub trait JobStarter {
    /// AUTO-1: may `lane` spend now?
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError>;
    /// Start `job` with `brief` as its prompt. It does not wait for the job.
    fn start(&mut self, job: &Value, brief: &str) -> Result<(), CoreError>;
}

/// A machine's canonical name, as the fleet's `canon_machine` gives it: trimmed and lower case,
/// with the old host names mapped to the hardware names.
pub fn canon_machine(name: &str) -> String {
    let name = name.trim().to_lowercase();
    match name.as_str() {
        "hyperion" => "desktop-3090ti".into(),
        "z13" => "strix-halo".into(),
        "legiongo" => "amd-handheld".into(),
        "shuttlecraft6" | "mac" => "mac-mini".into(),
        _ => name,
    }
}

/// This machine's name: a non-blank `fleet_machine` (the `FLEET_MACHINE` env) wins, else the
/// first line of `<home>/.config/software-depot/machine`. `None` when neither names one.
pub fn this_machine(fleet_machine: Option<&str>, home: &Path) -> Option<String> {
    if let Some(name) = fleet_machine.filter(|n| !n.trim().is_empty()) {
        return Some(canon_machine(name));
    }
    let path = home.join(".config").join("software-depot").join("machine");
    let text = std::fs::read_to_string(path).ok()?;
    let first = text.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        None
    } else {
        Some(canon_machine(first))
    }
}

/// The service that built a PR, from its first `model:` label. `None` when there is no such
/// label or the model is not one the fleet knows.
pub fn builder_lane(labels: &[String]) -> Option<&'static str> {
    let model = labels
        .iter()
        .find_map(|l| l.strip_prefix("model:"))?
        .trim()
        .to_lowercase();
    let lanes: [(&str, &'static str); 9] = [
        ("qwen", "qwen"),
        ("gpt", "chatgpt"),
        ("gemini", "gemini"),
        ("deepseek", "deepseek"),
        ("claude", "claudeapi"),
        ("sonnet", "claudeapi"),
        ("haiku", "claudeapi"),
        ("opus", "claudeapi"),
        ("fable", "claudeapi"),
    ];
    lanes
        .iter()
        .find(|(start, _)| model.starts_with(start))
        .map(|(_, lane)| *lane)
}

/// `text` made safe for a job id: each character outside `A-Z a-z 0-9 . _ -` becomes `-`.
pub fn id_part(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// The fleet size letter a rung runs at: Easy `S`, Medium `M`, anything else `L`.
pub fn size_letter(rung: Rung) -> &'static str {
    match rung {
        Rung::Easy => "S",
        Rung::Medium => "M",
        _ => "L",
    }
}

/// The repo's name, the part after the `/` (all of it when there is none).
fn repo_name(repo: &str) -> &str {
    repo.split_once('/').map_or(repo, |(_, name)| name)
}

/// The fleet job for try 1 of the review of `repo#pr` (closing `issue`): `review_job` with the
/// keys `fleet run` needs, on the `reviewer` lane. The read-only review kind replaces `review`,
/// and `review_job`'s brief list moves to `reads` (the fleet's `brief` is `exact` or `open`).
pub fn review_fleet_job(
    repo: &str,
    pr: u64,
    issue: u64,
    rung: Rung,
    builder: &str,
    reviewer: &str,
) -> Value {
    let mut job = review_job(repo, pr, issue, rung, 1, builder);
    let extra = json!({
        "id": format!("review-{}-{pr}", id_part(repo_name(repo))),
        "title": format!("Review {repo}#{pr} (closes #{issue})"),
        "kind": "adversarial-review-pr-diff",
        "size": size_letter(rung),
        "base": "main",
        "est_minutes": 20,
        "verify": [],
        "files_allowed": [],
        "lane": reviewer,
        "brief": "exact",
        "reads": ["diff", "issue"],
    });
    if let (Some(job), Value::Object(extra)) = (job.as_object_mut(), extra) {
        job.extend(extra);
    }
    job
}

/// The review's prompt: the PR, its head, the issue, and the verdict line it must end with.
pub fn review_brief(repo: &str, pr: u64, issue: u64, head_sha: &str) -> String {
    format!(
        "Review PR #{pr} in {repo} at head {head_sha} against issue #{issue}.\n\
         Read the PR's diff and the issue's body. Check every Expectation and every Done when line, and that the PR changed no locked test.\n\
         Post one comment on the PR. Its last line is exactly `VERDICT: PASS {head_sha}` or `VERDICT: FAIL {head_sha}`.\n"
    )
}

/// The rung a PR's review starts at: `first_rung` of the `size:` label of the first issue it
/// closes, when that issue is in the snapshot; Hard otherwise.
fn pr_rung(snap: &RepoSnapshot, closes: &[u64]) -> Rung {
    let size = closes
        .first()
        .and_then(|n| snap.issues.iter().find(|i| i.number == *n))
        .and_then(|i| i.labels.iter().find_map(|l| l.strip_prefix("size:")))
        .unwrap_or("");
    first_rung(size)
}

/// AUTO-3: start the review of each open PR labelled `by:<machine>` that has no verdict, closes
/// an issue, and has no review started yet. Repos in the order given (archived ones skipped),
/// PRs smallest first. The reviewer is the first of `REVIEW_LANES`, skipping the builder's,
/// that may spend; when none may, nothing starts and the next poll tries again. Answers the
/// job ids started; the first error stops the pass.
pub fn review_pass(
    snaps: &[RepoSnapshot],
    machine: &str,
    state: &mut AutopilotState,
    starter: &mut dyn JobStarter,
) -> Result<Vec<String>, CoreError> {
    let by = format!("by:{machine}");
    let mut started = Vec::new();
    for snap in snaps.iter().filter(|s| !s.archived) {
        let mut pulls: Vec<_> = snap.pulls.iter().collect();
        pulls.sort_by_key(|p| p.number);
        for pr in pulls {
            let key = format!("{}#{}", snap.repo, pr.number);
            let qualifies = pr.state == "open"
                && pr.labels.contains(&by)
                && pr.verdicts.is_empty()
                && !pr.closes.is_empty()
                && !state.reviews_started.contains(&key);
            if !qualifies {
                continue;
            }
            let builder = builder_lane(&pr.labels).unwrap_or("");
            let mut reviewer = None;
            for lane in REVIEW_LANES.iter().filter(|l| **l != builder) {
                if starter.may_spend(lane)? {
                    reviewer = Some(*lane);
                    break;
                }
            }
            let Some(reviewer) = reviewer else {
                continue;
            };
            let issue = pr.closes[0];
            let rung = pr_rung(snap, &pr.closes);
            let job = review_fleet_job(&snap.repo, pr.number, issue, rung, builder, reviewer);
            let brief = review_brief(&snap.repo, pr.number, issue, &pr.head_sha);
            starter.start(&job, &brief)?;
            state.reviews_started.push(key);
            started.push(job["id"].as_str().unwrap_or_default().to_string());
        }
    }
    Ok(started)
}

/// What `after_poll` needs: the app's data folder, this machine's name, and the starter.
pub struct AfterPoll<'a> {
    pub data_dir: &'a Path,
    pub machine: Option<&'a str>,
    pub starter: &'a mut dyn JobStarter,
}

/// AUTO-3: the passes after a poll. Loads the snapshots and the state, runs the passes, and
/// saves the state even when a pass failed (so a review already started is not started again).
/// Answers the job ids started.
pub fn after_poll(ctx: &mut AfterPoll<'_>) -> Result<Vec<String>, CoreError> {
    let snaps = SnapshotStore::new(ctx.data_dir).load_all()?;
    let mut state = AutopilotState::load(ctx.data_dir)?;
    let result = run_passes(&snaps, ctx.machine, &mut state, &mut *ctx.starter);
    state.save(ctx.data_dir)?;
    result
}

/// Each pass in turn; later passes extend `started`. No machine name starts no review.
fn run_passes(
    snaps: &[RepoSnapshot],
    machine: Option<&str>,
    state: &mut AutopilotState,
    starter: &mut dyn JobStarter,
) -> Result<Vec<String>, CoreError> {
    let mut started = Vec::new();
    if let Some(machine) = machine {
        started.extend(review_pass(snaps, machine, state, starter)?);
    }
    Ok(started)
}

/// Write `job` for `fleet run`: the brief to `<jobs_dir>/<id>.md`, then the job, pretty JSON,
/// to `<jobs_dir>/<id>.json` with `prompt_file` set to the brief and `clone` to
/// `<dev_root>/<name>`. A job with no `id` or no `repo` is refused. Answers the job file.
pub fn write_job(
    jobs_dir: &Path,
    dev_root: &Path,
    job: &Value,
    brief: &str,
) -> Result<PathBuf, CoreError> {
    let id = job["id"]
        .as_str()
        .ok_or_else(|| CoreError::Refused("job has no id".into()))?;
    let repo = job["repo"]
        .as_str()
        .ok_or_else(|| CoreError::Refused(format!("job {id} has no repo")))?;
    std::fs::create_dir_all(jobs_dir)
        .map_err(|e| CoreError::Io(format!("{}: {e}", jobs_dir.display())))?;
    let brief_path = jobs_dir.join(format!("{id}.md"));
    std::fs::write(&brief_path, brief)
        .map_err(|e| CoreError::Io(format!("{}: {e}", brief_path.display())))?;
    let mut job = job.clone();
    job["prompt_file"] = Value::String(brief_path.to_string_lossy().into_owned());
    job["clone"] = Value::String(
        dev_root
            .join(repo_name(repo))
            .to_string_lossy()
            .into_owned(),
    );
    let path = jobs_dir.join(format!("{id}.json"));
    let text = serde_json::to_string_pretty(&job)
        .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))?;
    std::fs::write(&path, text).map_err(|e| CoreError::Io(format!("{}: {e}", path.display())))?;
    Ok(path)
}

/// The app's starter: asks AUTO-1, writes the job file to `jobs_dir` and runs
/// `fleet run <job file> --json` in the background, its stdout to `<id>.out` beside the job.
pub struct FleetStarter {
    pub fleet: FleetBridge,
    pub jobs_dir: PathBuf,
    pub dev_root: PathBuf,
}

impl JobStarter for FleetStarter {
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError> {
        super::may_spend(&self.fleet, lane)
    }

    fn start(&mut self, job: &Value, brief: &str) -> Result<(), CoreError> {
        let path = write_job(&self.jobs_dir, &self.dev_root, job, brief)?;
        let out_path = path.with_extension("out");
        let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", out_path.display()));
        let out = std::fs::File::create(&out_path).map_err(io)?;
        let job_file = path.to_string_lossy().into_owned();
        let command = self.fleet.command_line(&["run", &job_file]);
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .env(FLEET_ROOT_ENV, self.fleet.root())
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::null())
            .spawn()
            .map_err(io)?;
        // Reap the child when it ends; the poll never waits for the job.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}
