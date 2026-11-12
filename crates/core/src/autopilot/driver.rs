//! AUTO-6: the satellite's script loops. One `fleet run --next <lane>` per service lane, at most
//! Qwen plus one service job at once (R10). No model is in the loop: a failed card is classed by
//! AUTO-9's `classify`.

use super::classify::{classify, FailureClass};
use super::loops::{self, LoopRecord};
use crate::fleet::{FleetBridge, FleetError, FleetErrorReason, FLEET_ROOT_ENV};
use crate::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

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
    ["run", "--next", lane]
        .iter()
        .map(|s| s.to_string())
        .collect()
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
    /// The lanes not to start now: a loop that ended with an error holds its lane for a while
    /// (`ChildRunner`, #383) instead of being started again at the next tick. None by default.
    fn held(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// One pass of the satellite: with the switch on, start one loop for each configured lane that
/// is not held (`LaneRunner::held`), that `may_start` allows and the budget (`may_spend`, AUTO-1)
/// allows. Returns the lanes started.
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
    let held = runner.held();
    for lane in &config.lanes {
        if !FLEET_LANES.contains(&lane.as_str())
            || held.contains(lane)
            || !may_start(&running, lane)
        {
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

/// The longest file name `card_file_name` gives before it shortens one (`<prefix>~<hash>.json`).
const CARD_NAME_MAX: usize = 150;

/// The file name a failed card is written under in the cards folder (#382). The id is kept as it
/// is where it is plain (`a-z 0-9 _ -`); every other byte (`/`, `\`, `.`, `%`, a capital letter,
/// a space, non-ASCII) is written `%XX`, so `../x`, `a/b` or an absolute path stays one file
/// inside the folder, and two different ids never share a file, also on a file system that does
/// not tell capitals apart (Windows, macOS). A name that is a Windows device name (`con`,
/// `nul`, `com1`, ...) or starts with `card-` has its first letter written `%XX` too. A card
/// with no id, or an empty one, is `card-<n>`. A name longer than `CARD_NAME_MAX` is cut and
/// ends in `~` and a hash of the whole id.
pub fn card_file_name(id: Option<&str>, n: usize) -> String {
    let id = match id {
        Some(id) if !id.is_empty() => id,
        _ => return format!("card-{n}.json"),
    };
    let mut name = String::new();
    for b in id.bytes() {
        if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-' {
            name.push(b as char);
        } else {
            name.push_str(&format!("%{b:02X}"));
        }
    }
    let device = matches!(name.as_str(), "con" | "prn" | "aux" | "nul")
        || ((name.starts_with("com") || name.starts_with("lpt"))
            && name.len() == 4
            && name.as_bytes()[3].is_ascii_digit());
    if device || name.starts_with("card-") {
        name = format!("%{:02X}{}", name.as_bytes()[0], &name[1..]);
    }
    if name.len() > CARD_NAME_MAX {
        let mut cut = CARD_NAME_MAX - 30;
        // Never cut inside a `%XX`.
        while name.as_bytes()[..cut]
            .iter()
            .rev()
            .take(2)
            .any(|&b| b == b'%')
        {
            cut -= 1;
        }
        // FNV-1a, 64 bits: the same id gives the same name on every run.
        let hash = id.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
        name = format!("{}~{hash:016x}", &name[..cut]);
    }
    format!("{name}.json")
}

/// AUTO-9 for each failed card of a finished loop: the card is written to `dir` under
/// `card_file_name` and classed by `classify`. The class is answered under the card's own id; a
/// card with no string `id` is named `card-<n>` by its place among the failed cards.
pub fn classify_failed(
    fleet: &FleetBridge,
    dir: &Path,
    stdout: &str,
) -> Result<Vec<(String, FailureClass)>, CoreError> {
    std::fs::create_dir_all(dir).map_err(|e| CoreError::Io(format!("{}: {e}", dir.display())))?;
    let mut classes = Vec::new();
    for (i, card) in failed_cards(stdout).iter().enumerate() {
        let raw = card.get("id").and_then(Value::as_str);
        let id = raw.map_or_else(|| format!("card-{}", i + 1), str::to_string);
        let path = dir.join(card_file_name(raw, i + 1));
        std::fs::write(&path, card.to_string())
            .map_err(|e| CoreError::Io(format!("{}: {e}", path.display())))?;
        let class = classify(fleet, &path)?;
        classes.push((id, class));
    }
    Ok(classes)
}

/// How long a lane whose loop ended with an error waits before it is started again: 10 minutes
/// (ten satellite ticks), so a loop that dies at once is not restarted every tick (#383).
pub const FAILED_LOOP_HOLD: Duration = Duration::from_secs(600);

/// The exit code of a fleet command that says "not now" (CORE-1): an answer, not an error.
pub const NOT_NOW: i32 = 75;

/// One loop that has ended: its lane, its stdout, and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopEnd {
    pub lane: String,
    /// The command line it ran, program first.
    pub command: Vec<String>,
    pub stdout: String,
    /// The exit code; `None` when it was killed by a signal, its status could not be read, or
    /// it was adopted (#432: another process started it, so its exit code is not known here).
    pub exit_code: Option<i32>,
    pub stderr: String,
    /// #432: the loop was started by a run that ended before it, and this run watched it to its
    /// end (`ChildRunner::adopt`).
    pub adopted: bool,
}

impl LoopEnd {
    /// Did the loop end with an error: a non-zero exit, a signal, or no status?
    pub fn failed(&self) -> bool {
        self.exit_code != Some(0)
    }

    /// CORE-2: the fleet error of a loop that failed, with its command, exit code and stderr;
    /// `None` for a loop that exited 0, or 75 ("not now": quiet hours, a braked pool, a busy dev
    /// gate), which the fleet answers on purpose (CORE-1); the lane is still held. `None` too
    /// for an adopted loop, whose exit code is not known.
    pub fn fleet_error(&self) -> Option<CoreError> {
        (self.failed() && !self.adopted && self.exit_code != Some(NOT_NOW)).then(|| {
            CoreError::Fleet(FleetError {
                command: self.command.clone(),
                exit_code: self.exit_code,
                stderr: self.stderr.clone(),
                reason: FleetErrorReason::Exit,
                undo_note: None,
            })
        })
    }
}

/// The app's runner: each lane's loop is a child process (the bridge's command line for
/// `run_args`, `FLEET_ROOT` set), its stdout written to `<out_dir>/<lane>.out` and its stderr to
/// `<out_dir>/<lane>.err`. A loop that ended with an error holds its lane for `hold`.
///
/// #432: the loops and the holds are kept on disk, in `<out_dir>/loops.json` (`loops`), so
/// every process on this machine sees them: `running` and `held` are what that file says, and
/// a loop whose run has ended is adopted (`adopt`) and watched to its end here.
pub struct ChildRunner {
    fleet: FleetBridge,
    out_dir: PathBuf,
    hold: Duration,
    children: Vec<(String, Vec<String>, Child)>,
    /// Loops of a run that ended, watched here by their process id.
    adopted: Vec<LoopRecord>,
    finished: Vec<LoopEnd>,
    /// What could not be written to `loops.json` since `take_errors`.
    errors: Vec<CoreError>,
}

impl ChildRunner {
    pub fn new(fleet: FleetBridge, out_dir: &Path) -> Self {
        Self::with_hold(fleet, out_dir, FAILED_LOOP_HOLD)
    }

    /// `new` with the hold after a failed loop given (a test's is short).
    pub fn with_hold(fleet: FleetBridge, out_dir: &Path, hold: Duration) -> Self {
        ChildRunner {
            fleet,
            out_dir: out_dir.to_path_buf(),
            hold,
            children: Vec::new(),
            adopted: Vec::new(),
            finished: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// #432: the bridge the next loop starts with (the window reads its settings each tick).
    pub fn set_fleet(&mut self, fleet: FleetBridge) {
        self.fleet = fleet;
    }

    /// The lanes held now after a failed loop, each with the seconds left (#385: the status
    /// read), as `loops.json` has them.
    pub fn holds(&mut self) -> Vec<(String, u64)> {
        self.reap();
        loops::read(&self.out_dir)
            .holds_at(loops::now_ms())
            .into_iter()
            .map(|(lane, ms)| (lane, (ms as u64).div_ceil(1000)))
            .collect()
    }

    /// The lanes of the loops this runner watches: the ones it started and the ones it adopted.
    pub fn watching(&mut self) -> Vec<String> {
        self.reap();
        self.children
            .iter()
            .map(|(lane, _, _)| lane.clone())
            .chain(self.adopted.iter().map(|record| record.lane.clone()))
            .collect()
    }

    /// #432: take over each loop in `loops.json` whose owner has ended (a headless run that
    /// exited or was killed): this process is its owner from now on and watches it to its end,
    /// so no second copy of it starts. A loop that has ended already ends at the next reap. The
    /// lanes taken. Call it under the satellite lock, so two runs never take the same loop.
    pub fn adopt(&mut self) -> Result<Vec<String>, CoreError> {
        let me = std::process::id();
        let token = loops::process_token(me);
        let taken = loops::update(&self.out_dir, |file| {
            let mut taken = Vec::new();
            for record in &mut file.loops {
                if record.owned() {
                    continue;
                }
                record.owner_pid = me;
                record.owner_token = token.clone();
                taken.push(record.clone());
            }
            taken
        })?;
        let lanes = taken.iter().map(|record| record.lane.clone()).collect();
        self.adopted.extend(taken);
        Ok(lanes)
    }

    /// Each loop that has ended since the last call, oldest first.
    pub fn take_finished(&mut self) -> Vec<LoopEnd> {
        self.reap();
        std::mem::take(&mut self.finished)
    }

    /// What could not be written to `loops.json` since the last call.
    pub fn take_errors(&mut self) -> Vec<CoreError> {
        std::mem::take(&mut self.errors)
    }

    fn read(&self, lane: &str, ext: &str) -> String {
        std::fs::read_to_string(self.out_dir.join(format!("{lane}.{ext}"))).unwrap_or_default()
    }

    fn reap(&mut self) {
        let mut ended: Vec<(LoopEnd, u32)> = Vec::new();
        let mut still = Vec::new();
        for (lane, command, mut child) in std::mem::take(&mut self.children) {
            let status = match child.try_wait() {
                Ok(None) => {
                    still.push((lane, command, child));
                    continue;
                }
                Ok(Some(status)) => status.code(),
                Err(_) => None,
            };
            let end = LoopEnd {
                stdout: self.read(&lane, "out"),
                stderr: self.read(&lane, "err"),
                exit_code: status,
                command,
                lane,
                adopted: false,
            };
            ended.push((end, child.id()));
        }
        self.children = still;
        let mut watched = Vec::new();
        for record in std::mem::take(&mut self.adopted) {
            if record.running() {
                watched.push(record);
                continue;
            }
            let end = LoopEnd {
                stdout: self.read(&record.lane, "out"),
                stderr: self.read(&record.lane, "err"),
                exit_code: None,
                command: record.command.clone(),
                lane: record.lane.clone(),
                adopted: true,
            };
            ended.push((end, record.pid));
        }
        self.adopted = watched;
        if ended.is_empty() {
            return;
        }
        let now = loops::now_ms();
        let until = now.saturating_add(self.hold.as_millis() as i64);
        let written = loops::update(&self.out_dir, |file| {
            for (end, pid) in &ended {
                file.forget(&end.lane, *pid);
                // An adopted loop's exit code is not known: it holds nothing.
                if end.failed() && !end.adopted {
                    file.hold(&end.lane, until, now);
                }
            }
        });
        if let Err(e) = written {
            self.errors.push(e);
        }
        self.finished.extend(ended.into_iter().map(|(end, _)| end));
    }
}

impl LaneRunner for ChildRunner {
    /// The lanes whose loop runs now on this machine, whoever started it (`loops.json`), and
    /// the ones this runner started.
    fn running(&mut self) -> Vec<String> {
        self.reap();
        let mut lanes = loops::read(&self.out_dir).running();
        for lane in self.watching() {
            if !lanes.contains(&lane) {
                lanes.push(lane);
            }
        }
        lanes
    }

    fn held(&mut self) -> Vec<String> {
        self.holds().into_iter().map(|(lane, _)| lane).collect()
    }

    fn start(&mut self, lane: &str) -> Result<(), CoreError> {
        let io = |e: std::io::Error| CoreError::Io(format!("{}: {e}", self.out_dir.display()));
        std::fs::create_dir_all(&self.out_dir).map_err(io)?;
        let out = std::fs::File::create(self.out_dir.join(format!("{lane}.out"))).map_err(io)?;
        let err = std::fs::File::create(self.out_dir.join(format!("{lane}.err"))).map_err(io)?;
        let args = run_args(lane);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let command = self.fleet.command_line(&args);
        let child = Command::new(&command[0])
            .args(&command[1..])
            .env(FLEET_ROOT_ENV, self.fleet.root())
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .spawn()
            .map_err(io)?;
        let me = std::process::id();
        let record = LoopRecord {
            lane: lane.to_string(),
            pid: child.id(),
            pid_token: loops::process_token(child.id()),
            owner_pid: me,
            owner_token: loops::process_token(me),
            command: command.clone(),
            started: loops::now_ms() / 1000,
        };
        self.children.push((lane.to_string(), command, child));
        // #432: on disk, so every satellite on this machine counts it, and the next one adopts
        // it if this process ends first. The loop runs (and is watched here) either way.
        loops::update(&self.out_dir, |file| file.loops.push(record))
    }
}
