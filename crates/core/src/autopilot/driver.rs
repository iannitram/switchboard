//! AUTO-6: the satellite's script loops. One `fleet run --next <lane>` per service lane, at most
//! Qwen plus one service job at once (R10). No model is in the loop: a failed card is classed by
//! AUTO-9's `classify`.

use super::classify::{classify, FailureClass};
use crate::fleet::{FleetBridge, FLEET_ROOT_ENV};
use crate::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// The lanes the fleet knows. A lane in the satellite file that is not here is skipped.
pub const FLEET_LANES: [&str; 5] = ["chatgpt", "claudeapi", "deepseek", "gemini", "qwen"];

/// The satellite switch: a file in the app's config folder. Missing means off.
pub const SATELLITE_FILE: &str = "satellite.json";

/// The satellite file: whether the satellite runs, and the lanes it may start, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SatelliteConfig {
    pub satellite_on: bool,
    pub lanes: Vec<String>,
}

impl Default for SatelliteConfig {
    fn default() -> Self {
        SatelliteConfig {
            satellite_on: false,
            lanes: ["qwen", "gemini", "chatgpt", "deepseek", "claudeapi"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

/// The satellite file at `path`. A missing file is the default (off); text that does not parse
/// is a parse error that names the file.
pub fn load_config(path: &Path) -> Result<SatelliteConfig, CoreError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SatelliteConfig::default());
        }
        Err(e) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
    };
    serde_json::from_str(&text).map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))
}

/// The lanes running now, and the lane asked for: may it start?
pub fn may_start(running: &[String], lane: &str) -> bool {
    // A lane never runs twice at once.
    if running.iter().any(|r| r.as_str() == lane) {
        return false;
    }
    // Qwen may always start (it is the one lane that may share with a service job).
    if lane == "qwen" {
        return true;
    }
    // Any other lane starts only when no non-Qwen service job is running.
    running.iter().all(|r| r.as_str() == "qwen")
}

/// The arguments of a lane's loop: `run --next <lane>`.
pub fn run_args(lane: &str) -> Vec<String> {
    ["run", "--next", lane].iter().map(|s| s.to_string()).collect()
}

/// The command a loop runs for a lane: `fleet run --next <lane>`.
pub fn next_command(lane: &str) -> Vec<String> {
    let mut command = vec!["fleet".to_string()];
    command.extend(run_args(lane));
    command
}

/// Where the satellite's loops run: start one for a lane, and say which are running.
pub trait LaneRunner {
    fn running(&mut self) -> Vec<String>;
    fn start(&mut self, lane: &str) -> Result<(), CoreError>;
}

/// One pass of the satellite: with the switch on, start one loop for each configured lane that
/// `may_start` allows and the budget (`may_spend`, AUTO-1) allows. Returns the lanes started.
/// The first error stops the pass; the lanes already started stay started.
pub fn tick(
    config: &SatelliteConfig,
    runner: &mut dyn LaneRunner,
    may_spend: &dyn Fn(&str) -> Result<bool, CoreError>,
) -> Result<Vec<String>, CoreError> {
    let mut started = Vec::new();
    if !config.satellite_on {
        return Ok(started);
    }
    let mut running = runner.running();
    for lane in &config.lanes {
        if !FLEET_LANES.contains(&lane.as_str()) || !may_start(&running, lane) {
            continue;
        }
        if !may_spend(lane.as_str())? {
            continue;
        }
        runner.start(lane.as_str())?;
        running.push(lane.clone());
        started.push(lane.clone());
    }
    Ok(started)
}

/// The JSON object lines of a loop's output, in order. Other lines are dropped.
pub fn cards(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .filter(Value::is_object)
        .collect()
}

/// The cards whose outcome is `failed`, `infra` or `timeout-partial`.
pub fn failed_cards(stdout: &str) -> Vec<Value> {
    cards(stdout)
        .into_iter()
        .filter(|card| {
            matches!(
                card.get("outcome").and_then(Value::as_str),
                Some("failed" | "infra" | "timeout-partial")
            )
        })
        .collect()
}

/// AUTO-9 for each failed card of a finished loop: the card is written to `dir` and classed by
/// `classify`. A card with no string `id` is named `card-<n>` by its place among the failed cards.
pub fn classify_failed(
    fleet: &FleetBridge,
    dir: &Path,
    stdout: &str,
) -> Result<Vec<(String, FailureClass)>, CoreError> {
    std::fs::create_dir_all(dir).map_err(|e| CoreError::Io(format!("{}: {e}", dir.display())))?;
    let mut classes = Vec::new();
    for (i, card) in failed_cards(stdout).iter().enumerate() {
        let id = card
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("card-{}", i + 1));
        let path = dir.join(format!("{id}.json"));
        std::fs::write(&path, card.to_string())
            .map_err(|e| CoreError::Io(format!("{}: {e}", path.display())))?;
        let class = classify(fleet, &path)?;
        classes.push((id, class));
    }
    Ok(classes)
}

/// The app's runner: each lane's loop is a child process (the bridge's command line for
/// `run_args`, `FLEET_ROOT` set), its stdout written to `<out_dir>/<lane>.out`.
pub struct ChildRunner {
    fleet: FleetBridge,
    out_dir: PathBuf,
    children: Vec<(String, Child)>,
    finished: Vec<(String, String)>,
}

impl ChildRunner {
    pub fn new(fleet: FleetBridge, out_dir: &Path) -> Self {
        ChildRunner {
            fleet,
            out_dir: out_dir.to_path_buf(),
            children: Vec::new(),
            finished: Vec::new(),
        }
    }

    /// (lane, stdout) of each loop that has ended since the last call, oldest first.
    pub fn take_finished(&mut self) -> Vec<(String, String)> {
        self.reap();
        std::mem::take(&mut self.finished)
    }

    fn reap(&mut self) {
        let mut still = Vec::new();
        for (lane, mut child) in std::mem::take(&mut self.children) {
            match child.try_wait() {
                Ok(None) => still.push((lane, child)),
                _ => {
                    let out = self.out_dir.join(format!("{lane}.out"));
                    let text = std::fs::read_to_string(out).unwrap_or_default();
                    self.finished.push((lane, text));
                }
            }
        }
        self.children = still;
    }
}

impl LaneRunner for ChildRunner {
    fn running(&mut self) -> Vec<String> {
        self.reap();
        self.children.iter().map(|(l, _)| l.clone()).collect()
    }

    fn start(&mut self, lane: &str) -> Result<(), CoreError> {
        let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", self.out_dir.display()));
        std::fs::create_dir_all(&self.out_dir).map_err(io)?;
        let out = std::fs::File::create(self.out_dir.join(format!("{lane}.out"))).map_err(io)?;
        let args = run_args(lane);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let command = self.fleet.command_line(&args);
        let child = Command::new(&command[0])
            .args(&command[1..])
            .env(FLEET_ROOT_ENV, self.fleet.root())
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::null())
            .spawn()
            .map_err(io)?;
        self.children.push((lane.to_string(), child));
        Ok(())
    }
}
