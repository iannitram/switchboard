//! CLI-1: `switchboard sky | waiting | reports | budget | loadout | star <repo>#<n>`, as text or
//! `--json`, printing what the GUI shows.
//! CLI-2: `switchboard refresh | backup <repo> | backup --all`, the GUI buttons.
//! CLI-3: a repo delete or a restore over a live repo is refused and becomes a Waiting on you
//! item; `--role <role> merge <repo>#<pr>` goes through CORE-3; inside a job worktree no role
//! gives write actions.
//!
//! The raw `fleet` CLI stays the documented escape hatch: Switchboard keeps no state that only
//! it can read.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{json, Value};
use switchboard_core::backup::{self, BackupOutcome};
use switchboard_core::backup_run::{self, BackupRow};
use switchboard_core::budget::{self, PoolRow};
use switchboard_core::clock::{Clock, SystemClock, UnixSeconds};
use switchboard_core::error::CoreError;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::live;
use switchboard_core::github::merge::{self as gh_merge, MergeJob};
use switchboard_core::github::poll::{self, Poller};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, HttpTransport};
use switchboard_core::github::RepoSnapshot;
use switchboard_core::it::{self, Loadout};
use switchboard_core::joblog::{self, NOT_RECORDED};
use switchboard_core::platform::{current_os, default_dev_root, GhAuthToken, Os};
use switchboard_core::reports::{self, Filter, Report};
use switchboard_core::rules::{self, Caller, Face, GuardedAction, Role, Verdict};
use switchboard_core::settings::{self, Settings};
use switchboard_core::sky;
use switchboard_core::waiting::{self, guarded_item, Item, LocalItems};

/// The environment variable that names the pipeline checkout. There is no default.
pub const PIPELINE_ENV: &str = "SWITCHBOARD_PIPELINE";
/// The environment variable that names the app data folder (the snapshot is in `snapshot/`).
/// Unset, the folder is the desktop app's own (see `data_dir`).
pub const DATA_ENV: &str = "SWITCHBOARD_DATA";
/// The environment variable that names the fleet state folder (its `ledger/` holds the job log).
/// Unset, the app data folder is used.
pub const STATE_ENV: &str = "SWITCHBOARD_STATE";
/// The environment variable that names the folder holding the clones (`<dev root>/<name>`).
/// Unset, it is this OS's default dev root under `HOME`.
pub const DEV_ROOT_ENV: &str = "SWITCHBOARD_DEV_ROOT";

/// The app data folder's name: the desktop app's identifier.
const APP_DIR: &str = "app.switchboard.desktop";
/// The line `waiting` and `reports` print when nothing is waiting.
const NOTHING_WAITING: &str = "nothing waiting on you";
/// The line `reports` prints when there are no reports.
const NO_REPORTS: &str = "no reports";
/// The message when no snapshot has been written yet.
const NO_SNAPSHOT: &str = "switchboard: no snapshot yet; run switchboard refresh";

const USAGE: &str = "usage: switchboard [--help | --version] [--json] [--role <role>] \
                     [--project <repo>] [--outcome <o>] [--pool <p>] \
                     <sky | waiting | reports | budget | loadout | star <repo>#<n> | \
                     refresh | backup <repo> | backup --all | delete <repo> | restore <repo> | merge <repo>#<pr> | mcp>";

/// The writes a CLI command can reach (CLI-3). The real ones are `CliWrites`; the tests pass
/// fakes.
pub trait Writes {
    /// GH-5: merge only with a PASS on the head and CI green, through `merge_pr.py`. An error
    /// is the refusal text.
    fn merge(&self, repo: &str, pr: u64) -> Result<(), String>;
    /// WAIT-1: add one Waiting on you item.
    fn waiting(&self, item: &Item) -> Result<(), String>;
    /// Whether Switchboard knows `repo` (a stored snapshot of `owner/name`, or a bare name).
    /// An error is the reason it cannot tell.
    fn knows_repo(&self, repo: &str) -> Result<bool, String>;
}

/// Whether `repo` names one of `known` (each `owner/name`): the whole `owner/name`, or, when
/// `repo` has no `/`, the name part.
pub fn repo_known(repo: &str, known: &[String]) -> bool {
    known
        .iter()
        .any(|full| full == repo || (!repo.contains('/') && full.rsplit('/').next() == Some(repo)))
}

/// The real writes (CLI-3): a merge through GH-5 from the stored snapshot, and the Waiting on
/// you item into the local items. `None` folders are named in the refusal.
pub struct CliWrites {
    /// The app data folder: the snapshot store and `waiting/local.json`.
    pub data: Option<PathBuf>,
    /// The pipeline checkout: `merge_pr.py` is under it.
    pub pipeline: Option<PathBuf>,
    /// The folder that holds the clones: the clone of `owner/name` is `<dev root>/<name>`.
    pub dev_root: Option<PathBuf>,
    /// The working folder is a job worktree (the guard has already refused then).
    pub in_job_worktree: bool,
    /// The re-read after a merge (GH-3); `None` builds the real one at merge time.
    pub transport: Option<Box<dyn GitHubTransport>>,
}

impl CliWrites {
    /// The folders from the environment: `SWITCHBOARD_DATA` (else the desktop app's folder),
    /// `SWITCHBOARD_PIPELINE`, and `SWITCHBOARD_DEV_ROOT` (else this OS's default under `HOME`).
    pub fn from_env(in_job_worktree: bool) -> Self {
        Self {
            data: data_dir(),
            pipeline: env_path(PIPELINE_ENV),
            dev_root: env_path(DEV_ROOT_ENV)
                .or_else(|| env_path("HOME").map(|home| default_dev_root(current_os(), &home))),
            in_job_worktree,
            transport: None,
        }
    }

    /// GH-5 from the snapshot: the stored PR, its clone, then `merge` through `merge_pr.py`.
    /// The error is the reason alone; `merge` puts the `merge <repo>#<pr>:` prefix on it.
    fn try_merge(&self, repo: &str, pr: u64) -> Result<(), String> {
        let data = self
            .data
            .as_deref()
            .ok_or("set SWITCHBOARD_DATA to the app data folder")?;
        let store = SnapshotStore::new(data);
        let snaps = store.load_all().map_err(|e| e.to_string())?;
        let snap = snaps
            .iter()
            .find(|snap| snap.repo == repo)
            .or_else(|| {
                snaps
                    .iter()
                    .find(|snap| snap.repo.ends_with(&format!("/{repo}")))
            })
            .ok_or_else(|| format!("no snapshot of {repo}; run switchboard refresh"))?;
        let pull = snap
            .pulls
            .iter()
            .find(|pull| pull.number == pr)
            .ok_or("not in the snapshot; run switchboard refresh")?;
        let pipeline = self
            .pipeline
            .as_deref()
            .ok_or("set SWITCHBOARD_PIPELINE to your pipeline checkout")?;
        let dev_root = self
            .dev_root
            .as_deref()
            .ok_or("set SWITCHBOARD_DEV_ROOT to the folder that holds the clones")?;
        let name = snap.repo.rsplit('/').next().unwrap_or_default();
        let clone = dev_root.join(name);
        if !clone.is_dir() {
            return Err(format!("no clone at {}", clone.display()));
        }
        // Only `--role project-manager` outside a job worktree reaches `Writes::merge` (the
        // guard in `write_command`), so this is the caller that was allowed.
        let caller = Caller {
            face: Face::Cli,
            role: Some(Role::ProjectManager),
            in_job_worktree: self.in_job_worktree,
            confirmed_in_app: false,
        };
        let job = MergeJob {
            pipeline_root: pipeline,
            clone: &clone,
            repo: snap.repo.as_str(),
            pr: pull,
            carried_over: false,
        };
        // The re-read after the push: a live transport when none was given (best effort).
        let live: Box<dyn GitHubTransport>;
        let transport: &dyn GitHubTransport = match &self.transport {
            Some(transport) => &**transport,
            None => {
                live = live_transport();
                &*live
            }
        };
        let poller = Poller {
            transport,
            store: &store,
            clock: &SystemClock,
            repos: vec![snap.repo.clone()],
        };
        gh_merge::merge(&caller, &job, &poller).map_err(|e| match e {
            CoreError::Refused(why) => why,
            other => other.to_string(),
        })
    }
}

impl Writes for CliWrites {
    fn merge(&self, repo: &str, pr: u64) -> Result<(), String> {
        self.try_merge(repo, pr)
            .map_err(|why| format!("merge {repo}#{pr}: {why}"))
    }

    /// WAIT-1: the item goes into the local items; asking twice keeps one (same `source`).
    fn waiting(&self, item: &Item) -> Result<(), String> {
        let data = self
            .data
            .as_deref()
            .ok_or("waiting: set SWITCHBOARD_DATA to the app data folder")?;
        LocalItems::new(data)
            .add(item.clone())
            .map_err(|e| format!("waiting: {e}"))
    }

    /// The repos of the stored snapshots, from the app data folder.
    fn knows_repo(&self, repo: &str) -> Result<bool, String> {
        let data = self
            .data
            .as_deref()
            .ok_or("set SWITCHBOARD_DATA to the app data folder")?;
        let snaps = SnapshotStore::new(data)
            .load_all()
            .map_err(|e| e.to_string())?;
        let known: Vec<String> = snaps.into_iter().map(|snap| snap.repo).collect();
        Ok(repo_known(repo, &known))
    }
}

/// The CLI's own merge (CLI-3, #242): builds `CliWrites` the way `run` does and calls its
/// `merge`, so the MCP's `merge` (MCP-2) is the CLI's merge and the two faces never drift.
pub(crate) fn real_merge(repo: &str, pr: u64) -> Result<(), String> {
    let in_job_worktree = match std::env::current_dir() {
        Ok(dir) => rules::is_job_worktree(&dir),
        // Cannot tell where we are: fail closed, no writes.
        Err(_) => true,
    };
    CliWrites::from_env(in_job_worktree).merge(repo, pr)
}

/// A transport that only answers with why there is no GitHub token.
struct NoGitHub(String);

impl GitHubTransport for NoGitHub {
    fn graphql(&self, _query: &str, _variables: &Value) -> Result<Value, CoreError> {
        Err(CoreError::GitHub(self.0.clone()))
    }

    fn rest_get(
        &self,
        _path: &str,
        _etag: Option<&str>,
    ) -> Result<switchboard_core::github::transport::RestAnswer, CoreError> {
        Err(CoreError::GitHub(self.0.clone()))
    }
}

/// The real transport for the re-read, or one that says why there is none (`gh auth token`).
fn live_transport() -> Box<dyn GitHubTransport> {
    match HttpTransport::new(&GhAuthToken::default()) {
        Ok(http) => Box::new(http),
        Err(e) => Box::new(NoGitHub(e.to_string())),
    }
}

pub fn run(argv: &[String]) -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        // Cannot tell whether this is a job worktree: fail closed, no writes.
        Err(e) => {
            eprintln!("switchboard: {e}");
            return ExitCode::from(1);
        }
    };
    run_with(
        argv,
        &cwd,
        SystemClock.now(),
        &CliWrites::from_env(rules::is_job_worktree(&cwd)),
        &mut std::io::stderr(),
    )
}

/// The text to print for a first word that asks for help or the version, or `None` when the
/// argv is a real command. Only the first word counts; the rest of the line is not read.
fn early_answer(argv: &[String]) -> Option<String> {
    match argv.first().map(String::as_str) {
        Some("--help" | "-h" | "help") => Some(USAGE.to_string()),
        Some("--version" | "-V") => Some(format!("switchboard {}", env!("CARGO_PKG_VERSION"))),
        _ => None,
    }
}

/// `run` with its seams passed in: the working directory, the time, the writes and stderr.
pub fn run_with(
    argv: &[String],
    cwd: &Path,
    now: UnixSeconds,
    writes: &dyn Writes,
    err: &mut dyn Write,
) -> ExitCode {
    if let Some(text) = early_answer(argv) {
        println!("{text}");
        return ExitCode::SUCCESS;
    }
    let (filter, argv) = match take_filter(argv) {
        Ok(taken) => taken,
        Err(e) => {
            eprintln!("switchboard: {e}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let (all, argv) = take_all(&argv);
    let args = match super::parse_args(&argv) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("switchboard: {e}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if let Some(role) = &args.role {
        if Role::parse(role).is_none() {
            let _ = writeln!(err, "switchboard: unknown role: {role}");
            return ExitCode::from(2);
        }
    }
    let command = args.command.first().map(String::as_str);
    if command != Some("reports") && filter != Filter::default() {
        eprintln!("switchboard: --project, --outcome and --pool are for reports");
        return ExitCode::from(2);
    }
    if all && command != Some("backup") {
        eprintln!("switchboard: --all is for backup");
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    match command {
        Some("refresh") => match args.command.get(1..) {
            Some([]) => run_refresh(args.json),
            _ => {
                eprintln!("switchboard: refresh takes no repo");
                eprintln!("{USAGE}");
                ExitCode::from(2)
            }
        },
        Some("backup") => {
            let mut target = args.command[1..].to_vec();
            if all {
                target.push("--all".to_string());
            }
            run_backup(&target)
        }
        Some("delete" | "restore" | "merge") => write_command(&args, cwd, now, writes, err),
        Some("budget") => budget(args.json),
        Some("sky") => sky_command(args.json),
        Some("star") => match args.command.get(1..) {
            Some([target]) => star_command(target, args.json),
            _ => {
                eprintln!("switchboard: star needs one <repo>#<n>");
                eprintln!("{USAGE}");
                ExitCode::from(2)
            }
        },
        Some("waiting") => waiting(args.json),
        Some("reports") => reports(args.json, &filter),
        Some("loadout") => loadout_command(
            &it::read_this_machine(&it::SystemProbeRunner, SystemClock.now()),
            args.json,
            &mut std::io::stdout(),
        ),
        Some(other) => {
            eprintln!("switchboard: unknown command: {other}");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        None => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// `--all` taken out of `argv` (`parse_args` does not know it): whether it was there, and the
/// other words in order.
fn take_all(argv: &[String]) -> (bool, Vec<String>) {
    let rest: Vec<String> = argv.iter().filter(|w| *w != "--all").cloned().collect();
    (rest.len() != argv.len(), rest)
}

/// CLI-2: `switchboard refresh`, the Refresh button: the real GitHub transport over the app
/// data folder.
fn run_refresh(json: bool) -> ExitCode {
    let Some(data) = data_dir() else {
        eprintln!("switchboard: set {DATA_ENV} to the app data folder");
        return ExitCode::from(2);
    };
    let transport = live_transport();
    refresh_with(
        transport.as_ref(),
        &data,
        &SystemClock,
        json,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// CLI-2: `refresh` over its seams. Each repo `live::repos` names is tried: a 304 on the
/// events ETag keeps its snapshot (`unchanged`, GH-2), anything else is read as the Refresh
/// button reads it (`refreshed`). A repo that fails goes to `err` and the exit is 1 once all
/// were tried.
pub fn refresh_with(
    transport: &dyn GitHubTransport,
    data: &Path,
    clock: &dyn Clock,
    json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let repos = match live::repos(data) {
        Ok(repos) => repos,
        Err(e) => {
            let _ = writeln!(err, "switchboard: refresh: {e}");
            return ExitCode::from(1);
        }
    };
    if repos.is_empty() {
        let _ = writeln!(
            err,
            "switchboard: refresh: no repos (no snapshot and no priority copy)"
        );
        return ExitCode::from(1);
    }
    let store = SnapshotStore::new(data);
    let mut failed = false;
    let mut rows = Vec::new();
    for repo in &repos {
        match refresh_one(transport, &store, clock, repo) {
            Ok(status) => {
                rows.push(json!({"repo": repo, "status": status}));
                if !json {
                    let _ = writeln!(out, "{repo}: {status}");
                }
            }
            Err(e) => {
                failed = true;
                rows.push(json!({"repo": repo, "status": "failed", "error": e.to_string()}));
                let _ = writeln!(err, "{repo}: failed: {e}");
            }
        }
    }
    if json {
        let _ = writeln!(out, "{}", Value::Array(rows));
    }
    ExitCode::from(u8::from(failed))
}

/// One repo of `refresh`: `unchanged` or `refreshed`.
fn refresh_one(
    transport: &dyn GitHubTransport,
    store: &SnapshotStore,
    clock: &dyn Clock,
    repo: &str,
) -> Result<&'static str, CoreError> {
    let old = store.load(repo).ok().flatten();
    let old_etag = old.as_ref().and_then(|snap| snap.etag.as_deref());
    let (same, etag) = poll::unchanged(transport, repo, old_etag)?;
    if let (true, Some(mut snap)) = (same, old) {
        snap.read_at = clock.now();
        snap.etag = etag;
        store.save(&snap)?;
        return Ok("unchanged");
    }
    Poller {
        transport,
        store,
        clock,
        repos: vec![repo.to_string()],
    }
    .refresh_all()?;
    // Keep the ETag this check answered, so the next refresh can say `unchanged` (GH-2).
    if let Some(mut snap) = store.load(repo)? {
        snap.etag = etag;
        store.save(&snap)?;
    }
    Ok("refreshed")
}

/// CLI-2: `switchboard backup <repo>` or `backup --all` (`target` holds the repo, or
/// `--all`), the Back up buttons: the settings file in the app data folder and the real GitHub
/// backup.
fn run_backup(target: &[String]) -> ExitCode {
    let Some(data) = data_dir() else {
        eprintln!("switchboard: set {DATA_ENV} to the app data folder");
        return ExitCode::from(2);
    };
    let settings = match settings::load(&data.join("settings.json")) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("switchboard: backup: {e}");
            return ExitCode::from(1);
        }
    };
    backup_with(
        &settings,
        &data,
        target,
        SystemClock.now(),
        &|repo| backup::backup_repo(&settings, repo),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// CLI-2: `backup` over its seams: `back_up` backs one repo up (BAK-1). Each outcome is
/// recorded as the button records it (`backup_run::record`, so a refusal adds its Waiting on
/// you item, BAK-2). A refusal or a failure on one repo goes to `err`; the other repos are still
/// tried and the exit is 1. `target` is one repo, or `--all`; anything else is exit 2.
pub fn backup_with(
    settings: &Settings,
    data: &Path,
    target: &[String],
    now: UnixSeconds,
    back_up: &dyn Fn(&str) -> Result<BackupOutcome, CoreError>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let repos = match target {
        [flag] if flag == "--all" => match backup_run::repos(data) {
            Ok(repos) => repos,
            Err(e) => {
                let _ = writeln!(err, "switchboard: backup: {e}");
                return ExitCode::from(1);
            }
        },
        [repo] if !repo.starts_with("--") => vec![repo.clone()],
        _ => {
            let _ = writeln!(err, "switchboard: backup needs a repo or --all");
            let _ = writeln!(err, "{USAGE}");
            return ExitCode::from(2);
        }
    };
    let Some(backups) = settings
        .nextcloud_backups
        .as_deref()
        .filter(|path| !path.as_os_str().is_empty())
    else {
        let _ = writeln!(
            err,
            "switchboard: backup: {}",
            CoreError::MissingPath("nextcloud_backups")
        );
        return ExitCode::from(1);
    };
    let mut failed = false;
    for repo in &repos {
        let row: Result<BackupRow, CoreError> = back_up(repo)
            .and_then(|outcome| backup_run::record(data, backups, repo, &outcome, now));
        match row {
            Ok(row) => match &row.reason {
                None => {
                    let _ = writeln!(out, "{repo}: backed up to {}", row.folder);
                }
                Some(reason) => {
                    failed = true;
                    let _ = writeln!(err, "{repo}: {reason}");
                }
            },
            Err(e) => {
                failed = true;
                let _ = writeln!(err, "{repo}: failed: {e}");
            }
        }
    }
    ExitCode::from(u8::from(failed))
}

/// CLI-3: `delete <repo>`, `restore <repo>` and `merge <repo>#<pr>` through `rules::guard`. A
/// refusal prints the guard's text and exits 1; `ToWaiting` also adds its Waiting on you item.
/// A merge the guard allows goes to `writes`, which applies GH-5's rule.
fn write_command(
    args: &super::Args,
    cwd: &Path,
    now: UnixSeconds,
    writes: &dyn Writes,
    err: &mut dyn Write,
) -> ExitCode {
    let command = args.command.first().map(String::as_str).unwrap_or_default();
    let action = match (command, args.command.get(1)) {
        ("merge", Some(target)) => match parse_target(target) {
            Some((repo, pr)) => GuardedAction::Merge { repo, pr },
            None => {
                let _ = writeln!(err, "switchboard: merge needs <repo>#<pr>");
                return ExitCode::from(2);
            }
        },
        ("delete", Some(repo)) => GuardedAction::DeleteRepo { repo: repo.clone() },
        ("restore", Some(repo)) => GuardedAction::RestoreOverLive { repo: repo.clone() },
        _ => {
            let _ = writeln!(err, "switchboard: {command} needs a repo");
            return ExitCode::from(2);
        }
    };
    // `delete` and `restore` of a repo Switchboard does not know write nothing (fail closed
    // when it cannot tell).
    if let GuardedAction::DeleteRepo { repo } | GuardedAction::RestoreOverLive { repo } = &action {
        match writes.knows_repo(repo) {
            Ok(true) => {}
            Ok(false) => {
                let _ = writeln!(err, "switchboard: no such repo: {repo}");
                return ExitCode::from(1);
            }
            Err(e) => {
                let _ = writeln!(err, "switchboard: {command}: {e}");
                return ExitCode::from(1);
            }
        }
    }
    match rules::guard(&guard_for(args, cwd), &action) {
        Verdict::Allow => match &action {
            GuardedAction::Merge { repo, pr } => match writes.merge(repo, *pr) {
                Ok(()) => {
                    println!("merge {repo}#{pr}: done");
                    ExitCode::SUCCESS
                }
                Err(text) => refuse(err, &text),
            },
            // The guard allows only a merge from the CLI (CORE-3).
            _ => refuse(err, &request_text(&action)),
        },
        Verdict::ToWaiting(text) => {
            let asked = format!("I was asked from the CLI to {}", request_text(&action));
            if let Err(e) = writes.waiting(&guarded_item(&action, &asked, now)) {
                let _ = writeln!(err, "{e}");
            }
            refuse(err, &text)
        }
        Verdict::Refuse(text) => refuse(err, &text),
    }
}

/// CLI-3: who is asking. The face is the CLI; the role is the `--role` flag, already checked;
/// a job worktree is found from `cwd`.
fn guard_for(args: &super::Args, cwd: &Path) -> Caller {
    Caller {
        face: Face::Cli,
        role: args.role.as_deref().and_then(Role::parse),
        in_job_worktree: rules::is_job_worktree(cwd),
        confirmed_in_app: false,
    }
}

/// The action as the guard words it, e.g. `delete switchboard`.
fn request_text(action: &GuardedAction) -> String {
    match action {
        GuardedAction::Merge { repo, pr } => format!("merge {repo}#{pr}"),
        GuardedAction::DeleteRepo { repo } => format!("delete {repo}"),
        GuardedAction::RestoreOverLive { repo } => format!("restore over {repo}"),
    }
}

/// A refusal: its text on stderr, exit 1.
fn refuse(err: &mut dyn Write, text: &str) -> ExitCode {
    let _ = writeln!(err, "{text}");
    ExitCode::from(1)
}

/// `<repo>#<pr>`: a non-empty repo and a PR number from 1.
fn parse_target(word: &str) -> Option<(String, u64)> {
    let (repo, pr) = word.split_once('#')?;
    let pr: u64 = pr.parse().ok().filter(|pr| *pr >= 1)?;
    (!repo.is_empty()).then(|| (repo.to_string(), pr))
}

/// The REP-3 filter flags (`--project <repo>`, `--outcome <o>`, `--pool <p>`) taken out of
/// `argv`, with the other words kept in order. `parse_args` knows only `--json` and `--role`.
fn take_filter(argv: &[String]) -> Result<(Filter, Vec<String>), String> {
    let mut filter = Filter::default();
    let mut rest = Vec::new();
    let mut words = argv.iter();
    while let Some(word) = words.next() {
        let slot = match word.as_str() {
            "--project" => &mut filter.project,
            "--outcome" => &mut filter.outcome,
            "--pool" => &mut filter.pool,
            _ => {
                rest.push(word.clone());
                continue;
            }
        };
        match words.next() {
            Some(value) if !value.starts_with("--") => *slot = Some(value.clone()),
            _ => return Err(format!("{word} needs a value")),
        }
    }
    Ok((filter, rest))
}

/// An environment variable as a path, or `None` when unset or empty.
fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The app data folder: `SWITCHBOARD_DATA`, else the desktop app's folder for this OS.
pub(crate) fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = env_path(DATA_ENV) {
        return Some(dir);
    }
    let home = env_path("HOME");
    match current_os() {
        Os::Windows => env_path("APPDATA").map(|d| d.join(APP_DIR)),
        Os::Mac => home.map(|h| h.join("Library").join("Application Support").join(APP_DIR)),
        Os::Linux => env_path("XDG_DATA_HOME")
            .or_else(|| home.map(|h| h.join(".local").join("share")))
            .map(|d| d.join(APP_DIR)),
    }
}

/// The pipeline checkout (`SWITCHBOARD_PIPELINE`), or `None` when unset.
pub(crate) fn pipeline_dir() -> Option<PathBuf> {
    env_path(PIPELINE_ENV)
}

/// The pipeline checkout, or the exit code after saying it is not set.
fn pipeline_root() -> Result<PathBuf, ExitCode> {
    env_path(PIPELINE_ENV).ok_or_else(|| {
        eprintln!("switchboard: set {PIPELINE_ENV} to your pipeline checkout");
        ExitCode::from(2)
    })
}

/// `switchboard budget`: one line per pool (text) or the rows as a JSON array (`--json`).
fn budget(json: bool) -> ExitCode {
    let root = match pipeline_root() {
        Ok(root) => root,
        Err(code) => return code,
    };
    let rows = match budget::read(&FleetBridge::new(&root)) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("switchboard budget: {e}");
            return ExitCode::from(1);
        }
    };
    if json {
        match serde_json::to_string_pretty(&rows) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("switchboard budget: {e}");
                return ExitCode::from(1);
            }
        }
    } else {
        print!("{}", budget_text(&rows));
    }
    ExitCode::SUCCESS
}

/// One line per pool, in row order: `<pool>: <pct>% vs pace <pace>%, <verdict>`.
fn budget_text(rows: &[PoolRow]) -> String {
    let percent = |value: Option<f64>| match value {
        Some(v) => format!("{v}%"),
        None => NOT_RECORDED.to_string(),
    };
    rows.iter()
        .map(|row| {
            format!(
                "{}: {} vs pace {}, {}\n",
                row.pool,
                percent(row.pct),
                percent(row.pace),
                row.verdict
            )
        })
        .collect()
}

/// Every stored snapshot, or the exit code after saying why there is none.
fn snapshots(command: &str) -> Result<(PathBuf, Vec<RepoSnapshot>), ExitCode> {
    let Some(data) = data_dir() else {
        eprintln!("switchboard: set {DATA_ENV} to the app data folder");
        return Err(ExitCode::from(2));
    };
    match SnapshotStore::new(&data).load_all() {
        Ok(snaps) if snaps.is_empty() => {
            eprintln!("{NO_SNAPSHOT}");
            Err(ExitCode::from(1))
        }
        Ok(snaps) => Ok((data, snaps)),
        Err(e) => {
            eprintln!("switchboard {command}: {e}");
            Err(ExitCode::from(1))
        }
    }
}

/// `switchboard waiting`: the Waiting on you items from the stored snapshots and the local items
/// (WAIT-1), newest first: one line each (text) or the items as a JSON array (`--json`). CalDAV
/// is not read yet (U7).
fn waiting(json: bool) -> ExitCode {
    let Some(data) = data_dir() else {
        eprintln!("switchboard: set {DATA_ENV} to the app data folder");
        return ExitCode::from(2);
    };
    let snaps = match SnapshotStore::new(&data).load_all() {
        Ok(snaps) => snaps,
        Err(e) => {
            eprintln!("switchboard waiting: {e}");
            return ExitCode::from(1);
        }
    };
    let local = match LocalItems::new(&data).load() {
        Ok(local) => local,
        Err(e) => {
            eprintln!("switchboard waiting: {e}");
            return ExitCode::from(1);
        }
    };
    if snaps.is_empty() && local.is_empty() {
        eprintln!("{NO_SNAPSHOT}");
        return ExitCode::from(1);
    }
    let items = waiting::assemble(&snaps, &local);
    if json {
        return print_json("waiting", serde_json::to_string_pretty(&items));
    }
    print!("{}", waiting_text(&items));
    ExitCode::SUCCESS
}

/// One line per item, in list order: `<what> | <why> | <action> | <link>`; no items gives the
/// nothing-waiting line.
fn waiting_text(items: &[waiting::Item]) -> String {
    if items.is_empty() {
        return format!("{NOTHING_WAITING}\n");
    }
    items
        .iter()
        .map(|item| {
            format!(
                "{} | {} | {} | {}\n",
                item.what, item.why, item.action, item.link
            )
        })
        .collect()
}

/// `switchboard reports`: the reports from the fleet's done cards and the job log, newest first,
/// narrowed by the filter flags: one line each (text) or the reports as a JSON array (`--json`).
fn reports(json: bool, filter: &Filter) -> ExitCode {
    let root = match pipeline_root() {
        Ok(root) => root,
        Err(code) => return code,
    };
    let fleet = FleetBridge::new(&root);
    let cards = match fleet.run_json(&["cards"]).map_err(CoreError::Fleet) {
        Ok(cards) => cards,
        Err(e) => {
            eprintln!("switchboard reports: {e}");
            return ExitCode::from(1);
        }
    };
    let Some(cards) = cards.as_array() else {
        eprintln!("switchboard reports: fleet cards is not a JSON array");
        return ExitCode::from(1);
    };
    let state = env_path(STATE_ENV)
        .or_else(data_dir)
        .unwrap_or_else(|| root.clone());
    let events = match joblog::read_ledger(&joblog::ledger_files(&root, &state)) {
        Ok(events) => events,
        Err(e) => {
            eprintln!("switchboard reports: {e}");
            return ExitCode::from(1);
        }
    };
    let rows = joblog::read_all(&events, &[]);
    let all: Vec<Report> = cards
        .iter()
        .map(|card| reports::from_card(card, &rows))
        .collect();
    let sorted = reports::newest_first(&all);
    let shown = reports::filter(&sorted, filter);
    if json {
        return print_json("reports", serde_json::to_string_pretty(&shown));
    }
    print!("{}", reports_text(&shown));
    ExitCode::SUCCESS
}

/// One line per report, in list order: `<title> | <outcome> | <project>`; no reports gives the
/// no-reports line.
fn reports_text(reports: &[&reports::Report]) -> String {
    if reports.is_empty() {
        return format!("{NO_REPORTS}\n");
    }
    reports
        .iter()
        .map(|report| {
            format!(
                "{} | {} | {}\n",
                joblog::shown(&report.title),
                report.outcome,
                joblog::shown(&report.project)
            )
        })
        .collect()
}

/// Print the JSON on stdout; a failure is named by `command` on stderr, exit 1.
fn print_json(command: &str, json: serde_json::Result<String>) -> ExitCode {
    match json {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("switchboard {command}: {e}");
            ExitCode::from(1)
        }
    }
}

/// One constellation as `sky` prints it.
struct SkyRow {
    repo: String,
    brightness: f64,
    open: usize,
    drifted: usize,
}

/// `switchboard sky`: one row per repo in the Sky view's placement order, from the same
/// `sky::model` the window paints.
fn sky_command(json: bool) -> ExitCode {
    let (data, snaps) = match snapshots("sky") {
        Ok(found) => found,
        Err(code) => return code,
    };
    let model = match sky::model::load(&data, SystemClock.now()) {
        Ok(model) => model,
        Err(e) => {
            eprintln!("switchboard sky: {e}");
            return ExitCode::from(1);
        }
    };
    let rows: Vec<SkyRow> = model
        .constellations
        .iter()
        .map(|c| SkyRow {
            repo: c.repo.clone(),
            brightness: c.brightness,
            open: snaps
                .iter()
                .find(|s| s.repo == c.repo)
                .map_or(0, |s| s.issues.iter().filter(|i| i.state == "open").count()),
            drifted: c.drift.len(),
        })
        .collect();
    if json {
        let rows: Vec<Value> = rows
            .iter()
            .map(|row| {
                json!({
                    "repo": row.repo,
                    "brightness": row.brightness,
                    "open": row.open,
                    "drifted": row.drifted,
                })
            })
            .collect();
        print_json("sky", serde_json::to_string_pretty(&Value::Array(rows)))
    } else {
        print!("{}", sky_text(&rows));
        ExitCode::SUCCESS
    }
}

/// One line per repo: `<repo>: brightness <b>, <n> open, <d> drifted`.
fn sky_text(rows: &[SkyRow]) -> String {
    rows.iter()
        .map(|row| {
            format!(
                "{}: brightness {:.2}, {} open, {} drifted\n",
                row.repo, row.brightness, row.open, row.drifted
            )
        })
        .collect()
}

/// The PR that closes a star, as `star` prints it.
struct StarPr {
    number: u64,
    state: String,
    /// The last R25 verdict, `PASS` or `FAIL`.
    review: Option<String>,
    /// The combined check state on the head.
    ci: String,
}

/// One issue as `star` prints it.
struct Star {
    repo: String,
    number: u64,
    title: String,
    state: String,
    labels: Vec<String>,
    size: Option<String>,
    pr: Option<StarPr>,
    /// The fleet job running on it; `None` inside `Some` when the job log holds none, and the
    /// outer `None` when the job log was not read (no pipeline checkout set).
    job: Option<Option<String>>,
}

/// `switchboard star <repo>#<n>`: the facts the Sky view's star card shows. `<repo>` is
/// `owner/name` or the bare name.
fn star_command(target: &str, json: bool) -> ExitCode {
    let Some((repo, number)) = target
        .rsplit_once('#')
        .and_then(|(repo, n)| Some((repo, n.parse::<u64>().ok()?)))
        .filter(|(repo, _)| !repo.is_empty())
    else {
        eprintln!("switchboard: star needs <repo>#<n>, got {target}");
        return ExitCode::from(2);
    };
    let (_, snaps) = match snapshots("star") {
        Ok(found) => found,
        Err(code) => return code,
    };
    let suffix = format!("/{repo}");
    let snap = snaps
        .iter()
        .find(|s| s.repo == repo)
        .or_else(|| snaps.iter().find(|s| s.repo.ends_with(suffix.as_str())));
    let Some((snap, issue)) =
        snap.and_then(|s| Some((s, s.issues.iter().find(|i| i.number == number)?)))
    else {
        eprintln!("switchboard: no such star: {target}");
        return ExitCode::from(1);
    };
    let job = match running_job(&snap.repo, number) {
        Ok(job) => job,
        Err(e) => {
            eprintln!("switchboard star: {e}");
            return ExitCode::from(1);
        }
    };
    let star = Star {
        repo: snap.repo.clone(),
        number,
        title: issue.title.clone(),
        state: issue.state.clone(),
        labels: issue.labels.clone(),
        size: issue
            .labels
            .iter()
            .find_map(|label| label.strip_prefix("size:"))
            .map(str::to_string),
        pr: snap
            .pulls
            .iter()
            .find(|pr| pr.closes.contains(&number))
            .map(|pr| StarPr {
                number: pr.number,
                state: pr.state.clone(),
                review: pr
                    .verdicts
                    .last()
                    .filter(|v| matches!(v.verdict.as_str(), "PASS" | "FAIL"))
                    .map(|v| v.verdict.clone()),
                ci: pr.ci_on_head.clone(),
            }),
        job,
    };
    if json {
        let job = match &star.job {
            Some(job) => json!(job),
            None => json!(NOT_RECORDED),
        };
        print_json(
            "star",
            serde_json::to_string_pretty(&json!({
                "repo": star.repo,
                "number": star.number,
                "title": star.title,
                "state": star.state,
                "labels": star.labels,
                "size": star.size,
                "pr": star.pr.as_ref().map(|pr| json!({
                    "number": pr.number,
                    "state": pr.state,
                    "review": pr.review,
                    "ci": pr.ci,
                })),
                "job": job,
            })),
        )
    } else {
        print!("{}", star_text(&star));
        ExitCode::SUCCESS
    }
}

/// One fact per line, the issue first.
fn star_text(star: &Star) -> String {
    let or_none = |value: Option<&str>| value.unwrap_or("none").to_string();
    let labels = if star.labels.is_empty() {
        "none".to_string()
    } else {
        star.labels.join(", ")
    };
    let (pr, review, ci) = match &star.pr {
        Some(pr) => (
            format!("#{} {}", pr.number, pr.state),
            or_none(pr.review.as_deref()),
            pr.ci.clone(),
        ),
        None => ("none".to_string(), "none".to_string(), "none".to_string()),
    };
    let job = match &star.job {
        Some(job) => or_none(job.as_deref()),
        None => NOT_RECORDED.to_string(),
    };
    format!(
        "{}#{}: {}\nstate: {}\nlabels: {labels}\nsize: {}\npr: {pr}\nreview: {review}\nci: {ci}\njob: {job}\n",
        star.repo,
        star.number,
        star.title,
        star.state,
        or_none(star.size.as_deref()),
    )
}

/// The fleet job running on `repo#number`: the last ledger `start` event for it with no `end`
/// after it, as `<lane> <model> on <machine>, attempt <n>`. `Ok(None)` when the pipeline
/// checkout is not set, so the job log cannot be read.
fn running_job(repo: &str, number: u64) -> Result<Option<Option<String>>, String> {
    let Some(pipeline) = env_path(PIPELINE_ENV) else {
        return Ok(None);
    };
    let state = env_path(STATE_ENV)
        .or_else(|| env_path("FLEET_STATE_DIR"))
        .unwrap_or_else(|| {
            let mut name = pipeline.file_name().unwrap_or_default().to_os_string();
            name.push("-fleet-state");
            pipeline.with_file_name(name)
        });
    let events =
        joblog::read_ledger(&joblog::ledger_files(&pipeline, &state)).map_err(|e| e.to_string())?;
    let short = repo.rsplit('/').next().unwrap_or(repo);
    let ids = [format!("{repo}#{number}"), format!("{short}#{number}")];
    let field = |event: &Value, key: &str| -> Option<String> {
        match event.get(key)? {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    };
    let mut running: Option<&Value> = None;
    for event in &events {
        if !field(event, "id").is_some_and(|id| ids.contains(&id)) {
            continue;
        }
        match event.get("event").and_then(Value::as_str) {
            Some("start") => running = Some(event),
            Some("end") => {
                let same_machine = running.is_some_and(|start| {
                    field(start, "machine").is_none()
                        || field(event, "machine").is_none()
                        || field(start, "machine") == field(event, "machine")
                });
                if same_machine {
                    running = None;
                }
            }
            _ => {}
        }
    }
    Ok(Some(running.map(|start| {
        let mut text = [field(start, "lane"), field(start, "model")]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            text = "a job".to_string();
        }
        if let Some(machine) = field(start, "machine") {
            text.push_str(&format!(" on {machine}"));
        }
        if let Some(attempt) = field(start, "attempt") {
            text.push_str(&format!(", attempt {attempt}"));
        }
        text
    })))
}

/// The toolchains the core probes, in the Loadout panel's order (`it.rs` `LOADOUT_TOOLCHAINS`).
const TOOLCHAINS: [&str; 3] = ["cargo", "node", "python3"];
/// The line `loadout` prints when this OS has no probes.
const NO_PROBES: &str = "no loadout probes on this OS";

/// `switchboard loadout`: the text of `loadout_text`, or the `Loadout` as pretty JSON
/// (`--json`), written to `out`; exit 0.
pub fn loadout_command(loadout: &Loadout, json: bool, out: &mut dyn Write) -> ExitCode {
    let text = if json {
        match serde_json::to_string_pretty(loadout) {
            Ok(text) => format!("{text}\n"),
            Err(e) => {
                eprintln!("switchboard loadout: {e}");
                return ExitCode::from(1);
            }
        }
    } else {
        loadout_text(loadout)
    };
    if let Err(e) = out.write_all(text.as_bytes()) {
        eprintln!("switchboard loadout: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// One line per probe, in the Loadout panel's order: `<name>  <state>`, then `  <detail>` when
/// there is one. No probes gives the no-probes line.
pub fn loadout_text(loadout: &Loadout) -> String {
    if loadout.clis.is_empty()
        && loadout.toolchains.is_empty()
        && loadout.models_loaded.is_empty()
        && loadout.recent_errors.is_empty()
        && !loadout.busy
    {
        return format!("{NO_PROBES}\n");
    }
    // (name, state, detail): the detail is empty when the row has none.
    let row =
        |name: &str, state: &str, detail: String| (name.to_string(), state.to_string(), detail);
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for (name, signed_in) in &loadout.clis {
        let state = if *signed_in {
            "signed in"
        } else {
            "not signed in"
        };
        rows.push(row(name, state, String::new()));
    }
    let mut toolchains: Vec<&str> = TOOLCHAINS.to_vec();
    let extra = loadout.toolchains.iter().map(String::as_str);
    toolchains.extend(extra.filter(|name| !TOOLCHAINS.contains(name)));
    for name in &toolchains {
        let found = loadout.toolchains.iter().any(|t| t.as_str() == *name);
        let state = if found { "found" } else { "not found" };
        rows.push(row(name, state, String::new()));
    }
    let loaded = loadout.models_loaded.len();
    let loaded_state = if loaded > 0 {
        format!("{loaded} loaded")
    } else {
        "no models loaded".to_string()
    };
    let models = loadout.models_loaded.join(", ");
    rows.push(row("llama-swap", &loaded_state, models));
    let status = if loadout.busy { "busy" } else { "idle" };
    rows.push(row("fleet status", status, String::new()));
    let errors = loadout.recent_errors.len();
    let errors_state = if errors > 0 {
        format!("{errors} recent")
    } else {
        "none".to_string()
    };
    let error_list = loadout.recent_errors.join("; ");
    rows.push(row("fleet errors", &errors_state, error_list));
    rows.into_iter()
        .map(|(name, state, detail)| {
            if detail.is_empty() {
                format!("{name}  {state}\n")
            } else {
                format!("{name}  {state}  {detail}\n")
            }
        })
        .collect()
}

#[cfg(test)]
mod early_answer_tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn help_spellings_answer_with_the_usage_text() {
        for word in ["--help", "-h", "help"] {
            assert_eq!(
                early_answer(&argv(&[word])),
                Some(USAGE.to_string()),
                "{word}"
            );
        }
    }

    #[test]
    fn version_spellings_answer_with_the_crate_version() {
        let expected = format!("switchboard {}", env!("CARGO_PKG_VERSION"));
        for word in ["--version", "-V"] {
            assert_eq!(
                early_answer(&argv(&[word])),
                Some(expected.clone()),
                "{word}"
            );
        }
    }

    #[test]
    fn a_real_command_gets_no_early_answer() {
        assert_eq!(early_answer(&argv(&["sky"])), None);
    }
}
