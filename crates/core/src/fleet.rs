//! CORE-1, CORE-2: the fleet bridge. The core wraps the pipeline repo's Python `fleet` tools
//! rather than porting them: it runs `<python> <root>/tools/fleet/fleet.py <cmd> ... --json`
//! and parses the result. Only commands with `--json`; one that lacks it gets `--json` added
//! in a pipeline repo change, never text parsing.

use crate::platform::{current_os, python_launcher};
use serde_json::Value;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The environment variable that points the bridge at another root (tests: a stub `fleet.py`
/// under `tests/fixtures/fleet-stub/`).
pub const FLEET_ROOT_ENV: &str = "FLEET_ROOT";

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
        }
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
    /// `with_env` pairs, stdin null, stdout and stderr captured.
    fn spawn(&self, command: &[String]) -> std::io::Result<std::process::Output> {
        let mut child = Command::new(&command[0]);
        child.args(&command[1..]);
        child.env(FLEET_ROOT_ENV, self.root.as_os_str());
        for (key, value) in &self.env {
            child.env(key, value);
        }
        child.stdin(Stdio::null());
        child.stdout(Stdio::piped());
        child.stderr(Stdio::piped());
        child.output()
    }

    /// Run the command line for `args` and wait. A failed spawn is a `FleetError` with
    /// reason `Spawn`; otherwise the command line, the exit code (`None` when the process
    /// was killed), stdout and stderr, each lossy UTF-8.
    fn run(&self, args: &[&str]) -> Result<(Vec<String>, Option<i32>, String, String), FleetError> {
        let command = self.command_line(args);
        let output = match self.spawn(&command) {
            Ok(output) => output,
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
        Ok((command, output.status.code(), stdout, stderr))
    }

    /// Run `fleet.py <args...> --json` with the platform's Python launcher and parse
    /// stdout. A failed spawn, a non-zero exit or stdout that is not JSON is a
    /// `FleetError`, never a guessed value.
    pub fn run_json(&self, args: &[&str]) -> Result<Value, FleetError> {
        let (command, code, stdout, stderr) = self.run(args)?;
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
        let (command, code, _stdout, stderr) = self.run(args)?;
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
