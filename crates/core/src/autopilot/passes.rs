//! AUTO-3: the autopilot's passes after each poll. `after_poll` loads the snapshots and the
//! autopilot's state file, runs each pass, saves the state and answers the job ids started.
//! A pass reaches the fleet only through a `JobStarter`: the app's is `FleetStarter`, the
//! tests' a fake.

use super::review::{first_rung, review_job, Rung};
use super::weekly::roadmap_changed;
use crate::clock::UnixSeconds;
use crate::fleet::{FleetBridge, FLEET_ROOT_ENV};
use crate::github::merge::merge_passed;
use crate::github::poll::Poller;
use crate::github::snapshot::{RepoSnapshot, SnapshotStore};
use crate::rules::{Caller, Face, Role};
use crate::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The autopilot's state file in the app data folder.
pub const STATE_FILE: &str = "autopilot.json";

/// The services a review may run on, in the order they are asked (R25: never the builder's).
pub const REVIEW_LANES: [&str; 3] = ["chatgpt", "gemini", "claudeapi"];

/// What the passes remember between polls.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutopilotState {
    /// `owner/name#pr` of every review started, oldest first.
    pub reviews_started: Vec<String>,
    /// AUTO-2: each repo's ROADMAP.md as last acted on, in the order first seen.
    pub roadmaps: Vec<(String, Option<String>)>,
}

impl AutopilotState {
    /// Read `<data_dir>/autopilot.json`; a missing file is the default state.
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

    /// Write `<data_dir>/autopilot.json` as pretty JSON, making the folder.
    pub fn save(&self, data_dir: &Path) -> Result<(), CoreError> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| CoreError::Io(format!("{}: {e}", data_dir.display())))?;
        let path = data_dir.join(STATE_FILE);
        let text =
            serde_json::to_string_pretty(self).map_err(|e| CoreError::Parse(e.to_string()))?;
        std::fs::write(&path, text).map_err(|e| CoreError::Io(format!("{}: {e}", path.display())))
    }
}

/// The seam between a pass and the fleet.
pub trait JobStarter {
    /// AUTO-1: may `lane` spend now?
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError>;
    /// Start `job` with `brief` as its prompt. Does not wait for the job.
    fn start(&mut self, job: &Value, brief: &str) -> Result<(), CoreError>;
}

/// The fleet's canonical machine name: trimmed, lower case, old names mapped.
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

/// This machine's name: a non-blank `fleet_machine` (the `FLEET_MACHINE` env), else the first
/// line of `<home>/.config/software-depot/machine`; `None` when neither names one.
pub fn this_machine(fleet_machine: Option<&str>, home: &Path) -> Option<String> {
    if let Some(name) = fleet_machine.filter(|n| !n.trim().is_empty()) {
        return Some(canon_machine(name));
    }
    let path = home.join(".config").join("software-depot").join("machine");
    let text = std::fs::read_to_string(path).ok()?;
    let first = text.lines().next()?.trim();
    if first.is_empty() {
        None
    } else {
        Some(canon_machine(first))
    }
}

/// The lane of the service that built a PR, from its first `model:` label.
pub fn builder_lane(labels: &[String]) -> Option<&'static str> {
    let model = labels
        .iter()
        .find_map(|l| l.strip_prefix("model:"))?
        .trim()
        .to_lowercase();
    const LANES: [(&str, &str); 9] = [
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
    LANES
        .iter()
        .find(|(prefix, _)| model.starts_with(prefix))
        .map(|(_, lane)| *lane)
}

/// `text` safe for a job id: each character outside `A-Z a-z 0-9 . _ -` becomes `-`.
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

/// The fleet size letter for a rung: Easy `S`, Medium `M`, anything else `L`.
pub fn size_letter(rung: Rung) -> &'static str {
    match rung {
        Rung::Easy => "S",
        Rung::Medium => "M",
        _ => "L",
    }
}

/// The full fleet job for try 1 of the review of `repo#pr`, on the `reviewer` lane.
pub fn review_fleet_job(
    repo: &str,
    pr: u64,
    issue: u64,
    rung: Rung,
    builder: &str,
    reviewer: &str,
) -> Value {
    let mut job = review_job(repo, pr, issue, rung, 1, builder);
    let name = repo.split_once('/').map_or(repo, |(_, name)| name);
    let extra = json!({
        "id": format!("review-{}-{pr}", id_part(name)),
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

/// The review's prompt: the PR, its head, the issue and the verdict line.
pub fn review_brief(repo: &str, pr: u64, issue: u64, head_sha: &str) -> String {
    format!(
        "Review PR #{pr} in {repo} at head {head_sha} against issue #{issue}.\n\
         Read the PR's diff and the issue's body. Check every Expectation and every Done when line, and that the PR changed no locked test.\n\
         Post one comment on the PR. Its last line is exactly `VERDICT: PASS {head_sha}` or `VERDICT: FAIL {head_sha}`.\n"
    )
}

/// AUTO-3: start the one review of each open PR `machine` opened that has no verdict yet and
/// closes an issue, on the first review lane that is not the builder's and may spend.
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
        for pull in pulls {
            let key = format!("{}#{}", snap.repo, pull.number);
            if pull.state != "open"
                || !pull.labels.contains(&by)
                || !pull.verdicts.is_empty()
                || state.reviews_started.contains(&key)
            {
                continue;
            }
            let Some(&issue) = pull.closes.first() else {
                continue;
            };
            let size = snap
                .issues
                .iter()
                .find(|i| i.number == issue)
                .and_then(|i| i.labels.iter().find_map(|l| l.strip_prefix("size:")))
                .unwrap_or("");
            let rung = first_rung(size);
            let builder = builder_lane(&pull.labels).unwrap_or("");
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
            let job = review_fleet_job(&snap.repo, pull.number, issue, rung, builder, reviewer);
            let brief = review_brief(&snap.repo, pull.number, issue, &pull.head_sha);
            starter.start(&job, &brief)?;
            state.reviews_started.push(key);
            if let Some(id) = job["id"].as_str() {
                started.push(id.to_string());
            }
        }
    }
    Ok(started)
}

/// AUTO-2: the lane a Project Manager job runs on (`judgement-planning`'s first lane; the router's pick, FD-3, replaces it when it lands).
pub const PM_LANE: &str = "claudeapi";

/// A Project Manager job for `repo`, started for `what` (e.g. `roadmap`) at `stamp`.
pub fn pm_job(repo: &str, what: &str, stamp: UnixSeconds) -> Value {
    let name = repo.rsplit('/').next().unwrap_or(repo);
    json!({
        "id": format!("pm-{what}-{}-{stamp}", id_part(name)),
        "title": format!("Project Manager: {what} for {repo}"),
        "kind": "judgement-planning",
        "size": "M",
        "repo": repo,
        "base": "main",
        "est_minutes": 30,
        "no_issue": format!("Project Manager job ({what})"),
        "verify": [],
        "files_allowed": [],
        "lane": PM_LANE,
        "brief": "open",
    })
}

/// A Project Manager job's prompt: who it is, the task, and how it files.
pub fn pm_brief(repo: &str, task: &str) -> String {
    format!(
        "You are the Project Manager for {repo}.\n\
         {task}\n\
         File every new issue, label or comment through `fleet ghq add-batch` (never `gh issue create`), sized and labelled by RUBRIC.md, written in the owner's first person.\n"
    )
}

/// AUTO-2: the task of a Project Manager job started by a ROADMAP.md change.
pub const ROADMAP_TASK: &str = "ROADMAP.md changed since the last poll. Read it, SPEC.md and the open issues, then file or adjust the issues the change needs.";

/// AUTO-2: start one Project Manager job for each repo whose ROADMAP.md changed since the text
/// last acted on. A repo seen for the first time is recorded and starts nothing; a braked pool
/// starts none and keeps the old text, so the change is tried again at the next poll.
pub fn roadmap_pass(
    snaps: &[RepoSnapshot],
    state: &mut AutopilotState,
    starter: &mut dyn JobStarter,
) -> Result<Vec<String>, CoreError> {
    let mut started = Vec::new();
    for snap in snaps.iter().filter(|s| !s.archived) {
        let next = (snap.repo.clone(), snap.roadmap_md.clone());
        let Some(i) = state
            .roadmaps
            .iter()
            .position(|(repo, _)| *repo == snap.repo)
        else {
            state.roadmaps.push(next);
            continue;
        };
        let pair = [next];
        if roadmap_changed(&state.roadmaps[i..=i], &pair).is_empty() {
            continue;
        }
        if !starter.may_spend(PM_LANE)? {
            continue;
        }
        let job = pm_job(&snap.repo, "roadmap", snap.read_at);
        starter.start(&job, &pm_brief(&snap.repo, ROADMAP_TASK))?;
        let [next] = pair;
        state.roadmaps[i] = next;
        if let Some(id) = job["id"].as_str() {
            started.push(id.to_string());
        }
    }
    Ok(started)
}

/// What `after_poll` needs: the app data folder, this machine's name and the fleet seam.
pub struct AfterPoll<'a> {
    pub data_dir: &'a Path,
    pub machine: Option<&'a str>,
    pub starter: &'a mut dyn JobStarter,
}

/// AUTO-3: the autopilot's passes after a poll. The state is saved even when a pass failed,
/// so a review already started is not started again.
pub fn after_poll(ctx: &mut AfterPoll<'_>) -> Result<Vec<String>, CoreError> {
    let snaps = SnapshotStore::new(ctx.data_dir).load_all()?;
    let mut state = AutopilotState::load(ctx.data_dir)?;
    let result = run_passes(&snaps, ctx.machine, &mut state, &mut *ctx.starter);
    state.save(ctx.data_dir)?;
    result
}

/// Every pass, in order; later passes extend `started`.
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
    started.extend(roadmap_pass(snaps, state, starter)?);
    Ok(started)
}

/// One repo's merges in a `merge_pass`: the repo, the PRs merged, the issues they close.
pub type Merged = (String, Vec<u64>, Vec<u64>);

/// The caller of the autopilot's merges: the app, as the Project Manager (R32).
pub fn autopilot_caller() -> Caller {
    Caller {
        face: Face::Gui,
        role: Some(Role::ProjectManager),
        in_job_worktree: false,
        confirmed_in_app: false,
    }
}

/// AUTO-4: merge every passed PR of each repo that has a clone beside the pipeline repo, in the
/// order given. Archived repos and repos with no clone are skipped. A refused merge returns at once.
pub fn merge_pass(
    snaps: &[RepoSnapshot],
    poller: &Poller<'_>,
    pipeline_root: &Path,
    dev_root: &Path,
) -> Result<Vec<Merged>, CoreError> {
    let caller = autopilot_caller();
    let mut out = Vec::new();
    for snap in snaps.iter().filter(|s| !s.archived) {
        let name = snap.repo.rsplit('/').next().unwrap_or(&snap.repo);
        let clone = dev_root.join(name);
        if !clone.is_dir() {
            continue;
        }
        let (merged, closed) = merge_passed(
            &caller,
            &clone,
            pipeline_root,
            &snap.repo,
            &snap.pulls,
            &|_| false,
            poller,
        )?;
        if !merged.is_empty() {
            out.push((snap.repo.clone(), merged, closed));
        }
    }
    Ok(out)
}

/// Write `job` and its brief for `fleet run`: the brief to `<jobs_dir>/<id>.md`, the job to
/// `<jobs_dir>/<id>.json` with `prompt_file` and `clone` (`<dev_root>/<name>`) set. Answers
/// the job file's path.
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
    let name = repo.split_once('/').map_or(repo, |(_, name)| name);
    std::fs::create_dir_all(jobs_dir)
        .map_err(|e| CoreError::Io(format!("{}: {e}", jobs_dir.display())))?;
    let brief_path = jobs_dir.join(format!("{id}.md"));
    std::fs::write(&brief_path, brief)
        .map_err(|e| CoreError::Io(format!("{}: {e}", brief_path.display())))?;
    let mut full = job.clone();
    if let Some(obj) = full.as_object_mut() {
        obj.insert(
            "prompt_file".into(),
            Value::String(brief_path.to_string_lossy().into_owned()),
        );
        obj.insert(
            "clone".into(),
            Value::String(dev_root.join(name).to_string_lossy().into_owned()),
        );
    }
    let job_path = jobs_dir.join(format!("{id}.json"));
    let text = serde_json::to_string_pretty(&full).map_err(|e| CoreError::Parse(e.to_string()))?;
    std::fs::write(&job_path, text)
        .map_err(|e| CoreError::Io(format!("{}: {e}", job_path.display())))?;
    Ok(job_path)
}

/// The app's starter: AUTO-1 through the bridge, and `fleet run <job file> --json` in the
/// background.
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
        let out = std::fs::File::create(&out_path)
            .map_err(|e| CoreError::Io(format!("{}: {e}", out_path.display())))?;
        let job_file = path.to_string_lossy().into_owned();
        let command = self.fleet.command_line(&["run", &job_file]);
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .env(FLEET_ROOT_ENV, self.fleet.root().as_os_str())
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| CoreError::Io(format!("{}: {e}", command[0])))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}
