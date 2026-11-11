//! CLI-1: `switchboard sky | waiting | reports | budget | loadout | star <repo>#<n>`, as text or
//! `--json`, printing what the GUI shows. `errors` (#375) lists the fleet errors the app
//! recorded and the open IT tickets.
//! CLI-2: `switchboard refresh | backup <repo> | backup --all`, the GUI buttons.
//! CLI-3: a repo delete or a restore over a live repo is refused and becomes a Waiting on you
//! item; `--role <role> merge <repo>#<pr>` goes through CORE-3; inside a job worktree no role
//! gives write actions. `--role budget-manager set_pool_cap <pool> <cap>` and `pool_edit <pool>
//! [--plan <p>] [--cap <n>] [--reset <day time>] [--cost <n>]` edit a pool through
//! `rules::guard_pool_edit` and `budget::save_pool_edit`, as MCP's tools do (#357, #374).
//! `budget --refresh` re-reads the pools first (#374); `settings` prints the settings file (#373).
//! The front door (#384): `ask`, `ask_as_job` and `rate`, in `cli_frontdoor.rs`.
//!
//! The raw `fleet` CLI stays the documented escape hatch: Switchboard keeps no state that only
//! it can read.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{json, Value};
use switchboard_core::backup::{self, BackupOutcome};
use switchboard_core::backup_run::{self, BackupRow};
use switchboard_core::budget::{self, PoolEdit, PoolRow};
use switchboard_core::clock::{Clock, SystemClock, UnixSeconds};
use switchboard_core::error::CoreError;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::fleet_errors::{self, FleetErrorRecord};
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

#[path = "cli_frontdoor.rs"]
pub mod frontdoor;

/// The environment variable that names the pipeline checkout. There is no default.
pub const PIPELINE_ENV: &str = "SWITCHBOARD_PIPELINE";
/// The environment variable that names the app data folder (the snapshot is in `snapshot/`).
/// Unset, the folder is the desktop app's own (see `data_dir`).
pub const DATA_ENV: &str = "SWITCHBOARD_DATA";
/// The environment variable that names the app config folder (where the desktop app keeps
/// `settings.json`). Unset, it falls back to `SWITCHBOARD_DATA`, then the desktop app's own (see
/// `config_dir`).
pub const CONFIG_ENV: &str = "SWITCHBOARD_CONFIG";
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

pub const USAGE: &str = "usage: switchboard [--help | --version] [--json] [--role <role>] \
                     [--project <repo>] [--outcome <o>] [--pool <p>] \
                     <sky | waiting | reports | errors | budget [--refresh] | loadout | settings | \
                     ask \"<text>\" [--dry-run] [--thread <id>] | \
                     ask_as_job <kind> \"<text>\" --unmet <need>[,<need>] | \
                     rate <thread> <turn> <up|down> | \
                     star <repo>#<n> | refresh | backup <repo> | backup --all | delete <repo> | \
                     restore <repo> | merge <repo>#<pr> | set_pool_cap <pool> <cap> | \
                     pool_edit <pool> [--plan <p>] [--cap <n>] [--reset <day time>] [--cost <n>] | \
                     autopilot <roadmap | review | merge | weekly | satellite> [--dry-run] | \
                     autopilot status | mcp>";

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
    /// The `owner/name` that `repo` names among the repos Switchboard knows (#346): `Ok(None)`
    /// when it knows none, an error when a bare name matches more than one owner or it cannot
    /// tell. The default asks `knows_repo` and keeps `repo` as given.
    fn full_repo(&self, repo: &str) -> Result<Option<String>, String> {
        self.knows_repo(repo)
            .map(|known| known.then(|| repo.to_string()))
    }
}

/// Whether `repo` names one of `known` (each `owner/name`): the whole `owner/name`, or, when
/// `repo` has no `/`, the name part.
pub fn repo_known(repo: &str, known: &[String]) -> bool {
    known
        .iter()
        .any(|full| full == repo || (!repo.contains('/') && full.rsplit('/').next() == Some(repo)))
}

/// #346: the one `owner/name` of `known` that `repo` names: the whole `owner/name`, or, when
/// `repo` has no `/`, the only one whose name part it is. `Ok(None)` when none does; an error
/// naming the matches when a bare name matches more than one owner.
pub fn resolve_repo<'a>(
    repo: &str,
    known: impl IntoIterator<Item = &'a str>,
) -> Result<Option<&'a str>, String> {
    let known: Vec<&'a str> = known.into_iter().collect();
    if let Some(full) = known.iter().find(|full| **full == repo) {
        return Ok(Some(full));
    }
    if repo.contains('/') {
        return Ok(None);
    }
    let mut matches: Vec<&'a str> = known
        .into_iter()
        .filter(|full| full.contains('/') && full.rsplit('/').next() == Some(repo))
        .collect();
    matches.sort_unstable();
    matches.dedup();
    match matches.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        many => Err(format!(
            "{repo} matches {}; name it as <owner>/{repo}",
            many.join(" and ")
        )),
    }
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
            pipeline: pipeline_dir(),
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
        let snaps = readable_snapshots(&store, repo)?;
        let full = resolve_repo(repo, snaps.iter().map(|snap| snap.repo.as_str()))?;
        let snap = full
            .and_then(|full| snaps.iter().find(|snap| snap.repo == full))
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
            carried_over: switchboard_core::github::merge::carried_over_by_patch_id(&clone, pull),
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
        self.full_repo(repo).map(|full| full.is_some())
    }

    /// #346: `repo` resolved against the stored snapshots' `owner/name`s.
    fn full_repo(&self, repo: &str) -> Result<Option<String>, String> {
        let data = self
            .data
            .as_deref()
            .ok_or("set SWITCHBOARD_DATA to the app data folder")?;
        let snaps = readable_snapshots(&SnapshotStore::new(data), repo)?;
        resolve_repo(repo, snaps.iter().map(|snap| snap.repo.as_str()))
            .map(|full| full.map(str::to_string))
    }
}

/// The stored snapshots for a write on `repo`. A snapshot file that does not read and could be
/// `repo` (its `owner/name`, or the name part for a bare name) is the error, so `merge` and
/// `delete` never work from a view missing the repo they name, nor pick another owner's repo of
/// the same name (#350).
fn readable_snapshots(store: &SnapshotStore, repo: &str) -> Result<Vec<RepoSnapshot>, String> {
    let (snaps, bad) = store.load_each().map_err(|e| e.to_string())?;
    match bad
        .iter()
        .find(|file| repo_known(repo, std::slice::from_ref(&file.repo)))
    {
        Some(file) => Err(format!(
            "{}; run switchboard refresh",
            terminal_safe(&file.error)
        )),
        None => Ok(snaps),
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
pub(crate) fn live_transport() -> Box<dyn GitHubTransport> {
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

/// The line `--version` prints.
pub(crate) fn version_text() -> String {
    format!("switchboard {}", env!("CARGO_PKG_VERSION"))
}

/// The text to print for a line that asks for help or the version, or `None` when the argv is a
/// real command (#355): `--help`, `-h`, `--version` or `-V` anywhere on the line, or `help` as
/// the first word. The first such word wins.
fn early_answer(argv: &[String]) -> Option<String> {
    if argv.first().map(String::as_str) == Some("help") {
        return Some(USAGE.to_string());
    }
    argv.iter().find_map(|word| match word.as_str() {
        "--help" | "-h" => Some(USAGE.to_string()),
        "--version" | "-V" => Some(version_text()),
        _ => None,
    })
}

/// Write `text` to stdout (#334): exit 0 once it is written, and when the reader has closed the
/// pipe (a quiet exit, nothing more to say); any other write error is one line on stderr and
/// exit 1. Never a panic.
pub(crate) fn emit(text: &str) -> ExitCode {
    emit_to(&mut std::io::stdout().lock(), text)
}

/// `emit` over its seam: the writer stdout would be.
pub fn emit_to(out: &mut dyn Write, text: &str) -> ExitCode {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(
                std::io::stderr(),
                "switchboard: cannot write the output: {e}"
            );
            ExitCode::from(1)
        }
    }
}

/// #352: text from GitHub or the fleet made safe for one line of a terminal: each control
/// character (an escape sequence's ESC, a newline, a tab, BEL) is written as its Rust escape
/// (`\u{1b}`, `\n`), so a title can neither drive the terminal nor forge a row. `--json`
/// already escapes them.
pub fn terminal_safe(text: &str) -> String {
    if !text.chars().any(char::is_control) {
        return text.to_string();
    }
    text.chars()
        .map(|c| {
            if c.is_control() {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// #356: a command that takes no more words, given some: the usage error, exit 2.
fn no_more_words(args: &super::Args) -> Option<ExitCode> {
    let extra = args.command.get(1..).filter(|rest| !rest.is_empty())?;
    let command = &args.command[0];
    eprintln!(
        "switchboard: {command} takes no other words, got {}",
        extra.join(" ")
    );
    eprintln!("{USAGE}");
    Some(ExitCode::from(2))
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
        return emit(&format!("{text}\n"));
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
    let (pool_flags, argv) = match take_pool_flags(&argv) {
        Ok(taken) => taken,
        Err(e) => {
            eprintln!("switchboard: {e}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let (door_flags, argv) = match frontdoor::take_flags(&argv) {
        Ok(taken) => taken,
        Err(e) => {
            eprintln!("switchboard: {e}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
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
    if pool_flags.any_edit() && command != Some("pool_edit") {
        eprintln!("switchboard: --plan, --cap, --reset and --cost are for pool_edit");
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    if pool_flags.refresh && command != Some("budget") {
        eprintln!("switchboard: --refresh is for budget");
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    if let Some(e) = frontdoor::misplaced(&door_flags, command) {
        eprintln!("switchboard: {e}");
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    if all && command != Some("backup") {
        eprintln!("switchboard: --all is for backup");
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    if matches!(
        command,
        Some("sky" | "waiting" | "reports" | "errors" | "budget" | "loadout" | "settings")
    ) {
        if let Some(code) = no_more_words(&args) {
            return code;
        }
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
        Some("budget") => budget(args.json, pool_flags.refresh),
        Some("set_pool_cap" | "pool_edit") => {
            pool_command(&args, &pool_flags, cwd, now, err, &mut std::io::stdout())
        }
        Some("settings") => settings_command(args.json, &mut std::io::stdout()),
        Some("ask" | "ask_as_job" | "rate") => frontdoor::command(
            &args.command,
            args.json,
            &door_flags,
            &guard_for(&args, cwd),
            err,
            &mut std::io::stdout(),
        ),
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
        Some("errors") => errors_command(args.json),
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
/// were tried. A stdout write that fails is said once (`emit_to`) and the exit is 1.
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
    let mut written = true;
    let mut rows = Vec::new();
    for repo in &repos {
        match refresh_one(transport, &store, clock, repo) {
            Ok(status) => {
                rows.push(json!({"repo": repo, "status": status}));
                if !json {
                    emit_more(out, &mut written, &format!("{repo}: {status}\n"));
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
        emit_more(out, &mut written, &format!("{}\n", Value::Array(rows)));
    }
    ExitCode::from(u8::from(failed || !written))
}

/// `emit_to` for one of several outputs: once a write has failed (`written` false), the rest are
/// skipped, so the error is said once and the caller exits 1.
fn emit_more(out: &mut dyn Write, written: &mut bool, text: &str) {
    if *written {
        *written = emit_to(out, text) == ExitCode::SUCCESS;
    }
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
/// `--all`), the Back up buttons: the settings file in the app config folder and the real GitHub
/// backup.
fn run_backup(target: &[String]) -> ExitCode {
    // A target that is not one repo or `--all` is the usage error before any folder or
    // settings file is read.
    if !matches!(target, [one] if one == "--all" || !one.starts_with("--")) {
        eprintln!("switchboard: backup needs a repo or --all");
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let Some(data) = data_dir() else {
        eprintln!("switchboard: set {DATA_ENV} to the app data folder");
        return ExitCode::from(2);
    };
    let Some(config) = config_dir() else {
        eprintln!("switchboard: set {CONFIG_ENV} to the app config folder");
        return ExitCode::from(2);
    };
    let settings = match settings::load(&config.join("settings.json")) {
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
/// tried and the exit is 1. `target` is one repo, or `--all`; anything else is exit 2. A
/// stdout write that fails is said once (`emit_to`) and the exit is 1.
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
    let mut written = true;
    for repo in &repos {
        let row: Result<BackupRow, CoreError> = back_up(repo)
            .and_then(|outcome| backup_run::record(data, backups, repo, &outcome, now));
        match row {
            Ok(row) => match &row.reason {
                None => {
                    let renamed = row
                        .manifest
                        .as_ref()
                        .and_then(|m| m.resolved_repo.as_deref())
                        .map(|now| {
                            format!(
                                " (GitHub answered for {}: renamed or moved)",
                                terminal_safe(now)
                            )
                        })
                        .unwrap_or_default();
                    let line = format!("{repo}: backed up to {}{renamed}\n", row.folder);
                    emit_more(out, &mut written, &line);
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
    ExitCode::from(u8::from(failed || !written))
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
    if let Some(extra) = args.command.get(2..).filter(|rest| !rest.is_empty()) {
        let what = if command == "merge" {
            "one <repo>#<pr>"
        } else {
            "one repo"
        };
        let _ = writeln!(
            err,
            "switchboard: {command} takes {what}, got more: {}",
            extra.join(" ")
        );
        let _ = writeln!(err, "{USAGE}");
        return ExitCode::from(2);
    }
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
    // when it cannot tell); a bare name is stored as the one `owner/name` it names (#346), so
    // the item links to the repo and asking with either spelling keeps one item.
    let mut action = action;
    match &mut action {
        GuardedAction::DeleteRepo { repo } | GuardedAction::RestoreOverLive { repo } => {
            match writes.full_repo(repo) {
                Ok(Some(full)) => *repo = full,
                Ok(None) => {
                    let _ = writeln!(err, "switchboard: no such repo: {repo}");
                    return ExitCode::from(1);
                }
                Err(e) => {
                    let _ = writeln!(err, "switchboard: {command}: {e}");
                    return ExitCode::from(1);
                }
            }
        }
        // A merge never asks here: `Writes::merge` resolves the repo itself and refuses a bare
        // name that matches more than one owner.
        GuardedAction::Merge { .. } => {}
    }
    match rules::guard(&guard_for(args, cwd), &action) {
        Verdict::Allow => match &action {
            GuardedAction::Merge { repo, pr } => match writes.merge(repo, *pr) {
                Ok(()) => emit(&format!("merge {repo}#{pr}: done\n")),
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

/// BUD-2, BUD-3 (#357, #374): the pool flags taken out of `argv`: `pool_edit`'s `--plan`,
/// `--cap`, `--reset` and `--cost`, each with its value as given, and `budget --refresh`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoolFlags {
    pub plan: Option<String>,
    pub cap: Option<String>,
    pub reset: Option<String>,
    pub cost: Option<String>,
    pub refresh: bool,
}

impl PoolFlags {
    /// Whether any of `pool_edit`'s flags was given.
    fn any_edit(&self) -> bool {
        self.plan.is_some() || self.cap.is_some() || self.reset.is_some() || self.cost.is_some()
    }
}

/// `PoolFlags` out of `argv`, with the other words kept in order. A value flag with no value is
/// an error; a value may start with `-` (a negative number is the core's refusal), not `--`.
pub fn take_pool_flags(argv: &[String]) -> Result<(PoolFlags, Vec<String>), String> {
    let mut flags = PoolFlags::default();
    let mut rest = Vec::new();
    let mut words = argv.iter();
    while let Some(word) = words.next() {
        let slot = match word.as_str() {
            "--plan" => &mut flags.plan,
            "--cap" => &mut flags.cap,
            "--reset" => &mut flags.reset,
            "--cost" => &mut flags.cost,
            "--refresh" => {
                flags.refresh = true;
                continue;
            }
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
    Ok((flags, rest))
}

/// A number word for `field` (`cap`, `cost`): an error names it when it is not a number.
fn number_word(field: &str, word: &str) -> Result<f64, String> {
    word.trim()
        .parse::<f64>()
        .map_err(|_| format!("{field} must be a number, got {}", terminal_safe(word)))
}

/// The pool edit a `set_pool_cap <pool> <cap>` or `pool_edit <pool> [--plan ..]` line asks for,
/// or the usage error.
fn pool_edit_from(args: &super::Args, flags: &PoolFlags) -> Result<PoolEdit, String> {
    let command = args.command.first().map(String::as_str).unwrap_or_default();
    match (command, args.command.get(1..).unwrap_or_default()) {
        ("set_pool_cap", [pool, cap]) => Ok(PoolEdit {
            pool: pool.clone(),
            cap: Some(number_word("cap", cap)?),
            ..PoolEdit::default()
        }),
        ("set_pool_cap", _) => Err("set_pool_cap needs <pool> <cap>".to_string()),
        ("pool_edit", [pool]) => {
            if !flags.any_edit() {
                return Err(
                    "pool_edit needs at least one of --plan, --cap, --reset, --cost".to_string(),
                );
            }
            Ok(PoolEdit {
                pool: pool.clone(),
                plan: flags.plan.clone(),
                cap: flags
                    .cap
                    .as_deref()
                    .map(|w| number_word("cap", w))
                    .transpose()?,
                reset: flags.reset.clone(),
                cost_usd: flags
                    .cost
                    .as_deref()
                    .map(|w| number_word("cost", w))
                    .transpose()?,
            })
        }
        _ => Err("pool_edit needs one <pool>".to_string()),
    }
}

/// CLI-3, BUD-2 (#357, #374): `--role budget-manager set_pool_cap <pool> <cap>` and
/// `--role budget-manager pool_edit <pool> [--plan <p>] [--cap <n>] [--reset <day time>]
/// [--cost <n>]`: the same check (`rules::guard_pool_edit`) and the same save
/// (`budget::save_pool_edit`: file, ledger row and Waiting item, all or nothing) as MCP's
/// tools of the same names. Text says what was set and what it was (#362); `--json` prints
/// the ledger row and the budget rows read again. A bad line is exit 2; a refusal or a save
/// that fails is its text on stderr, exit 1.
pub fn pool_command(
    args: &super::Args,
    flags: &PoolFlags,
    cwd: &Path,
    now: UnixSeconds,
    err: &mut dyn Write,
    out: &mut dyn Write,
) -> ExitCode {
    let command = args.command.first().map(String::as_str).unwrap_or_default();
    let edit = match pool_edit_from(args, flags) {
        Ok(edit) => edit,
        Err(e) => {
            let _ = writeln!(err, "switchboard: {e}");
            let _ = writeln!(err, "{USAGE}");
            return ExitCode::from(2);
        }
    };
    if let Verdict::Refuse(text) | Verdict::ToWaiting(text) =
        rules::guard_pool_edit(&guard_for(args, cwd), &edit.pool)
    {
        return refuse(err, &terminal_safe(&text));
    }
    let (root, data) = match pipeline_or_fail().and_then(|root| Ok((root, data_or_fail()?))) {
        Ok(found) => found,
        Err(fail) => {
            let _ = writeln!(err, "{}", fail.message);
            return ExitCode::from(fail.code);
        }
    };
    let clock = switchboard_core::clock::FixedClock::at(now);
    let saved = match budget::save_pool_edit(&FleetBridge::new(&root), &edit, &data, &clock) {
        Ok(saved) => saved,
        Err(e) => {
            // An empty pool name leaves no gap before the colon (#416).
            let who = [command, edit.pool.as_str()]
                .iter()
                .filter(|w| !w.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            let _ = writeln!(err, "{}", terminal_safe(&format!("{who}: {e}")));
            return ExitCode::from(1);
        }
    };
    let text = if args.json {
        let answer = json!({ "edit": saved.ledger_row, "budget": saved.rows });
        match serde_json::to_string_pretty(&answer) {
            Ok(text) => format!("{text}\n"),
            Err(e) => {
                let _ = writeln!(err, "switchboard {command}: {e}");
                return ExitCode::from(1);
            }
        }
    } else {
        format!(
            "{}\n",
            terminal_safe(&format!(
                "{command} {}: {}",
                edit.pool,
                budget::edit_summary(&saved.ledger_row)
            ))
        )
    };
    emit_to(out, &text)
}

/// CORE-5 (#373): `switchboard settings`: the settings file in the app config folder
/// (`config_dir`), as the Settings view shows it: one `<name>: <path>` line per setting
/// (`not set` when unset), or `--json` as the app's `settings_get` answers
/// (`{"settings": .., "missing": [..]}`). A file that does not read is exit 1.
pub fn settings_command(json: bool, out: &mut dyn Write) -> ExitCode {
    let Some(config) = config_dir() else {
        eprintln!("switchboard: set {CONFIG_ENV} to the app config folder");
        return ExitCode::from(2);
    };
    let settings = match settings::load(&config.join("settings.json")) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("switchboard settings: {e}");
            return ExitCode::from(1);
        }
    };
    let text = if json {
        let view = json!({ "settings": &settings, "missing": settings.missing() });
        match serde_json::to_string_pretty(&view) {
            Ok(text) => format!("{text}\n"),
            Err(e) => {
                eprintln!("switchboard settings: {e}");
                return ExitCode::from(1);
            }
        }
    } else {
        settings_text(&settings)
    };
    emit_to(out, &text)
}

/// One line per setting, in `settings::SETTING_NAMES` order.
fn settings_text(settings: &Settings) -> String {
    let values = [
        &settings.pipeline_repo,
        &settings.nextcloud_backups,
        &settings.vault_agents,
    ];
    settings::SETTING_NAMES
        .iter()
        .zip(values)
        .map(|(name, value)| {
            let shown = value
                .as_deref()
                .filter(|path| !path.as_os_str().is_empty())
                .map_or("not set".to_string(), |path| {
                    terminal_safe(&path.display().to_string())
                });
            format!("{name}: {shown}\n")
        })
        .collect()
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

/// The app config folder (`settings.json`): `SWITCHBOARD_CONFIG`, else `SWITCHBOARD_DATA` (the
/// earlier workaround), else the desktop app's folder for this OS.
pub(crate) fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        current_os(),
        env_path(CONFIG_ENV),
        env_path(DATA_ENV),
        env_path("HOME"),
        env_path("XDG_CONFIG_HOME"),
        env_path("APPDATA"),
    )
}

/// `config_dir` over the values it reads, so the tests need not touch the process environment.
fn config_dir_from(
    os: Os,
    config_env: Option<PathBuf>,
    data_env: Option<PathBuf>,
    home: Option<PathBuf>,
    xdg_config: Option<PathBuf>,
    appdata: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(dir) = config_env.or(data_env) {
        return Some(dir);
    }
    match os {
        Os::Windows => appdata.map(|d| d.join(APP_DIR)),
        Os::Mac => home.map(|h| h.join("Library").join("Application Support").join(APP_DIR)),
        Os::Linux => xdg_config
            .or_else(|| home.map(|h| h.join(".config")))
            .map(|d| d.join(APP_DIR)),
    }
}

/// The pipeline checkout (#359): `SWITCHBOARD_PIPELINE`, else `pipeline_repo` in the app's
/// `settings.json` (CORE-5, in the app config folder), else `None`. A settings file that cannot
/// be read counts as naming none.
pub(crate) fn pipeline_dir() -> Option<PathBuf> {
    pipeline_setting().ok().flatten()
}

/// `pipeline_dir` with the reason a settings file could not be read.
fn pipeline_setting() -> Result<Option<PathBuf>, String> {
    if let Some(dir) = env_path(PIPELINE_ENV) {
        return Ok(Some(dir));
    }
    let Some(config) = config_dir() else {
        return Ok(None);
    };
    settings::load(&config.join("settings.json"))
        .map(|settings| settings.pipeline_repo().ok().map(Path::to_path_buf))
        .map_err(|e| e.to_string())
}

/// A read answer that failed: the line the command prints on stderr and its exit code. The MCP
/// read tools answer with the message.
pub(crate) struct Fail {
    code: u8,
    pub(crate) message: String,
}

impl From<serde_json::Error> for Fail {
    fn from(e: serde_json::Error) -> Self {
        fail(1, format!("switchboard: {e}"))
    }
}

fn fail(code: u8, message: String) -> Fail {
    Fail { code, message }
}

/// Say why a read failed on stderr and give its exit code.
fn exit_with(fail: Fail) -> ExitCode {
    eprintln!("{}", fail.message);
    ExitCode::from(fail.code)
}

/// The pipeline checkout, or the failure saying it is not set.
fn pipeline_or_fail() -> Result<PathBuf, Fail> {
    match pipeline_setting() {
        Ok(Some(root)) => Ok(root),
        Ok(None) => Err(fail(
            2,
            format!(
                "switchboard: set {PIPELINE_ENV} to your pipeline checkout \
                 (or pipeline_repo in settings.json)"
            ),
        )),
        Err(e) => Err(fail(1, format!("switchboard: {e}"))),
    }
}

/// The pipeline checkout, or the exit code after saying it is not set.
fn pipeline_root() -> Result<PathBuf, ExitCode> {
    pipeline_or_fail().map_err(exit_with)
}

/// The app data folder, or the failure saying it is not set.
fn data_or_fail() -> Result<PathBuf, Fail> {
    data_dir().ok_or_else(|| {
        fail(
            2,
            format!("switchboard: set {DATA_ENV} to the app data folder"),
        )
    })
}

/// `switchboard budget`: one line per pool (text) or the rows as a JSON array (`--json`).
/// `--refresh` (BUD-3, #374) re-reads every pool that has a reader first, as the app's timer
/// does (`budget::refresh`).
fn budget(json: bool, refresh: bool) -> ExitCode {
    let rows = if refresh {
        budget_refreshed_rows()
    } else {
        budget_rows()
    };
    let rows = match rows {
        Ok(rows) => rows,
        Err(fail) => return exit_with(fail),
    };
    if json {
        print_json("budget", serde_json::to_string_pretty(&rows))
    } else {
        emit(&budget_text(&rows))
    }
}

/// MCP-1: the pool rows `budget` prints.
pub(crate) fn budget_rows() -> Result<Vec<PoolRow>, Fail> {
    let root = pipeline_or_fail()?;
    budget::read(&FleetBridge::new(&root)).map_err(|e| fail(1, format!("switchboard budget: {e}")))
}

/// BUD-3 (#374): the pool rows after `fleet budget --refresh`.
fn budget_refreshed_rows() -> Result<Vec<PoolRow>, Fail> {
    let root = pipeline_or_fail()?;
    budget::refresh(&FleetBridge::new(&root))
        .map_err(|e| fail(1, format!("switchboard budget: {e}")))
}

/// One line per pool, in row order, with what the budget panel shows (BUD-1, #361):
/// `<pool>: <pct>% vs pace <pace>%, <+d.d> pts, <verdict>, <age> min old`, and `, stale` when
/// fleet calls the reading stale.
fn budget_text(rows: &[PoolRow]) -> String {
    let percent = |value: Option<f64>| match value {
        Some(v) => format!("{v}%"),
        None => NOT_RECORDED.to_string(),
    };
    let points = |value: Option<f64>| match value {
        Some(v) if v >= 0.0 => format!("+{v:.1} pts"),
        Some(v) => format!("{v:.1} pts"),
        None => format!("pts {NOT_RECORDED}"),
    };
    rows.iter()
        .map(|row| {
            let age = match row.reading_age_min {
                Some(min) => format!("{min} min old"),
                None => format!("age {NOT_RECORDED}"),
            };
            format!(
                "{}: {} vs pace {}, {}, {}, {age}{}\n",
                terminal_safe(&row.pool),
                percent(row.pct),
                percent(row.pace),
                points(row.delta),
                terminal_safe(&row.verdict),
                if row.stale { ", stale" } else { "" }
            )
        })
        .collect()
}

/// Every stored snapshot in the data folder, or the failure saying why there is none.
fn snapshots(command: &str, data: &Path) -> Result<Vec<RepoSnapshot>, Fail> {
    match SnapshotStore::new(data).load_each() {
        Ok((snaps, bad)) if snaps.is_empty() => {
            warn_unreadable(command, &bad);
            Err(fail(1, NO_SNAPSHOT.to_string()))
        }
        Ok((snaps, bad)) => {
            warn_unreadable(command, &bad);
            Ok(snaps)
        }
        Err(e) => Err(fail(1, format!("switchboard {command}: {e}"))),
    }
}

/// GH-1 (#350): name each snapshot file that does not read on stderr; the views show the rest.
fn warn_unreadable(command: &str, bad: &[switchboard_core::github::snapshot::Unreadable]) {
    for file in bad {
        eprintln!(
            "switchboard {command}: {}; skipped, run switchboard refresh",
            terminal_safe(&file.error)
        );
    }
}

/// #375 (CORE-2, IT-2): what `errors` prints and the MCP `errors` tool returns: the fleet
/// errors the app recorded (`fleet_errors::load`, the list the app shows), newest first; how
/// many lines of the file did not read; and the open IT tickets in the stored snapshots.
#[derive(Debug)]
pub(crate) struct ErrorsAnswer {
    pub fleet_errors: Vec<FleetErrorRecord>,
    pub skipped_lines: usize,
    pub it_tickets: Vec<it::ItTicket>,
}

/// #375: the errors and tickets in `data`. A snapshot file that does not read is named on
/// stderr and the rest are used; no snapshot is no tickets, not a failure. Damaged lines of the
/// errors file are counted in `skipped_lines` and named on stderr.
pub(crate) fn errors_answer(data: &Path) -> Result<ErrorsAnswer, Fail> {
    let recorded =
        fleet_errors::load(data).map_err(|e| fail(1, format!("switchboard errors: {e}")))?;
    if recorded.skipped > 0 {
        eprintln!(
            "switchboard errors: {} line(s) of {} did not read; skipped",
            recorded.skipped,
            fleet_errors::FLEET_ERRORS_FILE
        );
    }
    let (snaps, bad) = SnapshotStore::new(data)
        .load_each()
        .map_err(|e| fail(1, format!("switchboard errors: {e}")))?;
    warn_unreadable("errors", &bad);
    Ok(ErrorsAnswer {
        fleet_errors: recorded.errors.iter().map(FleetErrorRecord::from).collect(),
        skipped_lines: recorded.skipped,
        it_tickets: it::open_tickets(&snaps),
    })
}

impl ErrorsAnswer {
    /// `{"fleet_errors": [..], "skipped_lines": n, "it_tickets": [..]}`.
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "fleet_errors": self.fleet_errors,
            "skipped_lines": self.skipped_lines,
            "it_tickets": self.it_tickets,
        })
    }
}

/// `switchboard errors`: the recorded fleet errors, newest first, then the open IT tickets
/// (text), or both as one JSON object (`--json`). Read only.
fn errors_command(json: bool) -> ExitCode {
    let answer = match data_or_fail().and_then(|data| errors_answer(&data)) {
        Ok(answer) => answer,
        Err(fail) => return exit_with(fail),
    };
    if json {
        return print_json("errors", serde_json::to_string_pretty(&answer.to_json()));
    }
    emit(&errors_text(&answer))
}

/// One line per fleet error as the app's banner words it, then one per open IT ticket.
fn errors_text(answer: &ErrorsAnswer) -> String {
    let mut text = String::new();
    if answer.fleet_errors.is_empty() {
        text.push_str("no fleet errors recorded\n");
    } else {
        text.push_str("fleet errors, newest first:\n");
        for err in &answer.fleet_errors {
            text.push_str(&format!("  {}\n", terminal_safe(&err.message)));
        }
    }
    if answer.it_tickets.is_empty() {
        text.push_str("no open IT tickets\n");
    } else {
        text.push_str("open IT tickets:\n");
        for t in &answer.it_tickets {
            text.push_str(&format!(
                "  {}#{}: {} | {}\n",
                terminal_safe(&t.repo),
                t.number,
                terminal_safe(&t.title),
                terminal_safe(&t.link)
            ));
        }
    }
    text
}

/// `switchboard waiting`: the Waiting on you items from the stored snapshots and the local items
/// (WAIT-1), newest first: one line each (text) or the items as a JSON array (`--json`). CalDAV
/// is not read yet (U7).
fn waiting(json: bool) -> ExitCode {
    let items = match data_or_fail().and_then(|data| waiting_items(&data)) {
        Ok(items) => items,
        Err(fail) => return exit_with(fail),
    };
    if json {
        return print_json("waiting", serde_json::to_string_pretty(&items));
    }
    emit(&waiting_text(&items))
}

/// MCP-1: the items `waiting` prints, newest first.
pub(crate) fn waiting_items(data: &Path) -> Result<Vec<Item>, Fail> {
    let (snaps, bad) = SnapshotStore::new(data)
        .load_each()
        .map_err(|e| fail(1, format!("switchboard waiting: {e}")))?;
    warn_unreadable("waiting", &bad);
    let local = LocalItems::new(data)
        .load()
        .map_err(|e| fail(1, format!("switchboard waiting: {e}")))?;
    if snaps.is_empty() && local.is_empty() {
        return Err(fail(1, NO_SNAPSHOT.to_string()));
    }
    let priority = waiting::priority_items(data, SystemClock.now());
    Ok(waiting::merge(vec![
        waiting::assemble(&snaps, &local),
        priority,
    ]))
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
                terminal_safe(&item.what),
                terminal_safe(&item.why),
                terminal_safe(&item.action),
                terminal_safe(&item.link)
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
    let state = std::env::var_os(reports::FLEET_STATE_DIR_ENV);
    let shown = match load_reports(&root, state.as_deref(), data_dir().as_deref(), filter) {
        Ok(shown) => shown,
        Err(e) => {
            eprintln!("switchboard reports: {e}");
            return ExitCode::from(1);
        }
    };
    if json {
        return print_json("reports", serde_json::to_string_pretty(&shown));
    }
    let refs: Vec<&Report> = shown.iter().collect();
    emit(&reports_text(&refs))
}

/// The reports on disk for the pipeline checkout at `root`, newest first, narrowed by `filter`.
/// `state` is the `FLEET_STATE_DIR` value; when it is not set the fleet's own state folder is used.
fn load_reports(
    root: &Path,
    state: Option<&std::ffi::OsStr>,
    data: Option<&Path>,
    filter: &Filter,
) -> Result<Vec<Report>, CoreError> {
    let threads = data.map(reports::thread_rows).unwrap_or_default();
    reports::load(&FleetBridge::new(root), state, &threads, Some(filter))
}

/// Each report in list order with the five parts the Reports view shows (REP-1, #361): the head
/// line `<title> | <outcome> | <project>` (outcome as `<outcome> (<failure mode>)` when there is
/// one), then indented lines for what changed, what was checked, what wasn't and cost. An empty
/// list says `nothing`. No reports gives the no-reports line.
fn reports_text(reports: &[&reports::Report]) -> String {
    if reports.is_empty() {
        return format!("{NO_REPORTS}\n");
    }
    reports.iter().map(|report| report_text(report)).collect()
}

/// One report as `reports_text` prints it.
fn report_text(report: &Report) -> String {
    let or_not = |value: Option<String>, what: &str| {
        value.unwrap_or_else(|| format!("{what} {NOT_RECORDED}"))
    };
    let outcome = match &report.failure_mode {
        Some(mode) => format!("{} ({mode})", report.outcome),
        None => report.outcome.clone(),
    };
    let mut text = format!(
        "{} | {} | {}\n",
        terminal_safe(&joblog::shown(&report.title)),
        terminal_safe(&outcome),
        terminal_safe(&joblog::shown(&report.project))
    );
    let changed = &report.changed;
    text.push_str(&format!(
        "  changed: {}, {}, {}\n",
        changed
            .pr
            .as_deref()
            .map_or("no PR".to_string(), terminal_safe),
        or_not(changed.files.map(|n| format!("files {n}")), "files"),
        or_not(changed.lines.map(|n| format!("lines {n}")), "lines"),
    ));
    for (part, lines) in [
        ("checked", &report.checked),
        ("not checked", &report.not_checked),
    ] {
        if lines.is_empty() {
            text.push_str(&format!("  {part}: nothing\n"));
        } else {
            text.push_str(&format!("  {part}:\n"));
            for line in lines {
                text.push_str(&format!("    {}\n", terminal_safe(line)));
            }
        }
    }
    let cost = &report.cost;
    text.push_str(&format!(
        "  cost: {}, {}, {}, {}\n",
        or_not(cost.pool.as_deref().map(terminal_safe), "pool"),
        or_not(cost.tokens.map(|n| format!("{n} tokens")), "tokens"),
        or_not(cost.usd.map(|usd| format!("${usd:.2}")), "cost"),
        or_not(cost.seconds.map(duration), "time"),
    ));
    text
}

/// Seconds, rounded to the second, as the Reports view writes them: `45s`, `15m 31s`, `1h 2m`.
fn duration(seconds: f64) -> String {
    let total = seconds.round().max(0.0) as u64;
    match total {
        0..=59 => format!("{total}s"),
        60..=3599 => format!("{}m {}s", total / 60, total % 60),
        _ => format!("{}h {}m", total / 3600, (total % 3600) / 60),
    }
}

/// Print the JSON on stdout; a failure is named by `command` on stderr, exit 1.
fn print_json(command: &str, json: serde_json::Result<String>) -> ExitCode {
    match json {
        Ok(text) => emit(&format!("{text}\n")),
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
    /// The priority copy's age in days, the same on every row (SKY-9, #345).
    priority_age_days: Option<i64>,
}

/// `switchboard sky`: one row per repo in the Sky view's placement order, from the same
/// `sky::model` the window paints.
fn sky_command(json: bool) -> ExitCode {
    let rows = match data_or_fail().and_then(|data| sky_rows(&data)) {
        Ok(rows) => rows,
        Err(fail) => return exit_with(fail),
    };
    if json {
        print_json("sky", serde_json::to_string_pretty(&sky_json(&rows)))
    } else {
        emit(&sky_text(&rows))
    }
}

/// The sky model and the snapshots it was built from.
fn sky_model(data: &Path) -> Result<(sky::model::SkyModel, Vec<RepoSnapshot>), Fail> {
    let snaps = snapshots("sky", data)?;
    let events = ledger_events().map_err(|e| fail(1, format!("switchboard sky: {e}")))?;
    let model = sky::model::load_with_ledger(data, &events.unwrap_or_default(), SystemClock.now())
        .map_err(|e| fail(1, format!("switchboard sky: {e}")))?;
    Ok((model, snaps))
}

/// MCP-1: the rows `sky` prints.
fn sky_rows(data: &Path) -> Result<Vec<SkyRow>, Fail> {
    let (model, snaps) = sky_model(data)?;
    Ok(model
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
            priority_age_days: model.priority_age_days,
        })
        .collect())
}

/// The rows as `sky --json` prints them.
fn sky_json(rows: &[SkyRow]) -> Value {
    Value::Array(
        rows.iter()
            .map(|row| {
                json!({
                    "repo": row.repo,
                    "brightness": row.brightness,
                    "open": row.open,
                    "drifted": row.drifted,
                    "priority_age_days": row.priority_age_days,
                })
            })
            .collect(),
    )
}

/// MCP-1: the sky rows as `sky --json` prints them.
pub(crate) fn sky_answer(data: &Path) -> Result<Value, Fail> {
    sky_rows(data).map(|rows| sky_json(&rows))
}

/// MCP-1: one repo's constellation from the sky model, as the window paints it. `<repo>` is
/// `owner/name` or the bare name.
pub(crate) fn constellation_answer(data: &Path, repo: &str) -> Result<Value, Fail> {
    let (model, _) = sky_model(data)?;
    let full = resolve_repo(repo, model.constellations.iter().map(|c| c.repo.as_str()))
        .map_err(|e| fail(1, format!("switchboard: {e}")))?;
    let found = full.and_then(|full| model.constellations.iter().find(|c| c.repo == full));
    match found {
        Some(c) => {
            serde_json::to_value(c).map_err(|e| fail(1, format!("switchboard constellation: {e}")))
        }
        None => Err(fail(1, format!("switchboard: no such repo: {repo}"))),
    }
}

/// One line per repo: `<repo>: brightness <b>, <n> open, <d> drifted`, then the priority
/// copy's age (`sky::priority::age_line`).
fn sky_text(rows: &[SkyRow]) -> String {
    let mut text: String = rows
        .iter()
        .map(|row| {
            format!(
                "{}: brightness {:.2}, {} open, {} drifted\n",
                terminal_safe(&row.repo),
                row.brightness,
                row.open,
                row.drifted
            )
        })
        .collect();
    text.push_str(&sky::priority::age_line(
        rows.first().and_then(|r| r.priority_age_days),
    ));
    text
}

/// The PR that closes a star, as `star` prints it.
struct StarPr {
    number: u64,
    state: String,
    /// The last R25 verdict, `PASS` or `FAIL`, marked when not on the head (`review_text`).
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
    let star = match data_or_fail().and_then(|data| star_facts(&data, target)) {
        Ok(star) => star,
        Err(fail) => return exit_with(fail),
    };
    if json {
        print_json("star", serde_json::to_string_pretty(&star_json(&star)))
    } else {
        emit(&star_text(&star))
    }
}

/// MCP-1: the facts `star` prints for `<repo>#<n>`.
fn star_facts(data: &Path, target: &str) -> Result<Star, Fail> {
    let Some((repo, number)) = target
        .rsplit_once('#')
        .and_then(|(repo, n)| Some((repo, n.parse::<u64>().ok()?)))
        .filter(|(repo, _)| !repo.is_empty())
    else {
        return Err(fail(
            2,
            format!("switchboard: star needs <repo>#<n>, got {target}"),
        ));
    };
    let snaps = snapshots("star", data)?;
    let full = resolve_repo(repo, snaps.iter().map(|s| s.repo.as_str()))
        .map_err(|e| fail(1, format!("switchboard: {e}")))?;
    let snap = full.and_then(|full| snaps.iter().find(|s| s.repo == full));
    let Some((snap, issue)) =
        snap.and_then(|s| Some((s, s.issues.iter().find(|i| i.number == number)?)))
    else {
        return Err(fail(1, format!("switchboard: no such star: {target}")));
    };
    let job =
        running_job(&snap.repo, number).map_err(|e| fail(1, format!("switchboard star: {e}")))?;
    Ok(Star {
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
                review: gh_merge::review_text(pr),
                ci: pr.ci_on_head.clone(),
            }),
        job,
    })
}

/// The star as `star --json` prints it.
fn star_json(star: &Star) -> Value {
    let job = match &star.job {
        Some(job) => json!(job),
        None => json!(NOT_RECORDED),
    };
    json!({
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
    })
}

/// MCP-1: the star as `star --json` prints it.
pub(crate) fn star_answer(data: &Path, target: &str) -> Result<Value, Fail> {
    star_facts(data, target).map(|star| star_json(&star))
}

/// MCP-1, REP-3 (#358): the reports as `reports --json` prints them, narrowed by `filter`, read
/// the way the CLI and the app read them (`load_reports`).
pub(crate) fn reports_answer(filter: &Filter) -> Result<Vec<Report>, Fail> {
    let root = pipeline_or_fail()?;
    let env = std::env::var_os(reports::FLEET_STATE_DIR_ENV);
    load_reports(&root, env.as_deref(), data_dir().as_deref(), filter)
        .map_err(|e| fail(1, format!("switchboard reports: {e}")))
}

/// One fact per line, the issue first.
fn star_text(star: &Star) -> String {
    let or_none = |value: Option<&str>| terminal_safe(value.unwrap_or("none"));
    let labels = if star.labels.is_empty() {
        "none".to_string()
    } else {
        terminal_safe(&star.labels.join(", "))
    };
    let (pr, review, ci) = match &star.pr {
        Some(pr) => (
            format!("#{} {}", pr.number, terminal_safe(&pr.state)),
            or_none(pr.review.as_deref()),
            terminal_safe(&pr.ci),
        ),
        None => ("none".to_string(), "none".to_string(), "none".to_string()),
    };
    let job = match &star.job {
        Some(job) => or_none(job.as_deref()),
        None => NOT_RECORDED.to_string(),
    };
    format!(
        "{}#{}: {}\nstate: {}\nlabels: {labels}\nsize: {}\npr: {pr}\nreview: {review}\nci: {ci}\njob: {job}\n",
        terminal_safe(&star.repo),
        star.number,
        terminal_safe(&star.title),
        terminal_safe(&star.state),
        or_none(star.size.as_deref()),
    )
}

/// The fleet ledger's events; `Ok(None)` when the pipeline checkout is not set
/// (`pipeline_dir`: `SWITCHBOARD_PIPELINE`, else `pipeline_repo` in settings.json, #359).
/// The fleet state folder is `SWITCHBOARD_STATE` when set, else the one `reports` and the app
/// read (`reports::state_dir`: `FLEET_STATE_DIR`, then the pipeline checkout's rule).
fn ledger_events() -> Result<Option<Vec<Value>>, String> {
    let Some(pipeline) = pipeline_dir() else {
        return Ok(None);
    };
    let state = env_path(STATE_ENV).unwrap_or_else(|| {
        let env = std::env::var_os(reports::FLEET_STATE_DIR_ENV);
        reports::state_dir(&pipeline, env.as_deref())
    });
    joblog::read_ledger(&joblog::ledger_files(&pipeline, &state))
        .map(Some)
        .map_err(|e| e.to_string())
}

/// The fleet job running on `repo#number`: the last ledger `start` event for it with no `end`
/// after it, as `<lane> <model> on <machine>, attempt <n>`. `Ok(None)` when the pipeline
/// checkout is not set, so the job log cannot be read.
fn running_job(repo: &str, number: u64) -> Result<Option<Option<String>>, String> {
    let Some(events) = ledger_events()? else {
        return Ok(None);
    };
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
const TOOLCHAINS: [&str; 4] = ["cargo", "node", "python3", "gemini"];
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
    // #334: a closed pipe is a quiet exit, like every other command.
    emit_to(out, &text)
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
    // #352: model names and fleet errors are outside text, escaped like the rest.
    let models = terminal_safe(&loadout.models_loaded.join(", "));
    rows.push(row("llama-swap", &loaded_state, models));
    let status = if loadout.busy { "busy" } else { "idle" };
    rows.push(row("fleet status", status, String::new()));
    let errors = loadout.recent_errors.len();
    let errors_state = if errors > 0 {
        format!("{errors} recent")
    } else {
        "none".to_string()
    };
    let error_list = terminal_safe(&loadout.recent_errors.join("; "));
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
mod load_reports_tests {
    use super::*;

    #[test]
    fn reports_load_reads_the_done_cards_from_the_state_env_folder() {
        // The test name is unique per copy of this module (the integration tests compile it twice).
        let tag: String = std::thread::current()
            .name()
            .unwrap_or("test")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let base = std::env::temp_dir().join(format!("sb-reports-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let pipeline = base.join("pipeline");
        let state = base.join("state");
        let done = state.join("jobs").join("done");
        std::fs::create_dir_all(&pipeline).unwrap();
        std::fs::create_dir_all(&done).unwrap();
        std::fs::write(
            done.join("card-1.json"),
            r#"{"id":"card-1","title":"Fix the sync retry","repo":"owner/app-a","outcome":"passed"}"#,
        )
        .unwrap();

        let found =
            load_reports(&pipeline, Some(state.as_os_str()), None, &Filter::default()).unwrap();
        let _ = std::fs::remove_dir_all(&base);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "card-1");
    }
}

#[cfg(test)]
mod config_dir_tests {
    use super::*;

    fn base() -> PathBuf {
        PathBuf::from("base")
    }

    #[test]
    fn linux_uses_xdg_config_home() {
        let xdg = base().join("xdg");
        let got = config_dir_from(Os::Linux, None, None, Some(base()), Some(xdg.clone()), None);
        assert_eq!(got, Some(xdg.join(APP_DIR)));
    }

    #[test]
    fn linux_falls_back_to_home_dot_config() {
        let got = config_dir_from(Os::Linux, None, None, Some(base()), None, None);
        assert_eq!(got, Some(base().join(".config").join(APP_DIR)));
    }

    #[test]
    fn config_env_wins_over_data_env() {
        let config = base().join("cfg");
        let data = base().join("data");
        let got = config_dir_from(
            Os::Linux,
            Some(config.clone()),
            Some(data),
            Some(base()),
            None,
            None,
        );
        assert_eq!(got, Some(config));
    }

    #[test]
    fn data_env_is_the_fallback() {
        let data = base().join("data");
        let got = config_dir_from(
            Os::Linux,
            None,
            Some(data.clone()),
            Some(base()),
            None,
            None,
        );
        assert_eq!(got, Some(data));
    }

    #[test]
    fn mac_uses_application_support() {
        let got = config_dir_from(Os::Mac, None, None, Some(base()), None, None);
        assert_eq!(
            got,
            Some(
                base()
                    .join("Library")
                    .join("Application Support")
                    .join(APP_DIR)
            )
        );
    }

    #[test]
    fn windows_uses_appdata() {
        let appdata = base().join("appdata");
        let got = config_dir_from(Os::Windows, None, None, None, None, Some(appdata.clone()));
        assert_eq!(got, Some(appdata.join(APP_DIR)));
    }
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
