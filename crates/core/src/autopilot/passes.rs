//! AUTO-3: the autopilot's passes after each poll. `after_poll` loads the snapshots and the
//! autopilot's state file, runs each pass, saves the state and answers the job ids started.
//! A pass reaches the fleet only through a `JobStarter`: the app's is `FleetStarter`, the
//! tests' a fake.

use super::review::{first_rung, review_job, Rung};
use super::weekly::{roadmap_changed, weekly_due};
use crate::abilities::FleetModels;
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
    /// AUTO-5: when the weekly run last started.
    pub weekly_last_run: Option<UnixSeconds>,
    /// AUTO-5: repos whose drift clear has not started yet (braked), oldest first.
    pub drift_pending: Vec<String>,
    /// AUTO-2, AUTO-5: the id of the Project Manager job last started; no other starts until it ends.
    pub pm_running: Option<String>,
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
    /// Has the job started under `job_id` ended? `FleetStarter` says yes when the job has output,
    /// has no job file, or its job file is older than `PM_JOB_TIMEOUT_SECS` (a launcher that died
    /// leaves an empty output, and the queue must not wait for it forever).
    fn finished(&mut self, job_id: &str) -> Result<bool, CoreError>;
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

/// AUTO-2, AUTO-5: the lanes a Project Manager job may run on, in the order they are asked: the
/// local model first, then the smallest model with budget.
pub const PM_LANES: [&str; 5] = ["qwen", "gemini", "chatgpt", "deepseek", "claudeapi"];

/// AUTO-2, AUTO-5: the first lane of `PM_LANES` that may spend now (AUTO-1); `None` when none
/// may, so the job stays pending. A failed question is the error.
fn pm_lane(starter: &mut dyn JobStarter) -> Result<Option<&'static str>, CoreError> {
    for lane in PM_LANES {
        if starter.may_spend(lane)? {
            return Ok(Some(lane));
        }
    }
    Ok(None)
}

/// AUTO-2, AUTO-5: may a Project Manager job start? Not while the one last started has not
/// finished; once it has, that is forgotten.
fn pm_free(state: &mut AutopilotState, starter: &mut dyn JobStarter) -> Result<bool, CoreError> {
    if let Some(id) = state.pm_running.clone() {
        if !starter.finished(&id)? {
            return Ok(false);
        }
        state.pm_running = None;
    }
    Ok(true)
}

/// AUTO-2: the Project Manager job for `what` in `repo`, read at `stamp`, on `lane`.
pub fn pm_job(repo: &str, what: &str, stamp: UnixSeconds, lane: &str) -> Value {
    let name = repo.rsplit_once('/').map_or(repo, |(_, name)| name);
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
        "lane": lane,
        "brief": "open",
    })
}

/// AUTO-2: the Project Manager's prompt for `repo`: who it is, the `task`, and how it files.
pub fn pm_brief(repo: &str, task: &str) -> String {
    format!(
        "You are the Project Manager for {repo}.\n\
         {task}\n\
         File every new issue, label or comment through `fleet ghq add-batch` (never `gh issue create`), sized and labelled by RUBRIC.md, written in the owner's first person.\n"
    )
}

/// AUTO-2: the task of a job started because ROADMAP.md changed.
pub const ROADMAP_TASK: &str = "ROADMAP.md changed since the last poll. Read it, SPEC.md and the open issues, then file or adjust the issues the change needs.";

/// AUTO-2: start one Project Manager job for each repo whose ROADMAP.md changed since the text
/// last acted on, one at a time (`pm_running`) and at most one per call, on the first lane of
/// `PM_LANES` that may spend. While one runs, or when no lane may spend, none starts and the old
/// text is kept, so the change is tried at the next poll. A repo seen first is recorded and
/// starts nothing; archived repos are skipped.
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
        if roadmap_changed(&state.roadmaps[i..=i], std::slice::from_ref(&next)).is_empty() {
            continue;
        }
        if !started.is_empty() || !pm_free(state, starter)? {
            continue;
        }
        let Some(lane) = pm_lane(starter)? else {
            continue;
        };
        let job = pm_job(&snap.repo, "roadmap", snap.read_at, lane);
        starter.start(&job, &pm_brief(&snap.repo, ROADMAP_TASK))?;
        state.roadmaps[i] = next;
        if let Some(id) = job["id"].as_str() {
            state.pm_running = Some(id.to_string());
            started.push(id.to_string());
        }
    }
    Ok(started)
}

/// AUTO-5: the task of the weekly drift clear's Project Manager job.
pub const DRIFT_TASK: &str = "This is the weekly drift clear. Compare SPEC.md, ROADMAP.md, docs/code-map.md and the open issues with the code on main, then file or adjust an issue for each place they disagree.";

/// AUTO-5: when the weekly run is due (`weekly_due`), record it as taken now and queue each of
/// `repos` for a drift clear at the end of `drift_pending` (a repo already queued keeps its place).
/// Answers whether the week's run was taken; changes nothing when it was not due.
pub fn weekly_take(
    now: UnixSeconds,
    utc_offset_secs: i64,
    repos: &[String],
    state: &mut AutopilotState,
) -> bool {
    if !weekly_due(now, utc_offset_secs, state.weekly_last_run) {
        return false;
    }
    state.weekly_last_run = Some(now);
    for repo in repos {
        if !state.drift_pending.contains(repo) {
            state.drift_pending.push(repo.clone());
        }
    }
    true
}

/// AUTO-5: start the drift clear of the first pending repo, on the first lane of `PM_LANES`
/// that may spend. One at a time: nothing starts while the Project Manager job last started
/// (`pm_running`) has not finished, and the other repos stay pending in order for a later check.
/// When no lane may spend the repo stays pending. A failed question or start keeps every repo
/// pending and returns the error. Answers the job ids started (none or one).
pub fn drift_pass(
    now: UnixSeconds,
    state: &mut AutopilotState,
    starter: &mut dyn JobStarter,
) -> Result<Vec<String>, CoreError> {
    let mut started = Vec::new();
    let Some(repo) = state.drift_pending.first().cloned() else {
        return Ok(started);
    };
    if !pm_free(state, starter)? {
        return Ok(started);
    }
    let Some(lane) = pm_lane(starter)? else {
        return Ok(started);
    };
    let job = pm_job(&repo, "drift", now, lane);
    starter.start(&job, &pm_brief(&repo, DRIFT_TASK))?;
    state.drift_pending.remove(0);
    if let Some(id) = job["id"].as_str() {
        state.pm_running = Some(id.to_string());
        started.push(id.to_string());
    }
    Ok(started)
}

/// AUTO-5: what one weekly check decided: the repos to back up, the drift clear jobs started,
/// and the drift pass's error (the backup still runs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WeeklyRun {
    /// The repos to back up now (BAK-1), empty unless this check took the week's run.
    pub backup: Vec<String>,
    /// The drift clear job ids started.
    pub drift_started: Vec<String>,
    /// The drift pass's error, if it had one.
    pub drift_error: Option<CoreError>,
}

/// AUTO-5: one weekly check after a poll. The week's run, when due, queues every repo that is
/// not archived; the drift pass runs every time, so a braked repo starts at a later check. The
/// state is saved either way.
pub fn weekly_run(
    data_dir: &Path,
    now: UnixSeconds,
    utc_offset_secs: i64,
    starter: &mut dyn JobStarter,
) -> Result<WeeklyRun, CoreError> {
    let repos: Vec<String> = SnapshotStore::new(data_dir)
        .load_all()?
        .into_iter()
        .filter(|snap| !snap.archived)
        .map(|snap| snap.repo)
        .collect();
    let mut state = AutopilotState::load(data_dir)?;
    let mut run = WeeklyRun::default();
    if weekly_take(now, utc_offset_secs, &repos, &mut state) {
        run.backup = repos;
    }
    match drift_pass(now, &mut state, starter) {
        Ok(ids) => run.drift_started = ids,
        Err(e) => run.drift_error = Some(e),
    }
    state.save(data_dir)?;
    Ok(run)
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

/// Create the `.out` and `.err` files beside a job file (`<job id>.json`), for the launcher's
/// stdout and stderr. Answers them as (out, err).
pub fn job_log_files(job_path: &Path) -> Result<(std::fs::File, std::fs::File), CoreError> {
    let create = |p: PathBuf| {
        std::fs::File::create(&p).map_err(|e| CoreError::Io(format!("{}: {e}", p.display())))
    };
    Ok((
        create(job_path.with_extension("out"))?,
        create(job_path.with_extension("err"))?,
    ))
}

/// How long a job with no output may run before the queue stops waiting for it: 2 hours.
pub const PM_JOB_TIMEOUT_SECS: u64 = 7_200;

/// Has a job whose job file is `age_secs` old run past `PM_JOB_TIMEOUT_SECS`? Exactly at the limit is not.
pub fn job_timed_out(age_secs: u64) -> bool {
    age_secs > PM_JOB_TIMEOUT_SECS
}

/// The app's starter: AUTO-1 through the bridge, and `fleet run <job file> --json` in the
/// background. The launcher's stdout goes to `<job id>.out` and its stderr to `<job id>.err`,
/// beside the job file, so a crash leaves its reason on disk.
///
/// The lane `qwen` may also spend only when the local model server answers: `qwen_probe` is
/// asked with the `machines.<machine>.qwen_server_url` from the fleet config.
pub struct FleetStarter {
    pub fleet: FleetBridge,
    pub jobs_dir: PathBuf,
    pub dev_root: PathBuf,
    /// This machine's name, for the fleet config's `machines` table.
    pub machine: Option<String>,
    /// Does the model server at this URL answer? `http_qwen_probe` in the app; a fake in tests.
    pub qwen_probe: QwenProbe,
}

/// A question to the local model server: does it answer at this URL?
pub type QwenProbe = Box<dyn Fn(&str) -> bool + Send>;

/// How long the model server gets to answer a probe, in seconds.
pub const QWEN_PROBE_SECS: u64 = 4;

/// The real probe: an HTTP GET with a `QWEN_PROBE_SECS` timeout. Any answer below 500 counts as
/// up; a refused or timed-out connection or a server error is down.
pub fn http_qwen_probe(url: &str) -> bool {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(QWEN_PROBE_SECS))
        .build();
    match agent.get(url).call() {
        Ok(_) => true,
        Err(ureq::Error::Status(code, _)) => code < 500,
        Err(_) => false,
    }
}

/// Does the local model server of `machine` answer? `false` when the fleet config has no
/// `qwen_server_url` for it.
pub fn qwen_server_up(fleet: &FleetBridge, machine: Option<&str>, probe: &QwenProbe) -> bool {
    FleetModels::read_qwen_server_url(fleet, machine).is_some_and(|url| probe(&url))
}

impl JobStarter for FleetStarter {
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError> {
        if !super::may_spend(&self.fleet, lane)? {
            return Ok(false);
        }
        if lane == "qwen" {
            return Ok(qwen_server_up(
                &self.fleet,
                self.machine.as_deref(),
                &self.qwen_probe,
            ));
        }
        Ok(true)
    }

    fn start(&mut self, job: &Value, brief: &str) -> Result<(), CoreError> {
        let path = write_job(&self.jobs_dir, &self.dev_root, job, brief)?;
        let (out, err) = job_log_files(&path)?;
        let job_file = path.to_string_lossy().into_owned();
        let command = self.fleet.command_line(&["run", &job_file]);
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .env(FLEET_ROOT_ENV, self.fleet.root().as_os_str())
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .spawn()
            .map_err(|e| CoreError::Io(format!("{}: {e}", command[0])))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    fn finished(&mut self, job_id: &str) -> Result<bool, CoreError> {
        let job_file = self.jobs_dir.join(format!("{job_id}.json"));
        if !job_file.exists() {
            return Ok(true);
        }
        let out = self.jobs_dir.join(format!("{job_id}.out"));
        if std::fs::metadata(out).is_ok_and(|m| m.len() > 0) {
            return Ok(true);
        }
        let age = std::fs::metadata(&job_file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map_or(0, |d| d.as_secs());
        Ok(job_timed_out(age))
    }
}
