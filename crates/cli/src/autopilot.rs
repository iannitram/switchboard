//! #385: the autopilot without the window. `switchboard autopilot <pass> [--dry-run]` runs one
//! pass (`roadmap`, `review`, `merge`, `weekly`, or one `satellite` tick) and prints what it
//! did; `switchboard autopilot status` reads the autopilot's state. The MCP's `autopilot_pass`,
//! `satellite_tick` and `autopilot_status` tools answer the same reports. Every pass is the
//! core's (`switchboard_core::autopilot::run`), the one the window's threads call; nothing is
//! decided here.
//!
//! A pass needs its role (`rules::guard_autopilot`): `--role project-manager` for the poll
//! passes, `--role satellite` for the tick. The status read needs none on the CLI.

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use serde_json::Value;
use switchboard_core::autopilot::passes::{self, FleetStarter};
use switchboard_core::autopilot::run::{self, Pass, PassReport, RunEnv, SatelliteSession};
use switchboard_core::autopilot::weekly::local_utc_offset;
use switchboard_core::backup::{self, RepoBackup};
use switchboard_core::clock::{Clock, SystemClock};
use switchboard_core::fleet::{FleetBridge, FleetErrorLog};
use switchboard_core::github::transport::GitHubTransport;
use switchboard_core::rules::{self, Caller, Face, Role, Verdict};
use switchboard_core::{fleet_errors, it, settings, CoreError};

use super::cli;
use super::mcp::ReadCore;

/// How long a headless pass waits for another run's lock before it says so.
pub const LOCK_WAIT: Duration = Duration::from_secs(2);

/// How often a headless satellite tick looks whether its loops have ended.
pub const LOOP_POLL: Duration = Duration::from_millis(500);

/// The MCP tools this module answers (`McpCore`).
pub const MCP_TOOLS: [&str; 3] = ["autopilot_status", "autopilot_pass", "satellite_tick"];

/// Why a headless call did not run: the line for stderr (or the MCP's error text) and the exit
/// code (2 a usage or setup error, 1 a refusal or a failure).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop {
    pub code: u8,
    pub message: String,
}

fn stop(code: u8, message: String) -> Stop {
    Stop { code, message }
}

/// The weekly backup of the repos given: `real_backup`, or a test's fake.
pub type BackupFn<'a> = dyn Fn(&[String]) -> Result<Vec<RepoBackup>, CoreError> + 'a;

/// What a headless pass runs on: the folders the CLI reads from the environment, and the
/// seams the tests replace (the clock, GitHub, the weekly backup).
pub struct Headless<'a> {
    /// The app data folder (`SWITCHBOARD_DATA`, else the desktop app's).
    pub data: Option<PathBuf>,
    /// The app config folder (`satellite.json`, `settings.json`).
    pub config: Option<PathBuf>,
    /// The pipeline checkout (`SWITCHBOARD_PIPELINE`, else `pipeline_repo` in the settings).
    pub pipeline: Option<PathBuf>,
    /// Where the clones are: `SWITCHBOARD_DEV_ROOT`, else beside the pipeline checkout, where
    /// the window looks.
    pub dev_root: Option<PathBuf>,
    /// This machine's name (`FLEET_MACHINE`, else as the window finds it).
    pub machine: Option<String>,
    pub clock: &'a dyn Clock,
    pub utc_offset: Option<i64>,
    /// GitHub, for the read after each merge.
    pub transport: &'a dyn Fn() -> Box<dyn GitHubTransport>,
    /// The weekly backup of these repos (`backup::backup_all` with the app's settings).
    pub backup: &'a BackupFn<'a>,
    pub lock_wait: Duration,
    /// This process's satellite, made at its first tick.
    pub satellite: RefCell<Option<SatelliteSession>>,
}

/// The real weekly backup: the app's settings from the config folder, then `backup_all`.
pub fn real_backup(repos: &[String]) -> Result<Vec<RepoBackup>, CoreError> {
    let config = cli::config_dir().ok_or(CoreError::MissingPath("the app config folder"))?;
    let settings = settings::load(&config.join("settings.json"))?;
    backup::backup_all(&settings, repos)
}

impl<'a> Headless<'a> {
    /// The headless core over the process environment and the seams given.
    pub fn from_env(
        clock: &'a dyn Clock,
        transport: &'a dyn Fn() -> Box<dyn GitHubTransport>,
        backup: &'a BackupFn<'a>,
    ) -> Self {
        let path = |key: &str| {
            std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let pipeline = cli::pipeline_dir();
        let dev_root =
            path(cli::DEV_ROOT_ENV).or_else(|| pipeline.as_ref()?.parent().map(PathBuf::from));
        let home = path("HOME")
            .or_else(|| path("USERPROFILE"))
            .unwrap_or_default();
        let named = std::env::var("FLEET_MACHINE").ok();
        Headless {
            data: cli::data_dir(),
            config: cli::config_dir(),
            pipeline,
            dev_root,
            machine: passes::this_machine(named.as_deref(), &home),
            clock,
            utc_offset: local_utc_offset(),
            transport,
            backup,
            lock_wait: LOCK_WAIT,
            satellite: RefCell::new(None),
        }
    }

    fn data(&self) -> Result<&PathBuf, Stop> {
        self.data.as_ref().ok_or_else(|| {
            stop(
                2,
                format!("switchboard: set {} to the app data folder", cli::DATA_ENV),
            )
        })
    }

    fn config(&self) -> Result<&PathBuf, Stop> {
        self.config.as_ref().ok_or_else(|| {
            stop(
                2,
                format!(
                    "switchboard: set {} to the app config folder",
                    cli::CONFIG_ENV
                ),
            )
        })
    }

    fn pipeline(&self) -> Result<&PathBuf, Stop> {
        self.pipeline.as_ref().ok_or_else(|| {
            stop(
                2,
                format!(
                    "switchboard: set {} to your pipeline checkout \
                     (or pipeline_repo in settings.json)",
                    cli::PIPELINE_ENV
                ),
            )
        })
    }

    /// Run one pass, or with `dry_run` say what it would do. The caller has passed
    /// `rules::guard_autopilot` already. `Err` is a pass that did not run at all: a folder not
    /// set, or another run holding the lock. A pass that ran and failed is a report with
    /// `errors`; its fleet errors are recorded as the window records them (CORE-2, IT-2),
    /// except in a dry run, which writes nothing.
    pub fn run_pass(&self, pass: Pass, dry_run: bool) -> Result<PassReport, Stop> {
        let data = self.data()?;
        let fleet = FleetBridge::new(self.pipeline()?);
        let mut report = if pass == Pass::Satellite {
            let config = self.config()?;
            if dry_run {
                run::satellite_dry(&fleet, data, config)
            } else {
                self.satellite
                    .borrow_mut()
                    .get_or_insert_with(|| SatelliteSession::new(fleet.clone(), data))
                    .tick(config, self.lock_wait)
            }
        } else {
            self.poll_pass(pass, dry_run, &fleet)?
        };
        if !dry_run {
            self.record(&fleet, &mut report);
        }
        Ok(report)
    }

    fn poll_pass(
        &self,
        pass: Pass,
        dry_run: bool,
        fleet: &FleetBridge,
    ) -> Result<PassReport, Stop> {
        let data = self.data()?;
        let pipeline = self.pipeline()?;
        let dev_root = self.dev_root.clone().unwrap_or_else(|| pipeline.clone());
        let mut starter = FleetStarter {
            fleet: fleet.clone(),
            jobs_dir: data.join("autopilot").join("jobs"),
            dev_root: dev_root.clone(),
            machine: self.machine.clone(),
            qwen_probe: Box::new(passes::http_qwen_probe),
        };
        let transport = (self.transport)();
        let env = RunEnv {
            data_dir: data,
            pipeline_root: pipeline,
            dev_root: &dev_root,
            machine: self.machine.as_deref(),
            clock: self.clock,
            utc_offset_secs: self.utc_offset,
            transport: transport.as_ref(),
            dry_run,
            lock_wait: self.lock_wait,
        };
        let mut reports = run::run_poll_passes(&env, &[pass], &mut starter)
            .map_err(|e| stop(1, format!("switchboard: {e}")))?;
        let mut report = reports
            .pop()
            .unwrap_or_else(|| PassReport::new(pass, dry_run));
        if pass == Pass::Weekly && !dry_run && !report.backup.is_empty() {
            let result = (self.backup)(&report.backup);
            run::add_backups(&mut report, result);
        }
        Ok(report)
    }

    /// CORE-2, IT-2: keep each fleet error of `report` in the errors file (`switchboard errors`
    /// lists it) and hand them to `fleet errors file`, as the window does. What cannot be
    /// kept or filed is one more error of the report.
    fn record(&self, fleet: &FleetBridge, report: &mut PassReport) {
        let mut log = FleetErrorLog::default();
        for e in &report.raised {
            if let CoreError::Fleet(e) = e {
                log.record(e.clone());
            }
        }
        if log.errors.is_empty() {
            return;
        }
        if let Some(data) = &self.data {
            for e in &log.errors {
                if let Err(e) = fleet_errors::record(data, e) {
                    report.errors.push(format!("fleet error not kept: {e}"));
                }
            }
        }
        if let Err(e) = it::file_error_tickets(fleet, &log) {
            report.errors.push(format!("IT ticket not filed: {e}"));
        }
    }

    /// Wait for what this process started: each satellite loop (its failed cards classed and
    /// its error recorded, AUTO-9, CORE-2), then each job launcher, so its exit code is on disk
    /// (#379). The report is the loops that ended; `None` when no satellite ran here.
    pub fn finish(&self) -> Option<PassReport> {
        let report = self.satellite.borrow_mut().as_mut().map(|session| {
            let mut report = session.finish(LOOP_POLL);
            if let Some(pipeline) = &self.pipeline {
                self.record(&FleetBridge::new(pipeline), &mut report);
            }
            report
        });
        passes::wait_for_launchers();
        report
    }

    /// The status read: `run::status` over the data and config folders.
    pub fn status(&self) -> Result<Value, Stop> {
        let data = self.data()?;
        run::status(
            data,
            self.config.as_deref(),
            self.clock.now(),
            self.utc_offset,
        )
        .map_err(|e| stop(1, format!("switchboard autopilot status: {e}")))
    }
}

/// `tick` then the loops that ended while it waited, as one report.
fn merge_reports(mut tick: PassReport, end: Option<PassReport>) -> PassReport {
    if let Some(end) = end {
        tick.loops.extend(end.loops);
        tick.errors.extend(end.errors);
        tick.raised.extend(end.raised);
    }
    tick
}

/// The `autopilot` command line, or `None` when `argv` names another command (or asks for
/// help or the version, which `cli::run` answers).
pub fn command(argv: &[String]) -> Option<ExitCode> {
    if !argv.iter().any(|w| w == "autopilot") {
        return None;
    }
    let clock = SystemClock;
    let transport = || cli::live_transport();
    let headless = Headless::from_env(&clock, &transport, &real_backup);
    let cwd = std::env::current_dir().ok();
    // Cannot tell where we are: fail closed, as a job worktree.
    let in_job_worktree = cwd.as_deref().map_or(true, rules::is_job_worktree);
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    command_with(argv, &headless, in_job_worktree, &mut out, &mut err)
}

/// `command` over its seams: the headless core, whether the working directory is a job
/// worktree, stdout and stderr. Exit 0 a pass that ran (or a status read), 1 a pass that
/// failed, a refusal, a held lock or a stdout that could not be written, 2 a usage error.
pub fn command_with(
    argv: &[String],
    headless: &Headless<'_>,
    in_job_worktree: bool,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> Option<ExitCode> {
    if argv.first().map(String::as_str) == Some("help")
        || argv
            .iter()
            .any(|w| matches!(w.as_str(), "--help" | "-h" | "--version" | "-V"))
    {
        return None;
    }
    let rest: Vec<String> = argv.iter().filter(|w| *w != "--dry-run").cloned().collect();
    let dry_run = rest.len() != argv.len();
    let usage = |err: &mut dyn std::io::Write, message: String| {
        let _ = writeln!(err, "switchboard: {message}");
        let _ = writeln!(err, "{}", cli::USAGE);
        Some(ExitCode::from(2))
    };
    let args = match super::parse_args(&rest) {
        Ok(args) => args,
        // Another command's flags are that command's to refuse.
        Err(_) if !rest.iter().any(|w| w == "autopilot") => return None,
        Err(e) => return usage(err, e),
    };
    if args.command.first().map(String::as_str) != Some("autopilot") {
        return None;
    }
    let role = match args.role.as_deref() {
        None => None,
        Some(flag) => match Role::parse(flag) {
            Some(role) => Some(role),
            None => {
                let _ = writeln!(err, "switchboard: unknown role: {flag}");
                return Some(ExitCode::from(2));
            }
        },
    };
    let word = match &args.command[1..] {
        [word] => word.as_str(),
        [] => {
            return usage(
                err,
                "autopilot needs a pass (roadmap, review, merge, weekly, satellite) or status"
                    .to_string(),
            )
        }
        [_, extra @ ..] => {
            return usage(
                err,
                format!("autopilot takes one word, got also {}", extra.join(" ")),
            )
        }
    };
    if word == "status" {
        if dry_run {
            return usage(err, "--dry-run is for a pass".to_string());
        }
        return Some(match headless.status() {
            Ok(status) if args.json => match serde_json::to_string_pretty(&status) {
                Ok(text) => cli::emit_to(out, &format!("{text}\n")),
                Err(e) => fail(err, &stop(1, format!("switchboard: {e}"))),
            },
            Ok(status) => cli::emit_to(out, &run::status_text(&status)),
            Err(stop) => fail(err, &stop),
        });
    }
    let Some(pass) = Pass::parse(word) else {
        return usage(err, format!("unknown autopilot pass: {word}"));
    };
    let caller = Caller {
        face: Face::Cli,
        role,
        in_job_worktree,
        confirmed_in_app: false,
    };
    let what = format!("autopilot {}", pass.name());
    if let Verdict::Refuse(text) | Verdict::ToWaiting(text) =
        rules::guard_autopilot(&caller, &what, &pass.role())
    {
        return Some(fail(err, &stop(1, text)));
    }
    let report = match headless.run_pass(pass, dry_run) {
        Ok(report) => report,
        Err(stop) => return Some(fail(err, &stop)),
    };
    // A satellite tick's report waits for the loops it started; a poll pass's is printed
    // first, then the job launchers are waited for.
    let report = if pass == Pass::Satellite && !dry_run {
        merge_reports(report, headless.finish())
    } else {
        report
    };
    let text = if args.json {
        match serde_json::to_string_pretty(&report) {
            Ok(text) => format!("{text}\n"),
            Err(e) => return Some(fail(err, &stop(1, format!("switchboard: {e}")))),
        }
    } else {
        report
            .text()
            .lines()
            .map(|line| format!("{}\n", cli::terminal_safe(line)))
            .collect()
    };
    let written = cli::emit_to(out, &text);
    if !dry_run {
        headless.finish();
    }
    Some(if written != ExitCode::SUCCESS || report.failed() {
        if written == ExitCode::SUCCESS {
            ExitCode::from(1)
        } else {
            written
        }
    } else {
        ExitCode::SUCCESS
    })
}

fn fail(err: &mut dyn std::io::Write, stop: &Stop) -> ExitCode {
    let _ = writeln!(err, "{}", stop.message);
    ExitCode::from(stop.code)
}

/// The MCP's core with the autopilot tools: `autopilot_status`, `autopilot_pass` and
/// `satellite_tick` are answered here (the role and `rules::guard_autopilot` are checked in
/// `mcp.rs` before the call), every other tool by `reads`.
pub struct McpCore<'a> {
    pub reads: &'a dyn ReadCore,
    pub headless: &'a Headless<'a>,
}

impl ReadCore for McpCore<'_> {
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String> {
        let dry_run = args["dry_run"].as_bool().unwrap_or(false);
        let pass = match tool {
            "autopilot_status" => return self.headless.status().map_err(|stop| stop.message),
            "satellite_tick" => Pass::Satellite,
            "autopilot_pass" => args["pass"]
                .as_str()
                .and_then(Pass::parse)
                .filter(|pass| *pass != Pass::Satellite)
                .ok_or("autopilot_pass: pass must be roadmap, review, merge or weekly")?,
            other => return self.reads.call(other, args),
        };
        let report = self
            .headless
            .run_pass(pass, dry_run)
            .map_err(|stop| stop.message)?;
        serde_json::to_value(&report).map_err(|e| format!("{tool}: {e}"))
    }
}
