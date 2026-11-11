//! #432 (owner rulings 1-3, 10): the satellite's loops and holds on disk. A tick holds the
//! satellite lock only while it runs; every satellite on this machine counts the loops in
//! `<data>/satellite/loops.json`; a loop whose run ended is adopted and watched to its end, never
//! started twice; the holds after a failed loop are seen by every process, a dry run included,
//! which only reads them. A stub `fleet.py` stands in for the fleet: its loop waits for a `go`
//! file, so nothing here races a clock.

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use switchboard_core::autopilot::driver::{ChildRunner, LaneRunner};
use switchboard_core::autopilot::loops::{self, LoopRecord};
use switchboard_core::autopilot::run::{
    self, satellite_dry, satellite_lock_target, Pass, PassReport, SatelliteLog, SatelliteSession,
    SATELLITE_FREE_AGAIN,
};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::lock::{self, FileLock};

/// `fleet budget allow <lane>` says yes. `fleet run --next <lane>` waits for a file `go` beside
/// the script (up to a minute), prints one card and exits with the number in a file `exit`
/// (0 when there is none). Every call is logged beside the script.
const FLEET_STUB: &str = r#"import json, os, sys, time
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:2] == ["run", "--next"]:
    give_up = time.time() + 60
    while not os.path.exists(os.path.join(here, "go")) and time.time() < give_up:
        time.sleep(0.02)
    print(json.dumps({"id": "sb-1", "outcome": "done"}))
    code = os.path.join(here, "exit")
    sys.exit(int(open(code).read()) if os.path.exists(code) else 0)
"#;

/// A loop process this test starts itself: it waits for the same `go` file, as a loop that a
/// run started before it was killed would.
const ORPHAN: &str = r#"import os, sys, time
give_up = time.time() + 60
while not os.path.exists(sys.argv[1]) and time.time() < give_up:
    time.sleep(0.02)
"#;

struct World {
    pipeline: PathBuf,
    data: PathBuf,
    config: PathBuf,
}

fn world(tag: &str) -> World {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base =
        std::env::temp_dir().join(format!("sb-sat-disk-{tag}-{}-{nanos}", std::process::id()));
    let w = World {
        pipeline: base.join("pipeline"),
        data: base.join("data"),
        config: base.join("config"),
    };
    let fleet = w.pipeline.join("tools").join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(fleet.join("fleet.py"), FLEET_STUB).unwrap();
    std::fs::write(fleet.join("orphan.py"), ORPHAN).unwrap();
    for dir in [&w.data, &w.config] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(
        w.config.join("satellite.json"),
        r#"{ "satellite_on": true, "lanes": ["gemini"] }"#,
    )
    .unwrap();
    w
}

impl World {
    fn fleet(&self) -> FleetBridge {
        FleetBridge::with_root(&self.pipeline)
    }

    fn tools(&self) -> PathBuf {
        self.pipeline.join("tools").join("fleet")
    }

    fn go(&self) {
        std::fs::write(self.tools().join("go"), "").unwrap();
    }

    fn exit_with(&self, code: i32) {
        std::fs::write(self.tools().join("exit"), code.to_string()).unwrap();
    }

    fn satellite(&self) -> PathBuf {
        satellite_lock_target(&self.data)
    }

    /// How many `run --next` the stub was asked for.
    fn loops_started(&self) -> usize {
        std::fs::read_to_string(self.tools().join("calls.log"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("\"--next\""))
            .count()
    }
}

/// Every file under `dir` with its bytes.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else {
                files.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    files
}

/// A process id no process has now: a child that has ended and been reaped.
fn gone_pid() -> u32 {
    let mut child = Command::new("python3")
        .args(["-c", "pass"])
        .spawn()
        .unwrap();
    child.wait().unwrap();
    child.id()
}

/// Wait (up to a minute) until `done` says so.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let give_up = Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(Instant::now() < give_up, "waited a minute for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_tick_holds_the_satellite_lock_only_while_it_runs() {
    let w = world("tick-lock");
    let mut first = SatelliteSession::new(w.fleet(), &w.data);
    let tick = first.tick(&w.config, Duration::ZERO);
    assert_eq!(tick.lanes_started, ["gemini"], "{tick:?}");
    assert_eq!(tick.held_elsewhere, Some(false));
    // Ruling 1: the lock is given back when the tick returns, though the loop runs on.
    assert!(!lock::held(&w.satellite()));
    let file = loops::read(&w.satellite());
    assert_eq!(file.running(), ["gemini"], "{file:?}");
    assert_eq!(file.loops[0].owner_pid, std::process::id());
    wait_until("the loop to start", || w.loops_started() == 1);

    // A second satellite (another face) ticks at once: no lock held, no error, and the loop
    // on disk keeps it from starting a second copy.
    let mut second = SatelliteSession::new(w.fleet(), &w.data);
    let again = second.tick(&w.config, Duration::ZERO);
    assert!(!again.failed(), "{again:?}");
    assert!(again.lanes_started.is_empty(), "{again:?}");
    assert_eq!(w.loops_started(), 1);
    let status = run::status(&w.data, Some(&w.config), 0, None).unwrap();
    assert_eq!(status["satellite"]["live"], true);
    assert_eq!(
        status["satellite"]["running"],
        serde_json::json!(["gemini"])
    );

    // While a tick runs (another run holds the lock), a tick is refused.
    let held = FileLock::take(&w.satellite(), Duration::ZERO).unwrap();
    let refused = second.tick(&w.config, Duration::ZERO);
    assert!(refused.errors[0].contains("another run holds the satellite"));
    assert_eq!(refused.held_elsewhere, Some(true));
    drop(held);

    w.go();
    let end = first.finish(Duration::from_millis(20));
    assert_eq!(end.loops.len(), 1, "{end:?}");
    assert_eq!(end.loops[0].exit_code, Some(0));
    assert!(!end.loops[0].adopted);
    assert!(loops::read(&w.satellite()).loops.is_empty());
    assert!(second.finish(Duration::from_millis(20)).loops.is_empty());
}

#[test]
fn a_loop_whose_run_ended_is_adopted_and_watched_to_its_end() {
    let w = world("adopt");
    // A run started the gemini loop and was killed: the loop runs on, its owner is gone.
    let go = w.tools().join("go");
    let mut orphan: Child = Command::new("python3")
        .arg(w.tools().join("orphan.py"))
        .arg(&go)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let dir = w.satellite();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("gemini.out"),
        "{\"id\": \"sb-9\", \"outcome\": \"done\"}\n",
    )
    .unwrap();
    loops::update(&dir, |file| {
        file.loops.push(LoopRecord {
            lane: "gemini".into(),
            pid: orphan.id(),
            pid_token: loops::process_token(orphan.id()),
            owner_pid: gone_pid(),
            owner_token: Some("a run that ended".into()),
            command: vec![
                "fleet".into(),
                "run".into(),
                "--next".into(),
                "gemini".into(),
            ],
            started: 1,
        })
    })
    .unwrap();

    // A dry run counts it and changes nothing.
    let before = tree(&w.data);
    let dry = satellite_dry(&w.fleet(), &w.data, &w.config);
    assert!(dry.lanes_started.is_empty(), "{dry:?}");
    assert!(dry.notes.iter().any(|n| n.contains("gemini")), "{dry:?}");
    assert_eq!(tree(&w.data), before);

    // Ruling 2: the next satellite run adopts it and starts no second copy.
    let mut session = SatelliteSession::new(w.fleet(), &w.data);
    let tick = session.tick(&w.config, Duration::ZERO);
    assert!(!tick.failed(), "{tick:?}");
    assert!(tick.lanes_started.is_empty(), "{tick:?}");
    assert!(
        tick.notes
            .iter()
            .any(|n| n.contains("adopted the gemini loop")),
        "{tick:?}"
    );
    assert_eq!(w.loops_started(), 0);
    let file = loops::read(&dir);
    assert_eq!(file.loops[0].owner_pid, std::process::id(), "{file:?}");
    // A second tick neither adopts it again nor starts one.
    let again = session.tick(&w.config, Duration::ZERO);
    assert!(
        again.notes.is_empty() && again.lanes_started.is_empty(),
        "{again:?}"
    );
    assert_eq!(w.loops_started(), 0);

    // It is watched to its end: its cards read, its record gone, no fleet error.
    w.go();
    orphan.wait().unwrap();
    let end = session.finish(Duration::from_millis(20));
    assert_eq!(end.loops.len(), 1, "{end:?}");
    assert_eq!(end.loops[0].lane, "gemini");
    assert!(end.loops[0].adopted);
    assert_eq!(end.loops[0].exit_code, None);
    assert!(!end.failed(), "{end:?}");
    let file = loops::read(&dir);
    assert!(file.loops.is_empty() && file.holds.is_empty(), "{file:?}");

    // With it gone the lane starts again.
    let next = session.tick(&w.config, Duration::ZERO);
    assert_eq!(next.lanes_started, ["gemini"], "{next:?}");
    session.finish(Duration::from_millis(20));
}

#[test]
fn a_record_whose_loop_and_run_both_ended_is_cleared_at_the_next_tick() {
    let w = world("both-gone");
    let dir = w.satellite();
    loops::update(&dir, |file| {
        file.loops.push(LoopRecord {
            lane: "gemini".into(),
            pid: gone_pid(),
            pid_token: Some("x".into()),
            owner_pid: gone_pid(),
            owner_token: Some("y".into()),
            command: vec![],
            started: 1,
        })
    })
    .unwrap();
    let mut session = SatelliteSession::new(w.fleet(), &w.data);
    w.go();
    let tick = session.tick(&w.config, Duration::ZERO);
    assert_eq!(tick.loops.len(), 1, "{tick:?}");
    assert!(tick.loops[0].adopted);
    assert_eq!(tick.lanes_started, ["gemini"], "{tick:?}");
    session.finish(Duration::from_millis(20));
    assert!(loops::read(&dir).loops.is_empty());
}

#[test]
fn a_hold_is_on_disk_for_every_process_and_a_dry_run_only_reads_it() {
    let w = world("holds");
    w.go();
    w.exit_with(1);
    let mut first = SatelliteSession::new(w.fleet(), &w.data);
    assert_eq!(
        first.tick(&w.config, Duration::ZERO).lanes_started,
        ["gemini"]
    );
    let end = first.finish(Duration::from_millis(20));
    assert!(end.failed(), "{end:?}");

    // Ruling 3: a runner made fresh (another process's) sees the hold.
    let mut fresh = ChildRunner::new(w.fleet(), &w.satellite());
    assert_eq!(fresh.held(), ["gemini"]);
    let holds = fresh.holds();
    assert!(holds[0].1 > 590 && holds[0].1 <= 600, "{holds:?}");
    let mut second = SatelliteSession::new(w.fleet(), &w.data);
    let tick = second.tick(&w.config, Duration::ZERO);
    assert!(tick.lanes_started.is_empty(), "{tick:?}");
    assert_eq!(w.loops_started(), 1);

    // A dry run sees it too and writes nothing.
    let before = tree(&w.data);
    let dry = satellite_dry(&w.fleet(), &w.data, &w.config);
    assert!(dry.lanes_started.is_empty(), "{dry:?}");
    assert_eq!(tree(&w.data), before);
    let status = run::status(&w.data, Some(&w.config), loops::now_ms() / 1000, None).unwrap();
    assert_eq!(status["satellite"]["held"][0]["lane"], "gemini", "{status}");
    assert!(status["satellite"]["as_of"].is_i64());
    assert_eq!(tree(&w.data), before);
}

#[test]
fn loops_file_changes_from_many_threads_are_all_kept() {
    let w = world("many");
    let dir = w.satellite();
    let threads: Vec<_> = (0..8)
        .map(|n| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                for i in 0..10 {
                    loops::update(&dir, |file| {
                        file.hold(&format!("lane-{n}-{i}"), i64::MAX, 0);
                    })
                    .unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(loops::read(&dir).holds.len(), 80);
    // No temp file is left beside it.
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["loops.json"], "{names:?}");
}

#[test]
fn the_process_token_tells_a_running_process_from_an_ended_one() {
    assert!(loops::process_token(std::process::id()).is_some());
    assert!(loops::alive(
        std::process::id(),
        loops::process_token(std::process::id()).as_deref()
    ));
    assert!(!loops::alive(std::process::id(), Some("another start")));
    assert!(loops::process_token(gone_pid()).is_none());
    // An ended child not yet reaped is not running.
    let mut child = Command::new("python3")
        .args(["-c", "pass"])
        .spawn()
        .unwrap();
    let pid = child.id();
    wait_until("the child to end", || loops::process_token(pid).is_none());
    child.wait().unwrap();
}

fn refused() -> PassReport {
    let mut report = PassReport::new(Pass::Satellite, false);
    report.held_elsewhere = Some(true);
    report
        .errors
        .push("another run holds the satellite (lock held)".to_string());
    report
}

fn took(lanes: &[&str]) -> PassReport {
    let mut report = PassReport::new(Pass::Satellite, false);
    report.held_elsewhere = Some(false);
    report.lanes_started = lanes.iter().map(|s| s.to_string()).collect();
    report
}

#[test]
fn being_held_out_is_said_when_it_starts_and_when_it_ends_not_every_tick() {
    let mut log = SatelliteLog::default();
    assert!(log.lines(&took(&[])).is_empty());
    let first = log.lines(&refused());
    assert_eq!(first.len(), 1);
    assert!(
        first[0].contains("another run holds the satellite"),
        "{first:?}"
    );
    for _ in 0..5 {
        assert!(log.lines(&refused()).is_empty());
    }
    assert_eq!(
        log.lines(&took(&["gemini"])),
        [SATELLITE_FREE_AGAIN, "satellite: started gemini"]
    );
    assert!(log.lines(&took(&[])).is_empty());
    // A tick that did not ask for the lock (the switch is off) changes nothing.
    assert!(log.lines(&refused()).len() == 1);
    let off = PassReport::new(Pass::Satellite, false);
    assert!(log.lines(&off).is_empty());
    assert!(log.lines(&refused()).is_empty());
    // Other errors are still said each time.
    let mut other = refused();
    other.errors.push("gemini: fleet exit 1".to_string());
    assert_eq!(log.lines(&other), ["satellite: gemini: fleet exit 1"]);
}

#[test]
fn the_satellite_report_says_whether_a_loop_was_adopted() {
    let w = world("json");
    w.go();
    let mut session = SatelliteSession::new(w.fleet(), &w.data);
    session.tick(&w.config, Duration::ZERO);
    let end = session.finish(Duration::from_millis(20));
    let json: Value = serde_json::to_value(&end).unwrap();
    assert_eq!(json["loops"][0]["adopted"], false, "{json}");
}

#[test]
fn a_session_uses_the_bridge_it_was_last_given() {
    // Ruling 4: the window reads its settings each tick; a new pipeline is used at once.
    let w = world("bridge-a");
    let other = world("bridge-b");
    let mut session = SatelliteSession::new(w.fleet(), &w.data);
    session.set_fleet(other.fleet());
    other.go();
    let tick = session.tick(&w.config, Duration::ZERO);
    assert_eq!(tick.lanes_started, ["gemini"], "{tick:?}");
    session.finish(Duration::from_millis(20));
    assert_eq!(w.loops_started(), 0);
    assert_eq!(other.loops_started(), 1);
}
