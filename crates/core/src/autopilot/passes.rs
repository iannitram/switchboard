//! AUTO-3: the autopilot's passes after each poll. `after_poll` loads the snapshots and the
//! autopilot's state file, runs each pass, saves the state and answers the job ids started.
//! A pass reaches the fleet only through a `JobStarter`: the app's is `FleetStarter`, the
//! tests' a fake.

use super::review::{first_rung, review_job, Rung};
use super::weekly::{roadmap_changed, weekly_due};
use crate::abilities::FleetModels;
use crate::clock::UnixSeconds;
use crate::fleet::{clean_child_env, FleetBridge, FleetError, FleetErrorReason, FLEET_ROOT_ENV};
use crate::github::merge::{
    carried_over_by_patch_id, eligibility, merge, MergeEligibility, MergeJob,
};
use crate::github::poll::Poller;
use crate::github::snapshot::{PullRequest, RepoSnapshot, SnapshotStore};
use crate::github::transport::GitHubTransport;
use crate::rules::{Caller, Face, Role};
use crate::waiting::{Item, LocalItems, Source};
use crate::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

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

    /// Write `<data_dir>/autopilot.json` as pretty JSON, making the folder. Written whole
    /// (`lock::write_whole`, #431): the status read, which takes no lock, finds the old state
    /// or the new one, never half of one.
    pub fn save(&self, data_dir: &Path) -> Result<(), CoreError> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| CoreError::Io(format!("{}: {e}", data_dir.display())))?;
        let path = data_dir.join(STATE_FILE);
        let text =
            serde_json::to_string_pretty(self).map_err(|e| CoreError::Parse(e.to_string()))?;
        crate::lock::write_whole(&path, text.as_bytes())
    }
}

/// The seam between a pass and the fleet.
pub trait JobStarter {
    /// AUTO-1: may `lane` spend now?
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError>;
    /// Start `job` with `brief` as its prompt. Does not wait for the job.
    fn start(&mut self, job: &Value, brief: &str) -> Result<(), CoreError>;
    /// Has the job started under `job_id` ended? `FleetStarter` says yes when the job has output,
    /// has no job file, its launcher has exited 0 or 75 ("not now"), or its job file is older than
    /// `PM_JOB_TIMEOUT_SECS`. An error means the job has ended with one: its launcher exited
    /// non-zero with no output (CORE-2, #379), or a Project Manager launcher said 75 again
    /// after its one retry (#432); the caller frees the job's slot and passes the error on.
    fn finished(&mut self, job_id: &str) -> Result<bool, CoreError>;
    /// `finished` for a read that must change nothing (a dry run, the status read, #385): the
    /// same answer with nothing cleaned up. `FleetStarter` leaves a failed job's worktree alone.
    fn peek_finished(&mut self, job_id: &str) -> Result<bool, CoreError> {
        self.finished(job_id)
    }
}

/// A dry run's starter (#385): it asks the real one whether a lane may spend and whether a job
/// has ended (`peek_finished`), and starts nothing.
pub struct DryStarter<'a> {
    pub inner: &'a mut dyn JobStarter,
}

impl JobStarter for DryStarter<'_> {
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError> {
        self.inner.may_spend(lane)
    }

    fn start(&mut self, _job: &Value, _brief: &str) -> Result<(), CoreError> {
        Ok(())
    }

    fn finished(&mut self, job_id: &str) -> Result<bool, CoreError> {
        self.inner.peek_finished(job_id)
    }
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
/// finished; once it has, that is forgotten. A job that ended with an error (`finished` answers
/// it) is forgotten too, and its error is returned, so the slot is free at the next pass (#379).
fn pm_free(state: &mut AutopilotState, starter: &mut dyn JobStarter) -> Result<bool, CoreError> {
    if let Some(id) = state.pm_running.clone() {
        match starter.finished(&id) {
            Ok(false) => return Ok(false),
            Ok(true) => state.pm_running = None,
            Err(e) => {
                state.pm_running = None;
                return Err(e);
            }
        }
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
    weekly_run_with(data_dir, now, utc_offset_secs, starter, false)
}

/// `weekly_run`, or with `dry_run` what it would do (#385): the same checks through a
/// `DryStarter`, no job started and the state file left as it is.
pub fn weekly_run_with(
    data_dir: &Path,
    now: UnixSeconds,
    utc_offset_secs: i64,
    starter: &mut dyn JobStarter,
    dry_run: bool,
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
    let drift = if dry_run {
        drift_pass(now, &mut state, &mut DryStarter { inner: starter })
    } else {
        drift_pass(now, &mut state, starter)
    };
    match drift {
        Ok(ids) => run.drift_started = ids,
        Err(e) => run.drift_error = Some(e),
    }
    if !dry_run {
        state.save(data_dir)?;
    }
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
    let which = PollPasses {
        review: true,
        roadmap: true,
        dry_run: false,
    };
    let out = after_poll_with(ctx, which)?;
    match out.review_error.or(out.roadmap_error) {
        Some(e) => Err(e),
        None => Ok(out
            .review_started
            .into_iter()
            .chain(out.roadmap_started)
            .collect()),
    }
}

/// Which of `after_poll`'s passes to run (#385), and whether as a dry run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollPasses {
    /// AUTO-3: the review pass.
    pub review: bool,
    /// AUTO-2: the roadmap pass.
    pub roadmap: bool,
    /// Start nothing and leave the state file as it is; the ids are what would start.
    pub dry_run: bool,
}

/// What `after_poll_with` did: each pass's job ids, and the error that stopped a pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PollOutcome {
    pub review_started: Vec<String>,
    pub roadmap_started: Vec<String>,
    /// The review pass's error; the roadmap pass then did not run.
    pub review_error: Option<CoreError>,
    pub roadmap_error: Option<CoreError>,
}

/// `after_poll` for the passes `which` names (#385): the window runs both, a headless run one.
/// The outer error is a snapshot or state file that does not read, or a state that does not
/// save. A dry run goes through a `DryStarter` and saves nothing.
pub fn after_poll_with(
    ctx: &mut AfterPoll<'_>,
    which: PollPasses,
) -> Result<PollOutcome, CoreError> {
    let snaps = SnapshotStore::new(ctx.data_dir).load_all()?;
    let mut state = AutopilotState::load(ctx.data_dir)?;
    let mut out = PollOutcome::default();
    if which.dry_run {
        let mut dry = DryStarter {
            inner: &mut *ctx.starter,
        };
        run_passes(&snaps, ctx.machine, &mut state, &mut dry, which, &mut out);
        return Ok(out);
    }
    run_passes(
        &snaps,
        ctx.machine,
        &mut state,
        &mut *ctx.starter,
        which,
        &mut out,
    );
    state.save(ctx.data_dir)?;
    Ok(out)
}

/// The passes `which` names, in order: the review pass (only with a machine name), then the
/// roadmap pass. A review pass that failed stops them; each error is kept in `out`.
fn run_passes(
    snaps: &[RepoSnapshot],
    machine: Option<&str>,
    state: &mut AutopilotState,
    starter: &mut dyn JobStarter,
    which: PollPasses,
    out: &mut PollOutcome,
) {
    if let (true, Some(machine)) = (which.review, machine) {
        match review_pass(snaps, machine, state, starter) {
            Ok(ids) => out.review_started = ids,
            Err(e) => {
                out.review_error = Some(e);
                return;
            }
        }
    }
    if which.roadmap {
        match roadmap_pass(snaps, state, starter) {
            Ok(ids) => out.roadmap_started = ids,
            Err(e) => out.roadmap_error = Some(e),
        }
    }
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

/// One merge the autopilot tried and was refused (#380): the repo, the PR and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefusedMerge {
    pub repo: String,
    pub pr: u64,
    pub why: String,
}

/// What one `merge_pass` did: each repo's merges, and each merge that was refused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergePass {
    pub merged: Vec<Merged>,
    pub refused: Vec<RefusedMerge>,
}

/// AUTO-4: merge every passed PR of each repo that has a clone beside the pipeline repo, in the
/// order given, smallest PR first in each repo. A PASS on an older sha counts when it carries
/// over by patch-id (`carried_over_by_patch_id`, #381). Archived repos and repos with no clone
/// are skipped. A refused merge is recorded in `refused` and the pass goes on to the next PR, so
/// one refusal neither hides the merges before it nor stops the ones after it (#380).
pub fn merge_pass(
    snaps: &[RepoSnapshot],
    poller: &Poller<'_>,
    pipeline_root: &Path,
    dev_root: &Path,
) -> MergePass {
    let caller = autopilot_caller();
    let mut out = MergePass::default();
    for (snap, clone, ready) in ready_by_repo(snaps, dev_root) {
        let (mut merged, mut closed) = (Vec::new(), Vec::new());
        for (pr, carried) in ready {
            let job = MergeJob {
                pipeline_root,
                clone: &clone,
                repo: &snap.repo,
                pr,
                carried_over: carried,
            };
            match merge(&caller, &job, poller) {
                Ok(()) => {
                    merged.push(pr.number);
                    closed.extend(pr.closes.iter().copied());
                }
                Err(e) => out.refused.push(RefusedMerge {
                    repo: snap.repo.clone(),
                    pr: pr.number,
                    why: match e {
                        CoreError::Refused(why) => why,
                        other => other.to_string(),
                    },
                }),
            }
        }
        if !merged.is_empty() {
            out.merged.push((snap.repo.clone(), merged, closed));
        }
    }
    out
}

/// One repo of a `merge_pass`: its snapshot, its clone, and its PRs to try, each with its
/// patch-id answer.
type ReadyRepo<'a> = (&'a RepoSnapshot, PathBuf, Vec<(&'a PullRequest, bool)>);

/// The PRs a `merge_pass` would try: each repo that is not archived and has a clone under
/// `dev_root`, with its open, eligible PRs that are not drafts (and their patch-id answer),
/// smallest number first.
fn ready_by_repo<'a>(snaps: &'a [RepoSnapshot], dev_root: &Path) -> Vec<ReadyRepo<'a>> {
    let mut out = Vec::new();
    for snap in snaps.iter().filter(|s| !s.archived) {
        let name = snap.repo.rsplit('/').next().unwrap_or(&snap.repo);
        let clone = dev_root.join(name);
        if !clone.is_dir() {
            continue;
        }
        let mut ready: Vec<(&PullRequest, bool)> = snap
            .pulls
            .iter()
            // #432 (ruling 7): a draft is not offered for merging; it is skipped, not refused.
            .filter(|pr| pr.state == "open" && !pr.draft)
            .map(|pr| (pr, carried_over_by_patch_id(&clone, pr)))
            .filter(|(pr, carried)| eligibility(pr, *carried) == MergeEligibility::Eligible)
            .collect();
        ready.sort_by_key(|(pr, _)| pr.number);
        out.push((snap, clone, ready));
    }
    out
}

/// AUTO-4 as a dry run (#385): what a `merge_pass` would try to merge now, in its order, with
/// nothing merged: each repo, its PRs and the issues they close.
pub fn merge_would(snaps: &[RepoSnapshot], dev_root: &Path) -> Vec<Merged> {
    ready_by_repo(snaps, dev_root)
        .into_iter()
        .filter(|(_, _, ready)| !ready.is_empty())
        .map(|(snap, _, ready)| {
            let prs = ready.iter().map(|(pr, _)| pr.number).collect();
            let closes = ready
                .iter()
                .flat_map(|(pr, _)| pr.closes.iter().copied())
                .collect();
            (snap.repo.clone(), prs, closes)
        })
        .collect()
}

/// WAIT-1, AUTO-4 (#380): the Waiting item of a refused autopilot merge. One per PR (a later
/// refusal of the same PR replaces it); its link opens the PR.
pub fn refused_merge_item(refused: &RefusedMerge, at: UnixSeconds) -> Item {
    let what = format!("Merge {}#{}", refused.repo, refused.pr);
    Item {
        what: what.clone(),
        why: format!(
            "The autopilot's merge was refused: {}. Fix it, or merge it by hand.",
            refused.why
        ),
        source: Source::GuardedAction { text: what },
        link: format!("https://github.com/{}/pull/{}", refused.repo, refused.pr),
        action: "Open PR".to_string(),
        since: at,
    }
}

/// Put each refused merge of a pass in Waiting on you (`<data>/waiting/local.json`). #432
/// (ruling 5): a PR refused again keeps the `since` of its first refusal; the reason is the
/// latest.
pub fn record_refused_merges(
    data_dir: &Path,
    refused: &[RefusedMerge],
    at: UnixSeconds,
) -> Result<(), CoreError> {
    if refused.is_empty() {
        return Ok(());
    }
    // One change under the file's lock (#431): the `since` kept is the one there now.
    LocalItems::new(data_dir).change(|items| {
        for r in refused {
            let mut item = refused_merge_item(r, at);
            match items.iter_mut().find(|old| old.source == item.source) {
                Some(old) => {
                    item.since = old.since;
                    *old = item;
                }
                None => items.push(item),
            }
        }
        true
    })?;
    Ok(())
}

/// #432 (ruling 6): take out of Waiting on you the item of each refused merge whose PR has
/// merged or closed since: merged by this pass (`merged`), or `merged` or `closed` in its
/// repo's snapshot. Answers the PRs cleared, as `(repo, pr)`; with none, nothing is written.
pub fn clear_settled_merges(
    data_dir: &Path,
    snaps: &[RepoSnapshot],
    merged: &[Merged],
) -> Result<Vec<(String, u64)>, CoreError> {
    let mut settled: Vec<(String, u64)> = Vec::new();
    for snap in snaps {
        for pr in snap.pulls.iter().filter(|pr| pr.state != "open") {
            settled.push((snap.repo.clone(), pr.number));
        }
    }
    for (repo, prs, _) in merged {
        settled.extend(prs.iter().map(|pr| (repo.clone(), *pr)));
    }
    settled.sort();
    settled.dedup();
    let mut cleared = Vec::new();
    let local = LocalItems::new(data_dir);
    let merge_item = |item: &Item| {
        settled.iter().any(|(repo, pr)| {
            item.source
                == Source::GuardedAction {
                    text: format!("Merge {repo}#{pr}"),
                }
        })
    };
    if !local.load()?.iter().any(merge_item) {
        return Ok(cleared);
    }
    // One change under the file's lock (#431); nothing cleared writes nothing.
    local.change(|items| {
        for (repo, pr) in settled {
            let source = Source::GuardedAction {
                text: format!("Merge {repo}#{pr}"),
            };
            let before = items.len();
            items.retain(|item| item.source != source);
            if items.len() != before {
                cleared.push((repo, pr));
            }
        }
        !cleared.is_empty()
    })?;
    Ok(cleared)
}

/// #433: the poll reads only the 50 most recently updated PRs, so a refused PR that drops off
/// that list never shows as merged or closed in a snapshot. Look up, one by one through
/// `transport`, each Waiting merge item whose PR is in no snapshot, and take out the ones that
/// merged or closed. A lookup that fails or answers anything else leaves the item. At most
/// `VANISHED_LOOKUPS_PER_PASS` are looked up per pass; with more, the window of items looked up
/// starts at `(minute of now * 5) % count`. Passes that run at varying minutes reach every item in
/// turn, but a run on an exact fixed cadence (a headless run every N minutes) can keep landing on
/// the same starts and miss some items for good. Answers the PRs cleared, as `(repo, pr)`; with
/// none, nothing is written.
pub fn clear_vanished_merges(
    data_dir: &Path,
    snaps: &[RepoSnapshot],
    transport: &dyn GitHubTransport,
    now: UnixSeconds,
) -> Result<Vec<(String, u64)>, CoreError> {
    let local = LocalItems::new(data_dir);
    let mut vanished: Vec<(Source, String, u64)> = Vec::new();
    for item in local.load()? {
        let Source::GuardedAction { text } = &item.source else {
            continue;
        };
        let Some((repo, pr)) = text
            .strip_prefix("Merge ")
            .and_then(|rest| rest.rsplit_once('#'))
            .and_then(|(repo, n)| Some((repo.to_string(), n.parse::<u64>().ok()?)))
        else {
            continue;
        };
        let polled = snaps
            .iter()
            .any(|s| s.repo == repo && s.pulls.iter().any(|p| p.number == pr));
        if !polled {
            vanished.push((item.source.clone(), repo, pr));
        }
    }
    let count = vanished.len();
    let start = if count > VANISHED_LOOKUPS_PER_PASS {
        let minute = usize::try_from(now.max(0) / 60).unwrap_or(0);
        minute.wrapping_mul(VANISHED_LOOKUPS_PER_PASS) % count
    } else {
        0
    };
    let mut cleared = Vec::new();
    for (source, repo, pr) in vanished
        .into_iter()
        .cycle()
        .skip(start)
        .take(count.min(VANISHED_LOOKUPS_PER_PASS))
    {
        let path = format!("/repos/{repo}/pulls/{pr}");
        let settled = matches!(
            transport.rest_get(&path, None),
            Ok(a) if a.status == 200 && a.body["state"].as_str() == Some("closed")
        );
        if settled && local.remove(&source)? {
            cleared.push((repo, pr));
        }
    }
    Ok(cleared)
}

/// #433: how many PRs `clear_vanished_merges` looks up in one pass at most.
pub const VANISHED_LOOKUPS_PER_PASS: usize = 5;

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

/// The file beside a job file (`<job id>.exit`) where the app's starter writes the launcher's
/// exit code when it ends (`signal` when it had none), so a later pass can tell a launcher that
/// died from one still running (#379).
pub fn job_exit_file(job_path: &Path) -> PathBuf {
    job_path.with_extension("exit")
}

/// How long a job with no output may run before the queue stops waiting for it: 2 hours.
pub const PM_JOB_TIMEOUT_SECS: u64 = 7_200;

/// Has a job whose job file is `age_secs` old run past `PM_JOB_TIMEOUT_SECS`? Exactly at the limit is not.
pub fn job_timed_out(age_secs: u64) -> bool {
    age_secs > PM_JOB_TIMEOUT_SECS
}

/// A worktree and branch that a failed Project Manager job left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmCleanup {
    pub worktree: PathBuf,
    pub branch: String,
}

/// Should the output of a finished `fleet run --json` job be cleaned up, and what? Yes only when
/// `outcome` is `infra`, `pushed` is false, the worktree folder's name starts with `fleet-pm-`
/// and `branch` starts with `fleet/pm-`; a missing or mistyped field is a no.
pub fn pm_cleanup_target(output: &Value) -> Option<PmCleanup> {
    if output.get("outcome")?.as_str()? != "infra" || output.get("pushed")?.as_bool()? {
        return None;
    }
    let worktree = PathBuf::from(output.get("worktree")?.as_str()?);
    let branch = output.get("branch")?.as_str()?;
    let folder = worktree.file_name()?.to_str()?;
    if !folder.starts_with("fleet-pm-") || !branch.starts_with("fleet/pm-") {
        return None;
    }
    Some(PmCleanup {
        worktree,
        branch: branch.to_string(),
    })
}

/// Removes the worktree and branch in `target` with git, run from the worktree's main clone.
/// A worktree folder that does not exist is skipped; every failure is ignored.
pub fn remove_pm_worktree(target: &PmCleanup) {
    if !target.worktree.is_dir() {
        return;
    }
    let git = |dir: &Path, args: &[&str]| {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        cmd.stdin(Stdio::null());
        cmd.output().ok().filter(|o| o.status.success())
    };
    let Some(common) = git(&target.worktree, &["rev-parse", "--git-common-dir"]) else {
        return;
    };
    let common = PathBuf::from(String::from_utf8_lossy(&common.stdout).trim());
    let common = if common.is_absolute() {
        common
    } else {
        target.worktree.join(common)
    };
    let Some(clone) = common.parent().map(Path::to_path_buf) else {
        return;
    };
    let worktree = target.worktree.to_string_lossy().into_owned();
    let _ = git(&clone, &["worktree", "remove", "--force", &worktree]);
    let _ = git(&clone, &["branch", "-D", &target.branch]);
}

/// The app's starter: AUTO-1 through the bridge, and `fleet run <job file> --json` in the
/// background. The launcher's stdout goes to `<job id>.out`, its stderr to `<job id>.err` and,
/// when it ends, its exit code to `<job id>.exit`, beside the job file, so a crash leaves its
/// reason on disk and frees the Project Manager's slot (#379).
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
    /// #432 (ruling 9): how long a Project Manager launcher that said "not now" (exit 75) with
    /// no output waits before its one retry. `PM_RETRY_WAIT` in the app; none in tests.
    pub retry_wait: Duration,
}

/// #432 (ruling 9): the wait before the one retry of a Project Manager launcher that exited 75
/// with no output.
pub const PM_RETRY_WAIT: Duration = Duration::from_secs(30);

/// Is `job_id` a Project Manager job (`pm_job`)? Only its launcher is retried after a 75.
pub fn is_pm_job(job_id: &str) -> bool {
    job_id.starts_with("pm-")
}

/// The file beside a job file (`<job id>.retry`) written when its launcher said "not now" with
/// no output and was started once more (#432, ruling 9): a 75 after it is a failure.
pub fn job_retry_file(job_path: &Path) -> PathBuf {
    job_path.with_extension("retry")
}

/// Start `command` with stdout and stderr to the files given.
fn launch(
    command: &[String],
    fleet_root: &std::ffi::OsStr,
    out: std::fs::File,
    err: std::fs::File,
) -> std::io::Result<std::process::Child> {
    let mut cmd = Command::new(&command[0]);
    cmd.args(&command[1..]);
    clean_child_env(&mut cmd);
    cmd.env(FLEET_ROOT_ENV, fleet_root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
}

/// Wait for `child`; its exit code, `None` when a signal ended it.
fn exit_code(mut child: std::process::Child) -> Option<i32> {
    child.wait().ok().and_then(|status| status.code())
}

/// A question to the local model server: does it answer at this URL?
pub type QwenProbe = Box<dyn Fn(&str) -> bool + Send>;

/// How long the model server gets to answer a probe, in seconds.
pub const QWEN_PROBE_SECS: u64 = 4;

/// The real probe: an HTTP GET with a `QWEN_PROBE_SECS` timeout. Any answer below 500 counts as
/// up; a refused or timed-out connection or a server error is down.
pub fn http_qwen_probe(url: &str) -> bool {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(QWEN_PROBE_SECS))
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
        let fleet_root = self.fleet.root().as_os_str().to_os_string();
        let child = launch(&command, &fleet_root, out, err)
            .map_err(|e| CoreError::Io(format!("{}: {e}", command[0])))?;
        let retry = job["id"].as_str().is_some_and(is_pm_job);
        let retry_wait = self.retry_wait;
        let waiter = std::thread::spawn(move || {
            let mut code = exit_code(child);
            let silent =
                || std::fs::metadata(path.with_extension("out")).map_or(true, |m| m.len() == 0);
            // #432 (ruling 9): a Project Manager launcher that says "not now" with no output
            // gets one more try after a short wait. Its stderr is kept and the retry's added.
            if retry && code == Some(super::driver::NOT_NOW) && silent() {
                std::thread::sleep(retry_wait);
                let _ = std::fs::write(job_retry_file(&path), "75");
                let files = std::fs::File::create(path.with_extension("out")).and_then(|out| {
                    let err = std::fs::OpenOptions::new()
                        .append(true)
                        .open(path.with_extension("err"))?;
                    Ok((out, err))
                });
                code = match files.and_then(|(out, err)| launch(&command, &fleet_root, out, err)) {
                    Ok(child) => exit_code(child),
                    Err(e) => {
                        let _ = std::fs::write(path.with_extension("err"), e.to_string());
                        None
                    }
                };
            }
            let code = code.map_or("signal".to_string(), |code| code.to_string());
            let _ = std::fs::write(job_exit_file(&path), code);
        });
        let mut launchers = LAUNCHERS.lock().unwrap_or_else(|p| p.into_inner());
        launchers.retain(|waiter| !waiter.is_finished());
        launchers.push(waiter);
        Ok(())
    }

    fn finished(&mut self, job_id: &str) -> Result<bool, CoreError> {
        self.ended(job_id, true)
    }

    fn peek_finished(&mut self, job_id: &str) -> Result<bool, CoreError> {
        self.ended(job_id, false)
    }
}

/// The threads waiting for the launchers `FleetStarter::start` spawned, each to write its
/// job's exit file (#379).
static LAUNCHERS: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> =
    std::sync::Mutex::new(Vec::new());

/// Wait until every launcher this process started has ended and its exit file is written
/// (#385). The window never calls it: its process lives on. A headless run does before it
/// exits, or a launcher that died would leave no exit code and hold the Project Manager's slot
/// until the job times out (#379).
///
/// The list stays locked through the joins, so a second caller waits for the first's joins
/// and none returns while a launcher it started is still running. A waiter only waits for
/// its child and writes one file, so it never asks for this lock; a `start` on another
/// thread waits for the joins to end.
pub fn wait_for_launchers() {
    let mut launchers = LAUNCHERS.lock().unwrap_or_else(|p| p.into_inner());
    for waiter in launchers.drain(..) {
        let _ = waiter.join();
    }
}

impl FleetStarter {
    /// `JobStarter::finished`; `clean_up` false leaves a failed job's worktree and branch
    /// alone (`peek_finished`).
    fn ended(&self, job_id: &str, clean_up: bool) -> Result<bool, CoreError> {
        let job_file = self.jobs_dir.join(format!("{job_id}.json"));
        if !job_file.exists() {
            return Ok(true);
        }
        let out = self.jobs_dir.join(format!("{job_id}.out"));
        if std::fs::metadata(&out).is_ok_and(|m| m.len() > 0) {
            if let Some(clean) = std::fs::read_to_string(&out)
                .ok()
                .filter(|_| clean_up)
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .and_then(|output| pm_cleanup_target(&output))
            {
                remove_pm_worktree(&clean);
            }
            return Ok(true);
        }
        // No output: a launcher that has exited 0 or 75 is done; one that exited otherwise failed to
        // run the job, which is a fleet error with its exit code and stderr (CORE-2).
        // An exit file still empty is being written: asked again at the next pass.
        if let Some(text) = std::fs::read_to_string(job_exit_file(&job_file))
            .ok()
            .filter(|text| !text.trim().is_empty())
        {
            let code = text.trim().parse::<i32>().ok();
            let not_now = code == Some(super::driver::NOT_NOW);
            let retried = job_retry_file(&job_file).exists();
            // 75 is the fleet's "not now" (CORE-1): an answer, not an error; but a Project
            // Manager launcher that said it again after its one retry has failed (#432).
            if code == Some(0) || (not_now && !retried) {
                return Ok(true);
            }
            let mut stderr =
                std::fs::read_to_string(job_file.with_extension("err")).unwrap_or_default();
            if not_now {
                stderr = format!("exit 75 (not now) twice with no output; retried once\n{stderr}");
            }
            let job_arg = job_file.to_string_lossy().into_owned();
            return Err(CoreError::Fleet(FleetError {
                command: self.fleet.command_line(&["run", &job_arg]),
                exit_code: code,
                stderr,
                reason: FleetErrorReason::Exit,
                undo_note: None,
            }));
        }
        let age = std::fs::metadata(&job_file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map_or(0, |d| d.as_secs());
        Ok(job_timed_out(age))
    }
}
