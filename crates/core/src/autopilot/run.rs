//! #385: the autopilot's passes as calls any face can make. The window's threads and the
//! headless faces (`switchboard autopilot <pass>`, the MCP's `autopilot_pass` and
//! `satellite_tick`) run the same functions here, so a server with no display does what the
//! window does. Each pass answers a `PassReport`: what it started, merged, refused or would
//! do, which the window prints to stderr and a headless face returns as its output.
//!
//! One run at a time on one machine: the poll passes hold a `FileLock` on `autopilot.json`,
//! the satellite one on `<data>/satellite`. A dry run takes no lock, starts nothing and writes
//! nothing.

use super::driver::{self, ChildRunner, LaneRunner, SatelliteConfig, SATELLITE_FILE};
use super::loops;
use super::passes::{
    after_poll_with, job_exit_file, job_timed_out, merge_pass, merge_would, record_refused_merges,
    AfterPoll, AutopilotState, JobStarter, Merged, PollPasses, RefusedMerge, STATE_FILE,
};
use super::weekly::weekly_due;
use crate::backup::{BackupOutcome, RepoBackup};
use crate::clock::{Clock, UnixSeconds};
use crate::fleet::FleetBridge;
use crate::github::poll::Poller;
use crate::github::snapshot::SnapshotStore;
use crate::github::transport::GitHubTransport;
use crate::lock::{self, FileLock};
use crate::rules::Role;
use crate::CoreError;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One autopilot pass, by SPEC's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// AUTO-2: a Project Manager job for each repo whose ROADMAP.md changed.
    Roadmap,
    /// AUTO-3: a review job for each PR that needs one.
    Review,
    /// AUTO-4: merge what passed.
    Merge,
    /// AUTO-5: the weekly drift clear and backup.
    Weekly,
    /// AUTO-6, AUTO-9: one tick of the satellite's script loops.
    Satellite,
}

impl Pass {
    /// Every pass, in the order the usage line names them.
    pub const ALL: [Pass; 5] = [
        Pass::Roadmap,
        Pass::Review,
        Pass::Merge,
        Pass::Weekly,
        Pass::Satellite,
    ];

    /// The passes the window runs after each poll, in its order.
    pub const AFTER_POLL: [Pass; 4] = [Pass::Review, Pass::Roadmap, Pass::Merge, Pass::Weekly];

    /// The pass a word names; exact match only.
    pub fn parse(word: &str) -> Option<Pass> {
        Pass::ALL.into_iter().find(|pass| pass.name() == word)
    }

    pub fn name(self) -> &'static str {
        match self {
            Pass::Roadmap => "roadmap",
            Pass::Review => "review",
            Pass::Merge => "merge",
            Pass::Weekly => "weekly",
            Pass::Satellite => "satellite",
        }
    }

    /// The role whose work this pass is (`rules::guard_autopilot`): the satellite's for its
    /// tick, the Project Manager's for the rest (R32, as `autopilot_caller`).
    pub fn role(self) -> Role {
        match self {
            Pass::Satellite => Role::Satellite,
            _ => Role::ProjectManager,
        }
    }
}

/// One repo's merges: the PRs merged (or, in a dry run, that would be tried) and the issues
/// they close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergedRow {
    pub repo: String,
    pub prs: Vec<u64>,
    pub closes: Vec<u64>,
}

/// One repo of the weekly backup: `written`, `refused` or `error`, and what there is to say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupRow {
    pub repo: String,
    pub outcome: &'static str,
    pub note: Option<String>,
}

/// One satellite loop that ended: its lane, its exit code (`null` for a signal) and the class
/// of each failed card (AUTO-9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoopRow {
    pub lane: String,
    pub exit_code: Option<i32>,
    /// #432: started by a run that ended before it and watched here; its exit code is not known.
    pub adopted: bool,
    pub cards: Vec<CardRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CardRow {
    pub id: String,
    pub class: String,
}

/// What one pass did, or in a dry run would do.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PassReport {
    pub pass: &'static str,
    pub dry_run: bool,
    /// The ids of the jobs started (review, roadmap, the weekly drift clear).
    pub started: Vec<String>,
    pub merged: Vec<MergedRow>,
    pub refused: Vec<RefusedMerge>,
    /// AUTO-5: the repos this week's backup takes; empty when the week's run is not due.
    pub backup: Vec<String>,
    /// What the backup of each did (`add_backups`); empty until it has run.
    pub backups: Vec<BackupRow>,
    /// AUTO-6: the lanes whose loop started.
    pub lanes_started: Vec<String>,
    pub loops: Vec<LoopRow>,
    /// Why nothing could be done, when that is not an error.
    pub notes: Vec<String>,
    /// What failed, as text; a report with any is a failed pass.
    pub errors: Vec<String>,
    /// The errors behind `errors`, for the face to record the fleet ones (CORE-2).
    #[serde(skip)]
    pub raised: Vec<CoreError>,
    /// A satellite tick only: `Some(true)` when another run held the satellite lock, so the
    /// tick did nothing; `Some(false)` when the tick took it; `None` when it did not ask
    /// (`SatelliteLog`, #432).
    #[serde(skip)]
    pub held_elsewhere: Option<bool>,
}

impl PassReport {
    pub fn new(pass: Pass, dry_run: bool) -> Self {
        PassReport {
            pass: pass.name(),
            dry_run,
            ..PassReport::default()
        }
    }

    /// Did anything fail?
    pub fn failed(&self) -> bool {
        !self.errors.is_empty()
    }

    fn fail(&mut self, about: &str, e: CoreError) {
        self.errors.push(if about.is_empty() {
            e.to_string()
        } else {
            format!("{about}: {e}")
        });
        self.raised.push(e);
    }

    /// The report as the lines the window prints to stderr; none when the pass had nothing to
    /// do (its `notes` are not printed there). A dry run says `would`.
    pub fn lines(&self) -> Vec<String> {
        let who = if self.pass == Pass::Satellite.name() {
            "satellite"
        } else {
            "autopilot"
        };
        let (start, merge) = if self.dry_run {
            ("would start", "would merge")
        } else {
            ("started", "merged")
        };
        let mut lines = Vec::new();
        for id in &self.started {
            lines.push(format!("{who}: {start} {id}"));
        }
        for row in &self.merged {
            lines.push(format!(
                "{who}: {merge} {} {:?}, closing {:?}",
                row.repo, row.prs, row.closes
            ));
        }
        for r in &self.refused {
            lines.push(format!(
                "{who}: merge {}#{} refused: {}",
                r.repo, r.pr, r.why
            ));
        }
        if self.dry_run {
            for repo in &self.backup {
                lines.push(format!("weekly backup: would back up {repo}"));
            }
        }
        for row in &self.backups {
            lines.push(match &row.note {
                Some(note) => format!("weekly backup: {}: {}: {note}", row.repo, row.outcome),
                None => format!("weekly backup: {}: {}", row.repo, row.outcome),
            });
        }
        for lane in &self.lanes_started {
            lines.push(format!("{who}: {start} {lane}"));
        }
        for row in &self.loops {
            for card in &row.cards {
                lines.push(format!("{who} {}: {} {}", row.lane, card.id, card.class));
            }
        }
        for error in &self.errors {
            lines.push(format!("{who}: {error}"));
        }
        lines
    }

    /// `lines` for a headless face, which also says why nothing could be done (`notes`) and
    /// that a pass had nothing to do.
    pub fn text(&self) -> String {
        let mut lines = self.lines();
        for note in &self.notes {
            lines.push(format!("autopilot {}: {note}", self.pass));
        }
        if lines.is_empty() {
            let dry = if self.dry_run { " (dry run)" } else { "" };
            lines.push(format!("autopilot {}: nothing to do{dry}", self.pass));
        }
        lines.join("\n") + "\n"
    }
}

/// What the poll passes run on.
pub struct RunEnv<'a> {
    /// The app data folder: snapshots, `autopilot.json`, the Waiting items.
    pub data_dir: &'a Path,
    pub pipeline_root: &'a Path,
    /// Where the clones are, each `<dev_root>/<name>`.
    pub dev_root: &'a Path,
    /// This machine's name; without one no review starts.
    pub machine: Option<&'a str>,
    pub clock: &'a dyn Clock,
    /// This computer's UTC offset (`weekly::local_utc_offset`); without one the weekly run waits.
    pub utc_offset_secs: Option<i64>,
    /// For the read after each merge (GH-3).
    pub transport: &'a dyn GitHubTransport,
    /// Say what would be done: no job, no merge, no state, no lock.
    pub dry_run: bool,
    /// How long to wait for another run's lock: none for the window (it comes back at the next
    /// poll), a moment for a headless run.
    pub lock_wait: Duration,
}

/// The lock target of the poll passes: `<data>/autopilot.json`.
pub fn passes_lock_target(data_dir: &Path) -> PathBuf {
    data_dir.join(STATE_FILE)
}

/// The lock target of the satellite: `<data>/satellite`.
pub fn satellite_lock_target(data_dir: &Path) -> PathBuf {
    data_dir.join("satellite")
}

/// Run the poll passes `which` names (the satellite's is not one), in the window's order:
/// review, roadmap, merge, weekly. One report each. The error is another run holding the lock
/// (the window, or a headless pass), or a data folder that cannot be made: then nothing ran.
///
/// The weekly report names the repos to back up (`backup`); the caller runs the backup, which
/// is long and needs nothing held, and adds its rows with `add_backups`.
pub fn run_poll_passes(
    env: &RunEnv<'_>,
    which: &[Pass],
    starter: &mut dyn JobStarter,
) -> Result<Vec<PassReport>, CoreError> {
    if env.dry_run {
        return Ok(poll_passes(env, which, starter));
    }
    std::fs::create_dir_all(env.data_dir)
        .map_err(|e| CoreError::Io(format!("{}: {e}", env.data_dir.display())))?;
    let held = FileLock::take(&passes_lock_target(env.data_dir), env.lock_wait).map_err(|e| {
        CoreError::Refused(format!("autopilot: another run holds the passes ({e})"))
    })?;
    Ok(held.keep_fresh(|| poll_passes(env, which, starter)))
}

fn poll_passes(env: &RunEnv<'_>, which: &[Pass], starter: &mut dyn JobStarter) -> Vec<PassReport> {
    let mut reports = Vec::new();
    let (review, roadmap) = (
        which.contains(&Pass::Review),
        which.contains(&Pass::Roadmap),
    );
    if review || roadmap {
        let mut rv = PassReport::new(Pass::Review, env.dry_run);
        let mut rm = PassReport::new(Pass::Roadmap, env.dry_run);
        if env.machine.is_none() {
            rv.notes
                .push("this machine has no name (FLEET_MACHINE); no review starts".to_string());
        }
        let mut ctx = AfterPoll {
            data_dir: env.data_dir,
            machine: env.machine,
            starter: &mut *starter,
        };
        let passes = PollPasses {
            review,
            roadmap,
            dry_run: env.dry_run,
        };
        match after_poll_with(&mut ctx, passes) {
            Ok(out) => {
                rv.started = out.review_started;
                rm.started = out.roadmap_started;
                if let Some(e) = out.review_error {
                    rv.fail("", e);
                    rm.notes
                        .push("not run: the review pass failed first".to_string());
                }
                if let Some(e) = out.roadmap_error {
                    rm.fail("", e);
                }
            }
            Err(e) => {
                let first = if review { &mut rv } else { &mut rm };
                first.fail("", e);
            }
        }
        if review {
            reports.push(rv);
        }
        if roadmap {
            reports.push(rm);
        }
    }
    if which.contains(&Pass::Merge) {
        reports.push(merge_report(env));
    }
    if which.contains(&Pass::Weekly) {
        reports.push(weekly_report(env, starter));
    }
    reports
}

fn merged_rows(merged: Vec<Merged>) -> Vec<MergedRow> {
    merged
        .into_iter()
        .map(|(repo, prs, closes)| MergedRow { repo, prs, closes })
        .collect()
}

/// AUTO-4 (#380): every passed PR is tried, each merge followed by a read of its repo (GH-3);
/// each refusal lands in Waiting on you.
fn merge_report(env: &RunEnv<'_>) -> PassReport {
    let mut report = PassReport::new(Pass::Merge, env.dry_run);
    let store = SnapshotStore::new(env.data_dir);
    let snaps = match store.load_all() {
        Ok(snaps) => snaps,
        Err(e) => {
            report.fail("merge", e);
            return report;
        }
    };
    if env.dry_run {
        report.merged = merged_rows(merge_would(&snaps, env.dev_root));
        return report;
    }
    let poller = Poller {
        transport: env.transport,
        store: &store,
        clock: env.clock,
        repos: Vec::new(),
    };
    let pass = merge_pass(&snaps, &poller, env.pipeline_root, env.dev_root);
    report.merged = merged_rows(pass.merged);
    report.refused = pass.refused;
    if let Err(e) = record_refused_merges(env.data_dir, &report.refused, env.clock.now()) {
        report.fail("waiting", e);
    }
    report
}

/// AUTO-5: the weekly drift clear and the repos to back up, once Sunday 6am local has passed.
fn weekly_report(env: &RunEnv<'_>, starter: &mut dyn JobStarter) -> PassReport {
    let mut report = PassReport::new(Pass::Weekly, env.dry_run);
    let Some(offset) = env.utc_offset_secs else {
        report
            .notes
            .push("this computer's UTC offset is not known; the weekly run waits".to_string());
        return report;
    };
    let now = env.clock.now();
    match super::passes::weekly_run_with(env.data_dir, now, offset, starter, env.dry_run) {
        Ok(run) => {
            report.started = run.drift_started;
            report.backup = run.backup;
            if let Some(e) = run.drift_error {
                // #379: a Project Manager job that died is recorded like any fleet error.
                report.fail("drift clear", e);
            }
        }
        Err(e) => report.fail("weekly", e),
    }
    report
}

/// Add what the weekly backup did (`backup::backup_all` over the report's `backup`) to the
/// weekly report: one row per repo; a repo that failed, or a backup that could not start, is
/// also an error.
pub fn add_backups(report: &mut PassReport, result: Result<Vec<RepoBackup>, CoreError>) {
    let outcomes = match result {
        Ok(outcomes) => outcomes,
        Err(e) => return report.fail("weekly backup", e),
    };
    for (repo, outcome) in outcomes {
        let (outcome, note) = match outcome {
            Ok(BackupOutcome::Written(manifest)) => (
                "written",
                manifest
                    .resolved_repo
                    .map(|now| format!("GitHub answered for {now}: renamed or moved")),
            ),
            Ok(BackupOutcome::Refused(why)) => ("refused", Some(why)),
            Err(e) => {
                let text = e.to_string();
                report.fail("weekly backup", e);
                ("error", Some(text))
            }
        };
        report.backups.push(BackupRow {
            repo,
            outcome,
            note,
        });
    }
}

/// The satellite of one process (AUTO-6, AUTO-9): the runner with the loops it watches.
///
/// #432: each tick takes the satellite lock for that tick only and gives it back when the tick
/// returns, in every face (the window, `switchboard autopilot satellite`, `switchboard mcp`).
/// What keeps a second copy of a loop from starting is `<data>/satellite/loops.json`
/// (`loops`): every loop is recorded there when it starts, every tick counts the loops running
/// on this machine whoever started them, and a tick adopts the loops of a run that has ended
/// and watches them to their end. The holds after a failed loop are kept there too.
pub struct SatelliteSession {
    fleet: FleetBridge,
    data_dir: PathBuf,
    runner: ChildRunner,
}

impl SatelliteSession {
    pub fn new(fleet: FleetBridge, data_dir: &Path) -> Self {
        let runner = ChildRunner::new(fleet.clone(), &satellite_lock_target(data_dir));
        Self::with_runner(fleet, data_dir, runner)
    }

    /// `new` with the runner given (a test's has a short hold).
    pub fn with_runner(fleet: FleetBridge, data_dir: &Path, runner: ChildRunner) -> Self {
        SatelliteSession {
            fleet,
            data_dir: data_dir.to_path_buf(),
            runner,
        }
    }

    /// #432: the bridge the next tick asks and starts loops with. The window reads its settings
    /// at each tick and hands the bridge in here, so a changed pipeline is used at once.
    pub fn set_fleet(&mut self, fleet: FleetBridge) {
        self.runner.set_fleet(fleet.clone());
        self.fleet = fleet;
    }

    /// One tick: read `satellite.json` in `config_dir` (off when missing), record each loop
    /// that ended (a fleet error for one that ended with an error, CORE-2; the class of each
    /// failed card, AUTO-9) and, with the switch on, take the satellite lock, adopt the loops
    /// of a run that has ended (#432), start the lanes `driver::tick` allows and give the lock
    /// back. Another run holding the lock (another tick, now) is this tick's error and nothing
    /// starts.
    pub fn tick(&mut self, config_dir: &Path, lock_wait: Duration) -> PassReport {
        let mut report = PassReport::new(Pass::Satellite, false);
        let config = match driver::load_config(&config_dir.join(SATELLITE_FILE)) {
            Ok(config) => config,
            Err(e) => {
                report.fail("", e);
                return report;
            }
        };
        self.reap(&mut report);
        if !config.satellite_on {
            report
                .notes
                .push("the satellite switch is off (satellite.json)".to_string());
            return report;
        }
        let taken = std::fs::create_dir_all(&self.data_dir)
            .map_err(|e| CoreError::Io(format!("{}: {e}", self.data_dir.display())))
            .and_then(|()| FileLock::take(&satellite_lock_target(&self.data_dir), lock_wait));
        let _held = match taken {
            Ok(lock) => lock,
            Err(e) => {
                report.held_elsewhere = Some(true);
                report
                    .errors
                    .push(format!("another run holds the satellite ({e})"));
                return report;
            }
        };
        report.held_elsewhere = Some(false);
        match self.runner.adopt() {
            Ok(lanes) => {
                for lane in lanes {
                    report.notes.push(format!(
                        "adopted the {lane} loop of a run that ended; watching it to its end"
                    ));
                }
            }
            Err(e) => report.fail("", e),
        }
        self.reap(&mut report);
        let fleet = &self.fleet;
        let spend = |lane: &str| super::may_spend(fleet, lane);
        match driver::tick(&config, &mut self.runner, &spend) {
            Ok(started) => report.lanes_started = started,
            Err(e) => report.fail("", e),
        }
        for e in self.runner.take_errors() {
            report.fail("", e);
        }
        report
    }

    /// The lanes whose loop runs on this machine now, whoever started it.
    pub fn running(&mut self) -> Vec<String> {
        self.runner.running()
    }

    /// Wait until every loop this session watches (started or adopted) has ended, looking every
    /// `every`. The report holds each loop that ended, as a tick's does. A headless run calls
    /// this before it exits, so a loop's failed cards are classed and its error recorded.
    pub fn finish(&mut self, every: Duration) -> PassReport {
        let mut report = PassReport::new(Pass::Satellite, false);
        loop {
            self.reap(&mut report);
            if self.runner.watching().is_empty() {
                break;
            }
            std::thread::sleep(every);
        }
        report
    }

    fn reap(&mut self, report: &mut PassReport) {
        for end in self.runner.take_finished() {
            // CORE-2 (#383): a loop that ended with an error is a fleet error with its exit
            // code and stderr; the runner holds its lane for a while.
            if let Some(e) = end.fleet_error() {
                report.fail(&end.lane, e);
            }
            let dir = satellite_lock_target(&self.data_dir)
                .join("cards")
                .join(&end.lane);
            let mut row = LoopRow {
                lane: end.lane.clone(),
                exit_code: end.exit_code,
                adopted: end.adopted,
                cards: Vec::new(),
            };
            match driver::classify_failed(&self.fleet, &dir, &end.stdout) {
                Ok(classes) => {
                    row.cards = classes
                        .into_iter()
                        .map(|(id, class)| CardRow {
                            id,
                            class: format!("{class:?}"),
                        })
                        .collect();
                }
                Err(e) => report.fail(&end.lane, e),
            }
            report.loops.push(row);
        }
        for e in self.runner.take_errors() {
            report.fail("", e);
        }
    }
}

/// #432: the window's satellite lines. A tick refused because another run holds the satellite
/// is said once when that starts, not at every tick, and "free again" once a tick takes the
/// lock again.
#[derive(Debug, Default)]
pub struct SatelliteLog {
    held_elsewhere: bool,
}

/// The line `SatelliteLog` prints when a tick takes the satellite again.
pub const SATELLITE_FREE_AGAIN: &str = "satellite: free again";

impl SatelliteLog {
    /// The lines to print for this tick's report (`PassReport::lines`, less the repeat).
    pub fn lines(&mut self, report: &PassReport) -> Vec<String> {
        let mut lines = report.lines();
        match report.held_elsewhere {
            Some(true) => {
                if self.held_elsewhere {
                    lines.retain(|line| !line.contains("another run holds the satellite"));
                }
                self.held_elsewhere = true;
            }
            Some(false) => {
                if self.held_elsewhere {
                    lines.insert(0, SATELLITE_FREE_AGAIN.to_string());
                }
                self.held_elsewhere = false;
            }
            None => {}
        }
        lines
    }
}

/// What the satellites on this machine have on disk (`loops.json`), read at `now`: whether
/// one is at work now (a loop runs, or a tick holds the lock), the lanes whose loop runs, the
/// holds whose time has not passed (`{lane, seconds_left}`), and when the file last changed.
struct SatelliteSeen {
    live: bool,
    running: Vec<String>,
    held: Vec<Value>,
    as_of: Value,
}

fn satellite_seen(data_dir: &Path, now: UnixSeconds) -> SatelliteSeen {
    let target = satellite_lock_target(data_dir);
    let file = loops::read(&target);
    let running = file.running();
    let held = file
        .holds_at(now.saturating_mul(1000))
        .into_iter()
        .map(|(lane, ms)| json!({ "lane": lane, "seconds_left": (ms + 999) / 1000 }))
        .collect();
    SatelliteSeen {
        live: !running.is_empty() || lock::held(&target),
        running,
        held,
        as_of: if file.at > 0 {
            json!(file.at)
        } else {
            Value::Null
        },
    }
}

/// A runner that starts nothing: the lanes running and held as given, the lanes asked for kept.
struct DryRunner {
    running: Vec<String>,
    held: Vec<String>,
}

impl LaneRunner for DryRunner {
    fn running(&mut self) -> Vec<String> {
        self.running.clone()
    }

    fn start(&mut self, _lane: &str) -> Result<(), CoreError> {
        Ok(())
    }

    fn held(&mut self) -> Vec<String> {
        self.held.clone()
    }
}

/// A satellite tick as a dry run: the lanes a tick would start now, with no loop started, no
/// lock taken and nothing written. The budget is still asked (`fleet budget allow`, AUTO-1).
/// #432: the loops running and the holds on this machine (`loops.json`) are read and counted,
/// as a real tick counts them; a note names the loops running.
pub fn satellite_dry(fleet: &FleetBridge, data_dir: &Path, config_dir: &Path) -> PassReport {
    let mut report = PassReport::new(Pass::Satellite, true);
    let config = match driver::load_config(&config_dir.join(SATELLITE_FILE)) {
        Ok(config) => config,
        Err(e) => {
            report.fail("", e);
            return report;
        }
    };
    if !config.satellite_on {
        report
            .notes
            .push("the satellite switch is off (satellite.json)".to_string());
        return report;
    }
    let seen = satellite_seen(data_dir, loops::now_ms() / 1000);
    if !seen.running.is_empty() {
        report.notes.push(format!(
            "loops running on this machine: {}",
            seen.running.join(", ")
        ));
    }
    let mut runner = DryRunner {
        running: seen.running,
        held: seen
            .held
            .iter()
            .filter_map(|hold| hold["lane"].as_str().map(str::to_string))
            .collect(),
    };
    let spend = |lane: &str| super::may_spend(fleet, lane);
    match driver::tick(&config, &mut runner, &spend) {
        Ok(started) => report.lanes_started = started,
        Err(e) => report.fail("", e),
    }
    report
}

/// The jobs the passes started that have not ended: each job file in `<data>/autopilot/jobs/`
/// with no output, no exit code and not timed out, by id. A read: nothing is cleaned up.
fn running_jobs(data_dir: &Path) -> Vec<Value> {
    let dir = data_dir.join("autopilot").join("jobs");
    let mut jobs: Vec<Value> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter(|path| {
            let full = |p: PathBuf| std::fs::metadata(p).is_ok_and(|m| m.len() > 0);
            let age = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .map_or(0, |d| d.as_secs());
            !full(path.with_extension("out")) && !full(job_exit_file(path)) && !job_timed_out(age)
        })
        .map(|path| {
            let job: Value = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or(Value::Null);
            let id = path.file_stem().map(|s| s.to_string_lossy().into_owned());
            json!({ "id": id, "repo": job["repo"], "lane": job["lane"] })
        })
        .collect();
    jobs.sort_by_key(|job| job["id"].as_str().map(str::to_string));
    jobs
}

/// The status read (#385): what `autopilot.json` holds (each ROADMAP by its repo's name only),
/// whether the week's run is due, whether a run holds the passes now, the jobs still running,
/// and the satellite: its switch and lanes, whether one is at work on this machine (`live`: a
/// loop runs or a tick holds the lock), the lanes whose loop runs, and the holds, as
/// `loops.json` has them (`as_of`: when it last changed, #432). It reads only; nothing is
/// changed or cleaned up.
pub fn status(
    data_dir: &Path,
    config_dir: Option<&Path>,
    now: UnixSeconds,
    utc_offset_secs: Option<i64>,
) -> Result<Value, CoreError> {
    let state = AutopilotState::load(data_dir)?;
    let config = match config_dir {
        Some(dir) => driver::load_config(&dir.join(SATELLITE_FILE))?,
        None => SatelliteConfig::default(),
    };
    let seen = satellite_seen(data_dir, now);
    let roadmaps: Vec<&str> = state
        .roadmaps
        .iter()
        .map(|(repo, _)| repo.as_str())
        .collect();
    Ok(json!({
        "state": {
            "reviews_started": state.reviews_started,
            "roadmaps_seen": roadmaps,
            "weekly_last_run": state.weekly_last_run,
            "drift_pending": state.drift_pending,
            "pm_running": state.pm_running,
        },
        "weekly_due": utc_offset_secs.map(|offset| weekly_due(now, offset, state.weekly_last_run)),
        "passes_running": lock::held(&passes_lock_target(data_dir)),
        "running_jobs": running_jobs(data_dir),
        "satellite": {
            "on": config.satellite_on,
            "lanes": config.lanes,
            "live": seen.live,
            "running": seen.running,
            "held": seen.held,
            "as_of": seen.as_of,
        },
    }))
}

/// `status` as text, one fact per line.
pub fn status_text(status: &Value) -> String {
    let list = |v: &Value| {
        let words: Vec<String> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| match item {
                Value::String(text) => text.clone(),
                other => match (other["id"].as_str(), other["lane"].as_str()) {
                    (Some(id), _) => id.to_string(),
                    (None, Some(lane)) => {
                        format!("{lane} ({} s left)", other["seconds_left"])
                    }
                    _ => other.to_string(),
                },
            })
            .collect();
        if words.is_empty() {
            "none".to_string()
        } else {
            words.join(", ")
        }
    };
    let or_none = |v: &Value| match v {
        Value::Null => "none".to_string(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let yes_no = |v: &Value| match v.as_bool() {
        Some(true) => "yes",
        Some(false) => "no",
        None => "not known",
    };
    let state = &status["state"];
    let satellite = &status["satellite"];
    let reviews = state["reviews_started"].as_array().map_or(0, Vec::len);
    [
        format!("passes running now: {}", yes_no(&status["passes_running"])),
        format!("running jobs: {}", list(&status["running_jobs"])),
        format!("project manager job: {}", or_none(&state["pm_running"])),
        format!("reviews started: {reviews}"),
        format!("roadmaps seen: {}", list(&state["roadmaps_seen"])),
        format!("weekly last run: {}", or_none(&state["weekly_last_run"])),
        format!("weekly due: {}", yes_no(&status["weekly_due"])),
        format!("drift clear pending: {}", list(&state["drift_pending"])),
        format!(
            "satellite switch: {}",
            if satellite["on"] == true { "on" } else { "off" }
        ),
        format!("satellite lanes: {}", list(&satellite["lanes"])),
        format!(
            "satellite loops running here: {}",
            yes_no(&satellite["live"])
        ),
        format!("satellite loops: {}", list(&satellite["running"])),
        format!("satellite holds: {}", list(&satellite["held"])),
    ]
    .join("\n")
        + "\n"
}
