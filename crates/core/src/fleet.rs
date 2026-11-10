//! CORE-1, CORE-2: the fleet bridge. The core wraps the pipeline repo's Python `fleet` tools
//! rather than porting them: it runs `<python> <root>/tools/fleet/fleet.py <cmd> ... --json`
//! and parses the result. Only commands with `--json`; one that lacks it gets `--json` added
//! in a pipeline repo change, never text parsing.

use crate::platform::{current_os, python_launcher};
use serde_json::Value;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// CORE-1: how long one fleet call may run before the core kills it and records a fleet error
/// (`FleetErrorReason::Timeout`). `fleet.py` calls are local reads (budget, ledger, status); a
/// refresh that reads a provider is the slowest, and a call past two minutes is stuck (a held
/// lock, a dead network). `FleetBridge::with_timeout` changes it for one bridge.
pub const FLEET_TIMEOUT_SECS: u64 = 120;

/// The environment variable that points the bridge at another root (tests: a stub `fleet.py`
/// under `tests/fixtures/fleet-stub/`).
pub const FLEET_ROOT_ENV: &str = "FLEET_ROOT";

/// The `LD_LIBRARY_PATH` a fleet child should get: `ld_library_path` without every entry that
/// starts with `appdir`. `None` means remove the variable (nothing is left); an empty `appdir`
/// leaves the value as it is.
pub fn clean_ld_library_path(ld_library_path: &OsStr, appdir: &OsStr) -> Option<OsString> {
    if appdir.is_empty() {
        return Some(ld_library_path.to_os_string());
    }
    let appdir = Path::new(appdir);
    let kept: Vec<PathBuf> = std::env::split_paths(ld_library_path)
        .filter(|entry| !entry.starts_with(appdir))
        .collect();
    if kept.is_empty() {
        return None;
    }
    std::env::join_paths(kept).ok()
}

/// Gives `cmd` a clean environment for a fleet child: the packaged app's `PYTHONHOME`,
/// `PYTHONPATH` and `PYTHONDONTWRITEBYTECODE` removed, and, when `APPDIR` is set and not empty,
/// the bundle's entries taken out of `LD_LIBRARY_PATH` (the variable goes when none is left).
pub fn clean_child_env(cmd: &mut Command) {
    for key in ["PYTHONHOME", "PYTHONPATH", "PYTHONDONTWRITEBYTECODE"] {
        cmd.env_remove(key);
    }
    let appdir = std::env::var_os("APPDIR").filter(|v| !v.is_empty());
    if let (Some(appdir), Some(ld)) = (appdir, std::env::var_os("LD_LIBRARY_PATH")) {
        match clean_ld_library_path(&ld, &appdir) {
            Some(value) => cmd.env("LD_LIBRARY_PATH", value),
            None => cmd.env_remove("LD_LIBRARY_PATH"),
        };
    }
}

/// CORE-2: a fleet command that failed or printed JSON the core can't parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetError {
    /// The full command line, program first.
    pub command: Vec<String>,
    /// The exit code; `None` when the process could not start or was killed.
    pub exit_code: Option<i32>,
    /// Its stderr (all of it; the face shows the first lines).
    pub stderr: String,
    pub reason: FleetErrorReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FleetErrorReason {
    /// The process exited non-zero.
    Exit,
    /// The process exited 0 but stdout was not JSON. Holds the parser's message.
    BadJson(String),
    /// The process could not be started. Holds the OS error text.
    Spawn(String),
    /// The process ran past the time limit and was killed. Holds the limit in seconds.
    Timeout(u64),
}

impl std::fmt::Display for FleetError {
    /// CORE-2: the command, then what went wrong: the exit code and the first lines of stderr,
    /// the JSON parser's message, the spawn error or the time limit.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let command = self.command.join(" ");
        match &self.reason {
            FleetErrorReason::Exit => {
                match self.exit_code {
                    Some(code) => write!(f, "{command} exited {code}")?,
                    None => write!(f, "{command} was killed")?,
                }
                let lines: Vec<&str> = self
                    .stderr
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .take(5)
                    .collect();
                if !lines.is_empty() {
                    write!(f, ": {}", lines.join(" | "))?;
                }
                Ok(())
            }
            FleetErrorReason::BadJson(why) => {
                write!(f, "{command} printed output that is not JSON ({why})")
            }
            FleetErrorReason::Spawn(why) => write!(f, "{command} could not start: {why}"),
            FleetErrorReason::Timeout(secs) => {
                write!(f, "{command} ran past {secs} s and was stopped")
            }
        }
    }
}

/// CORE-1: the fleet bridge: the root it runs against, the launcher and the extra env for
/// the child.
#[derive(Debug, Clone)]
pub struct FleetBridge {
    root: PathBuf,
    /// The launcher (`with_python`); `None`: the platform's Python launcher.
    python: Option<(String, Vec<String>)>,
    /// Extra env for the child, in the order added.
    env: Vec<(String, String)>,
    /// The longest one call may run (`FLEET_TIMEOUT_SECS`).
    timeout: Duration,
}

impl FleetBridge {
    /// A bridge over the pipeline repo at `pipeline_root`, unless `FLEET_ROOT` is set, which
    /// wins.
    pub fn new(pipeline_root: &Path) -> Self {
        let fleet_root = std::env::var_os(FLEET_ROOT_ENV);
        Self::with_root(&Self::resolve_root(pipeline_root, fleet_root.as_deref()))
    }

    /// The root the bridge runs against: a non-empty `fleet_root_env` wins, else
    /// `pipeline_root`.
    pub fn resolve_root(pipeline_root: &Path, fleet_root_env: Option<&OsStr>) -> PathBuf {
        match fleet_root_env {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            _ => pipeline_root.to_path_buf(),
        }
    }

    /// A bridge over exactly `root`, whatever `FLEET_ROOT` holds.
    pub fn with_root(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            python: None,
            env: Vec::new(),
            timeout: Duration::from_secs(FLEET_TIMEOUT_SECS),
        }
    }

    /// Let one call run `timeout` at most (default `FLEET_TIMEOUT_SECS`).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Use `program` with its `args` as the launcher instead of the platform's Python.
    pub fn with_python(mut self, program: &str, args: &[&str]) -> Self {
        self.python = Some((
            program.to_string(),
            args.iter().map(|a| a.to_string()).collect(),
        ));
        self
    }

    /// Add `key=value` to the child's env, after `FLEET_ROOT`, in the order added.
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_string(), value.to_string()));
        self
    }

    /// The root this bridge runs against.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/tools/fleet/fleet.py`.
    pub fn script(&self) -> PathBuf {
        self.root.join("tools").join("fleet").join("fleet.py")
    }

    /// The full command line: the launcher program, its args, the script, `args`, then
    /// `--json`.
    pub fn command_line(&self, args: &[&str]) -> Vec<String> {
        let mut command = Vec::with_capacity(3 + args.len());
        match &self.python {
            Some((program, lead)) => {
                command.push(program.clone());
                command.extend(lead.iter().cloned());
            }
            None => {
                let (program, lead) = python_launcher(current_os());
                command.push(program.to_string());
                command.extend(lead.iter().map(|a| a.to_string()));
            }
        }
        command.push(self.script().to_string_lossy().into_owned());
        command.extend(args.iter().map(|a| a.to_string()));
        command.push("--json".to_string());
        command
    }

    /// The child for `command` (program first): `FLEET_ROOT` set to the root, then the
    /// `with_env` pairs, stdout and stderr captured. Stdin is null, or `input` when given (written
    /// from a thread while the output is read, so a large input cannot deadlock on a full pipe).
    /// CORE-1: a child still running after the bridge's timeout is killed (its whole process
    /// group on Unix); the flag is true then and the output is what it printed until then.
    fn spawn(
        &self,
        command: &[String],
        input: Option<&[u8]>,
    ) -> std::io::Result<(std::process::Output, bool)> {
        let mut child = Command::new(&command[0]);
        child.args(&command[1..]);
        clean_child_env(&mut child);
        child.env(FLEET_ROOT_ENV, self.root.as_os_str());
        for (key, value) in &self.env {
            child.env(key, value);
        }
        child.stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        child.stdout(Stdio::piped());
        child.stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Its own group, so a timeout can stop the launcher's children too.
            child.process_group(0);
        }
        let mut proc = child.spawn()?;
        let writer = match (input, proc.stdin.take()) {
            (Some(bytes), Some(mut stdin)) => {
                let bytes = bytes.to_vec();
                // A child that exits without reading gives a broken pipe: its exit code says why.
                Some(std::thread::spawn(move || {
                    let _ = stdin.write_all(&bytes);
                }))
            }
            _ => None,
        };
        let readers = [
            read_in_thread(proc.stdout.take()),
            read_in_thread(proc.stderr.take()),
        ];

        let deadline = Instant::now() + self.timeout;
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = proc.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                timed_out = true;
                kill_tree(&mut proc);
                break proc.wait()?;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        // A grandchild that outlives the child (or left the group after a kill) can keep a pipe
        // open: wait a moment past the limit, then take what was read rather than hang.
        let grace = if timed_out {
            Duration::from_secs(2)
        } else {
            deadline.saturating_duration_since(Instant::now()) + Duration::from_secs(2)
        };
        let [stdout, stderr] = readers.map(|rx| rx.recv_timeout(grace).unwrap_or_default());
        if let Some(writer) = writer {
            if !timed_out {
                let _ = writer.join();
            }
        }
        Ok((
            std::process::Output {
                status,
                stdout,
                stderr,
            },
            timed_out,
        ))
    }

    /// Run the command line for `args` and wait. A failed spawn is a `FleetError` with
    /// reason `Spawn`; otherwise the command line, the exit code (`None` when the process
    /// was killed), stdout and stderr, each lossy UTF-8.
    fn run(
        &self,
        args: &[&str],
        input: Option<&[u8]>,
    ) -> Result<(Vec<String>, Option<i32>, String, String), FleetError> {
        let command = self.command_line(args);
        let (output, timed_out) = match self.spawn(&command, input) {
            Ok(done) => done,
            Err(err) => {
                return Err(FleetError {
                    command,
                    exit_code: None,
                    stderr: String::new(),
                    reason: FleetErrorReason::Spawn(err.to_string()),
                })
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if timed_out {
            return Err(FleetError {
                command,
                exit_code: None,
                stderr,
                reason: FleetErrorReason::Timeout(self.timeout.as_secs()),
            });
        }
        Ok((command, output.status.code(), stdout, stderr))
    }

    /// Run `fleet.py <args...> --json` with the platform's Python launcher and parse
    /// stdout. A failed spawn, a non-zero exit or stdout that is not JSON is a
    /// `FleetError`, never a guessed value.
    pub fn run_json(&self, args: &[&str]) -> Result<Value, FleetError> {
        let (command, code, stdout, stderr) = self.run(args, None)?;
        match code {
            Some(0) => serde_json::from_str(&stdout).map_err(|err| FleetError {
                command,
                exit_code: Some(0),
                stderr,
                reason: FleetErrorReason::BadJson(err.to_string()),
            }),
            code => Err(FleetError {
                command,
                exit_code: code,
                stderr,
                reason: FleetErrorReason::Exit,
            }),
        }
    }

    /// Run a fleet command whose answer is its exit code (`budget allow <lane>`: 0 go,
    /// 75 not now). Any other code is a `FleetError`.
    pub fn run_status(&self, args: &[&str]) -> Result<i32, FleetError> {
        Self::status(self.run(args, None)?)
    }

    /// `run_status` with `input` on the child's stdin (IT-2: `errors file --records -` reads
    /// the records there, so nothing is written to a temp file).
    pub fn run_status_with_input(&self, args: &[&str], input: &str) -> Result<i32, FleetError> {
        Self::status(self.run(args, Some(input.as_bytes()))?)
    }

    /// 0 and 75 are answers; any other code is a `FleetError`.
    fn status(
        (command, code, _stdout, stderr): (Vec<String>, Option<i32>, String, String),
    ) -> Result<i32, FleetError> {
        match code {
            Some(code) if code == 0 || code == 75 => Ok(code),
            code => Err(FleetError {
                command,
                exit_code: code,
                stderr,
                reason: FleetErrorReason::Exit,
            }),
        }
    }
}

/// Read `pipe` to its end on a thread; the bytes arrive on the channel.
fn read_in_thread<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

/// Kill `proc` and, on Unix, the process group it leads (`kill` is the same tool a shell uses; no
/// extra dependency).
fn kill_tree(proc: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-s", "KILL", "--", &format!("-{}", proc.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = proc.kill();
}

/// CORE-2: how many fleet errors `FleetErrorLog` keeps.
pub const FLEET_ERROR_LOG_CAP: usize = 50;

/// CORE-2: every fleet error the core has recorded this session, newest last, for the view
/// and the IT region (IT-2).
#[derive(Debug, Default)]
pub struct FleetErrorLog {
    pub errors: Vec<FleetError>,
}

impl FleetErrorLog {
    /// Record `err` as it came (all of stderr, no dedupe), at the end. Once the log is
    /// over `FLEET_ERROR_LOG_CAP`, drop from the front until it is 50.
    pub fn record(&mut self, err: FleetError) {
        self.errors.push(err);
        if self.errors.len() > FLEET_ERROR_LOG_CAP {
            let excess = self.errors.len() - FLEET_ERROR_LOG_CAP;
            self.errors.drain(..excess);
        }
    }

    /// The last error recorded, if any.
    pub fn latest(&self) -> Option<&FleetError> {
        self.errors.last()
    }
}
