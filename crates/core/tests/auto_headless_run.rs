//! #385: the passes behind the window's threads, the CLI and the MCP (`autopilot::run`). Each
//! poll pass runs alone through `run_poll_passes`; a dry run reports the same work and changes
//! no file; the lock keeps a second run out; the satellite session holds its lock while its
//! loops run. A fake starter and a stub `fleet.py` and `merge_pr.py` stand in for the fleet.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use switchboard_core::autopilot::passes::{AutopilotState, JobStarter, STATE_FILE};
use switchboard_core::autopilot::run::{
    self, passes_lock_target, run_poll_passes, satellite_dry, satellite_lock_target, Pass,
    PassReport, RunEnv, SatelliteSession,
};
use switchboard_core::clock::FixedClock;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::{Issue, PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::lock::{self, FileLock};
use switchboard_core::rules::{guard_autopilot, Caller, Face, Role, Verdict};
use switchboard_core::CoreError;

const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
/// 2026-11-08, a Sunday, at 2:05pm UTC: 6:05am Pacific.
const SUNDAY_AFTER_SIX: i64 = 1_794_146_700;
const PACIFIC: i64 = -8 * 3_600;

const MERGE_STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(sys.argv[1:]) + "\n")
"#;

/// `fleet budget allow <lane>` says yes; `fleet run --next <lane>` prints one card; every call
/// is logged beside the script.
const FLEET_STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:2] == ["run", "--next"]:
    print(json.dumps({"id": "sb-1", "outcome": "done"}))
"#;

/// A pipeline (stub `merge_pr.py` and `fleet.py`), a dev root with a clone of `demo`, a data
/// folder and a config folder.
struct World {
    pipeline: PathBuf,
    dev: PathBuf,
    data: PathBuf,
    config: PathBuf,
}

fn world(tag: &str) -> World {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base =
        std::env::temp_dir().join(format!("sb-auto-run-{tag}-{}-{nanos}", std::process::id()));
    let w = World {
        pipeline: base.join("pipeline"),
        dev: base.join("dev"),
        data: base.join("data"),
        config: base.join("config"),
    };
    for (dir, name, text) in [
        ("merge-pr", "merge_pr.py", MERGE_STUB),
        ("fleet", "fleet.py", FLEET_STUB),
    ] {
        let dir = w.pipeline.join("tools").join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), text).unwrap();
    }
    for dir in [&w.dev.join("demo"), &w.data, &w.config] {
        std::fs::create_dir_all(dir).unwrap();
    }
    w
}

fn calls(w: &World, tool: &str) -> Vec<Vec<String>> {
    std::fs::read_to_string(w.pipeline.join("tools").join(tool).join("calls.log"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// Every file under `dir` with its bytes.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
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

/// Says yes to every lane; logs every start.
#[derive(Default)]
struct FakeStarter {
    asked: Vec<String>,
    jobs: Vec<Value>,
    /// Checked at each question: is the passes' lock held now?
    lock_target: Option<PathBuf>,
    lock_seen: Vec<bool>,
}

impl JobStarter for FakeStarter {
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError> {
        self.asked.push(lane.to_string());
        if let Some(target) = &self.lock_target {
            let free = FileLock::take(target, Duration::ZERO).is_ok();
            self.lock_seen.push(lock::held(target) && !free);
        }
        Ok(true)
    }
    fn start(&mut self, job: &Value, _brief: &str) -> Result<(), CoreError> {
        self.jobs.push(job.clone());
        Ok(())
    }
    fn finished(&mut self, _: &str) -> Result<bool, CoreError> {
        Ok(true)
    }
}

#[derive(Default)]
struct FakeGitHub {
    reads: Mutex<Vec<String>>,
}

impl GitHubTransport for FakeGitHub {
    fn graphql(&self, _query: &str, variables: &Value) -> Result<Value, CoreError> {
        self.reads.lock().unwrap().push(format!(
            "{}/{}",
            variables["owner"].as_str().unwrap_or("?"),
            variables["name"].as_str().unwrap_or("?")
        ));
        Ok(json!({ "repository": {} }))
    }

    fn rest_get(&self, _path: &str, _etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        Err(CoreError::GitHub("no REST here".to_string()))
    }
}

/// `iannitram/demo`: issue 12, PR 7 opened by this machine and built by Qwen with no review
/// yet, PR 8 with a PASS on its head and green CI, and a ROADMAP.md.
fn demo() -> RepoSnapshot {
    let pr = |number: u64, passed: bool| PullRequest {
        number,
        title: format!("PR {number}"),
        state: "open".into(),
        head_sha: HEAD.into(),
        head_ref: format!("sat/{number}"),
        labels: vec!["by:test-box".into(), "model:qwen3-coder".into()],
        closes: vec![12],
        verdicts: if passed {
            vec![ReviewVerdict {
                verdict: "PASS".into(),
                sha: HEAD.into(),
                needs_me: false,
            }]
        } else {
            vec![]
        },
        ci_on_head: "success".into(),
    };
    RepoSnapshot {
        repo: "iannitram/demo".into(),
        read_at: 1_000,
        issues: vec![Issue {
            number: 12,
            title: "Issue 12".into(),
            state: "open".into(),
            labels: vec!["size:easy".into(), "kind:wiring".into()],
            ..Default::default()
        }],
        pulls: vec![pr(7, false), pr(8, true)],
        roadmap_md: Some("# the new plan".into()),
        ..Default::default()
    }
}

/// A world whose data folder holds `demo` and a state that saw an older ROADMAP and ran the
/// weekly run a week before.
fn demo_world(tag: &str) -> World {
    let w = world(tag);
    SnapshotStore::new(&w.data).save(&demo()).unwrap();
    let state = AutopilotState {
        roadmaps: vec![("iannitram/demo".into(), Some("# the old plan".into()))],
        weekly_last_run: Some(SUNDAY_AFTER_SIX - 7 * 86_400),
        ..Default::default()
    };
    state.save(&w.data).unwrap();
    w
}

fn run_one(
    w: &World,
    pass: Pass,
    dry_run: bool,
    starter: &mut FakeStarter,
    github: &FakeGitHub,
) -> Result<PassReport, CoreError> {
    let clock = FixedClock::at(SUNDAY_AFTER_SIX);
    let env = RunEnv {
        data_dir: &w.data,
        pipeline_root: &w.pipeline,
        dev_root: &w.dev,
        machine: Some("test-box"),
        clock: &clock,
        utc_offset_secs: Some(PACIFIC),
        transport: github,
        dry_run,
        lock_wait: Duration::ZERO,
    };
    let mut reports = run_poll_passes(&env, &[pass], starter)?;
    assert_eq!(reports.len(), 1, "one pass, one report");
    Ok(reports.remove(0))
}

/// The pass as a dry run, then for real: the dry run changes no file, starts no job and
/// merges nothing; both reports are answered.
fn dry_then_real(w: &World, pass: Pass) -> (PassReport, PassReport, FakeStarter, FakeGitHub) {
    let before = tree(&w.data);
    let mut starter = FakeStarter::default();
    let github = FakeGitHub::default();
    let dry = run_one(w, pass, true, &mut starter, &github).unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.pass, pass.name());
    assert!(starter.jobs.is_empty(), "a dry run starts no job");
    assert!(calls(w, "merge-pr").is_empty(), "a dry run merges nothing");
    assert!(github.reads.lock().unwrap().is_empty());
    assert_eq!(tree(&w.data), before, "a dry run writes nothing");
    assert!(!dry.failed(), "{:?}", dry.errors);

    let real = run_one(w, pass, false, &mut starter, &github).unwrap();
    assert!(!real.dry_run);
    assert!(!real.failed(), "{:?}", real.errors);
    (dry, real, starter, github)
}

#[test]
fn a_pass_has_a_name_and_a_role() {
    for pass in Pass::ALL {
        assert_eq!(Pass::parse(pass.name()), Some(pass));
    }
    assert_eq!(Pass::parse("status"), None);
    assert_eq!(Pass::parse("Merge"), None);
    assert_eq!(Pass::Satellite.role(), Role::Satellite);
    for pass in Pass::AFTER_POLL {
        assert_eq!(pass.role(), Role::ProjectManager);
    }
}

#[test]
fn the_review_pass_starts_the_review_and_a_dry_run_says_which() {
    let w = demo_world("review");
    let (dry, real, starter, _) = dry_then_real(&w, Pass::Review);
    assert_eq!(dry.started.len(), 1, "{dry:?}");
    assert_eq!(dry.started, real.started);
    assert_eq!(starter.jobs.len(), 1);
    assert!(dry.text().contains("would start"), "{}", dry.text());
    assert!(
        real.text().contains("autopilot: started"),
        "{}",
        real.text()
    );
    let state = AutopilotState::load(&w.data).unwrap();
    assert_eq!(state.reviews_started, ["iannitram/demo#7"]);
    // The roadmap was not this pass's to act on.
    assert_eq!(
        state.roadmaps,
        [(
            "iannitram/demo".to_string(),
            Some("# the old plan".to_string())
        )]
    );
}

#[test]
fn the_roadmap_pass_starts_the_project_manager_and_a_dry_run_says_which() {
    let w = demo_world("roadmap");
    let (dry, real, starter, _) = dry_then_real(&w, Pass::Roadmap);
    assert_eq!(dry.started.len(), 1, "{dry:?}");
    assert_eq!(dry.started, real.started);
    assert_eq!(starter.jobs.len(), 1);
    let state = AutopilotState::load(&w.data).unwrap();
    assert_eq!(
        state.roadmaps,
        [(
            "iannitram/demo".to_string(),
            Some("# the new plan".to_string())
        )]
    );
    assert_eq!(state.pm_running, real.started.first().cloned());
    assert!(state.reviews_started.is_empty(), "no review in this pass");
}

#[test]
fn the_merge_pass_merges_what_passed_and_a_dry_run_says_which() {
    let w = demo_world("merge");
    let (dry, real, starter, github) = dry_then_real(&w, Pass::Merge);
    assert_eq!(dry.merged.len(), 1, "{dry:?}");
    assert_eq!(dry.merged[0].repo, "iannitram/demo");
    assert_eq!(dry.merged[0].prs, [8]);
    assert_eq!(dry.merged[0].closes, [12]);
    assert!(
        dry.text().contains("would merge iannitram/demo [8]"),
        "{}",
        dry.text()
    );
    assert_eq!(real.merged, dry.merged);
    assert!(
        real.text().contains("autopilot: merged iannitram/demo [8]"),
        "{}",
        real.text()
    );
    assert!(starter.jobs.is_empty() && starter.asked.is_empty());
    let merges = calls(&w, "merge-pr");
    assert!(!merges.is_empty());
    assert!(
        merges.iter().all(|args| args.contains(&"8".to_string())),
        "{merges:?}"
    );
    assert_eq!(*github.reads.lock().unwrap(), ["iannitram/demo"]);
}

#[test]
fn the_weekly_pass_starts_the_drift_clear_and_names_the_backup() {
    let w = demo_world("weekly");
    let (dry, real, starter, _) = dry_then_real(&w, Pass::Weekly);
    assert_eq!(dry.started.len(), 1, "{dry:?}");
    assert_eq!(dry.backup, ["iannitram/demo"]);
    assert!(
        dry.text()
            .contains("weekly backup: would back up iannitram/demo"),
        "{}",
        dry.text()
    );
    assert_eq!(real.started, dry.started);
    assert_eq!(real.backup, dry.backup);
    assert_eq!(starter.jobs.len(), 1);
    let state = AutopilotState::load(&w.data).unwrap();
    assert_eq!(state.weekly_last_run, Some(SUNDAY_AFTER_SIX));

    // Not due again this week: nothing to do, as text too.
    let mut starter = FakeStarter::default();
    let again = run_one(
        &w,
        Pass::Weekly,
        false,
        &mut starter,
        &FakeGitHub::default(),
    )
    .unwrap();
    assert!(again.started.is_empty() && again.backup.is_empty());
    assert_eq!(again.text(), "autopilot weekly: nothing to do\n");
}

#[test]
fn the_weekly_pass_waits_when_the_utc_offset_is_not_known() {
    let w = demo_world("no-offset");
    let before = tree(&w.data);
    let clock = FixedClock::at(SUNDAY_AFTER_SIX);
    let github = FakeGitHub::default();
    let env = RunEnv {
        data_dir: &w.data,
        pipeline_root: &w.pipeline,
        dev_root: &w.dev,
        machine: Some("test-box"),
        clock: &clock,
        utc_offset_secs: None,
        transport: &github,
        dry_run: false,
        lock_wait: Duration::ZERO,
    };
    let mut starter = FakeStarter::default();
    let reports = run_poll_passes(&env, &[Pass::Weekly], &mut starter).unwrap();
    assert!(
        reports[0].notes[0].contains("UTC offset"),
        "{:?}",
        reports[0]
    );
    assert!(starter.jobs.is_empty());
    assert_eq!(tree(&w.data), before);
}

#[test]
fn the_window_s_four_passes_come_back_in_its_order() {
    let w = demo_world("all");
    let clock = FixedClock::at(SUNDAY_AFTER_SIX);
    let github = FakeGitHub::default();
    let env = RunEnv {
        data_dir: &w.data,
        pipeline_root: &w.pipeline,
        dev_root: &w.dev,
        machine: Some("test-box"),
        clock: &clock,
        utc_offset_secs: Some(PACIFIC),
        transport: &github,
        dry_run: true,
        lock_wait: Duration::ZERO,
    };
    let mut starter = FakeStarter::default();
    let reports = run_poll_passes(&env, &Pass::AFTER_POLL, &mut starter).unwrap();
    let names: Vec<&str> = reports.iter().map(|r| r.pass).collect();
    assert_eq!(names, ["review", "roadmap", "merge", "weekly"]);
}

#[test]
fn a_second_run_is_refused_while_one_holds_the_passes() {
    let w = demo_world("lock");
    let target = passes_lock_target(&w.data);
    assert_eq!(target, w.data.join(STATE_FILE));
    assert!(!lock::held(&target));

    // Another run (the window, or a headless pass) holds the lock: nothing runs here.
    let held = FileLock::take(&target, Duration::ZERO).unwrap();
    let before = tree(&w.data);
    let mut starter = FakeStarter::default();
    let github = FakeGitHub::default();
    match run_one(&w, Pass::Review, false, &mut starter, &github) {
        Err(CoreError::Refused(why)) => {
            assert!(why.contains("another run holds the passes"), "{why}")
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(starter.asked.is_empty() && starter.jobs.is_empty());
    assert_eq!(tree(&w.data), before);
    assert_eq!(
        run::status(&w.data, None, 0, None).unwrap()["passes_running"],
        true
    );
    // A dry run changes nothing, so it needs no lock.
    assert!(run_one(&w, Pass::Review, true, &mut starter, &github).is_ok());
    drop(held);

    // A real pass holds the lock while it works and gives it back.
    let mut starter = FakeStarter {
        lock_target: Some(target.clone()),
        ..Default::default()
    };
    let report = run_one(&w, Pass::Review, false, &mut starter, &github).unwrap();
    assert_eq!(report.started.len(), 1);
    assert!(!starter.lock_seen.is_empty());
    assert!(
        starter.lock_seen.iter().all(|held| *held),
        "{:?}",
        starter.lock_seen
    );
    assert!(!lock::held(&target));
}

fn satellite_on(w: &World) {
    std::fs::write(
        w.config.join("satellite.json"),
        r#"{ "satellite_on": true, "lanes": ["gemini"] }"#,
    )
    .unwrap();
}

#[test]
fn a_satellite_tick_starts_its_lanes_and_keeps_a_second_satellite_out() {
    let w = world("satellite");
    let fleet = FleetBridge::with_root(&w.pipeline);

    // Off (no satellite.json): a note, no lock, nothing written.
    let mut session = SatelliteSession::new(fleet.clone(), &w.data);
    let off = session.tick(&w.config, Duration::ZERO);
    assert!(off.lanes_started.is_empty() && !off.failed());
    assert!(off.notes[0].contains("switch is off"), "{off:?}");
    assert!(tree(&w.data).is_empty());

    satellite_on(&w);
    // A dry run names the lane, starts no loop and writes nothing.
    let dry = satellite_dry(&fleet, &w.data, &w.config);
    assert!(dry.dry_run);
    assert_eq!(dry.lanes_started, ["gemini"]);
    assert_eq!(dry.text(), "satellite: would start gemini\n");
    assert!(tree(&w.data).is_empty());
    assert!(calls(&w, "fleet").iter().all(|args| args[0] != "run"));
    assert!(!lock::held(&satellite_lock_target(&w.data)));

    // A real tick starts it and holds the satellite.
    let tick = session.tick(&w.config, Duration::ZERO);
    assert_eq!(tick.lanes_started, ["gemini"], "{tick:?}");
    assert_eq!(tick.lines(), ["satellite: started gemini"]);
    assert!(lock::held(&satellite_lock_target(&w.data)));
    let status = run::status(&w.data, Some(&w.config), 0, None).unwrap();
    assert_eq!(status["satellite"]["live"], true);
    assert_eq!(status["satellite"]["on"], true);

    // A second satellite on this machine (the window's, or another headless run's) starts
    // nothing beside it.
    let mut second = SatelliteSession::new(fleet.clone(), &w.data);
    let refused = second.tick(&w.config, Duration::ZERO);
    assert!(refused.lanes_started.is_empty());
    assert!(refused.failed());
    assert!(
        refused.errors[0].contains("another run holds the satellite"),
        "{refused:?}"
    );

    // The session waits for its loop, reports it and gives the satellite back.
    let end = session.finish(Duration::from_millis(50));
    assert_eq!(end.loops.len(), 1, "{end:?}");
    assert_eq!(end.loops[0].lane, "gemini");
    assert_eq!(end.loops[0].exit_code, Some(0));
    assert!(!end.failed(), "{:?}", end.errors);
    assert!(!lock::held(&satellite_lock_target(&w.data)));
    assert!(calls(&w, "fleet")
        .iter()
        .any(|args| args[..3] == ["run", "--next", "gemini"]));
    let again = second.tick(&w.config, Duration::ZERO);
    assert_eq!(again.lanes_started, ["gemini"], "{again:?}");
    second.finish(Duration::from_millis(50));
}

#[test]
fn the_status_read_has_the_state_the_jobs_and_the_satellite() {
    let w = demo_world("status");
    let before = tree(&w.data);
    let status = run::status(&w.data, Some(&w.config), SUNDAY_AFTER_SIX, Some(PACIFIC)).unwrap();
    assert_eq!(
        status,
        json!({
            "state": {
                "reviews_started": [],
                "roadmaps_seen": ["iannitram/demo"],
                "weekly_last_run": SUNDAY_AFTER_SIX - 7 * 86_400,
                "drift_pending": [],
                "pm_running": null,
            },
            "weekly_due": true,
            "passes_running": false,
            "running_jobs": [],
            "satellite": {
                "on": false,
                "lanes": ["qwen", "gemini", "chatgpt", "deepseek", "claudeapi"],
                "live": false,
                "running": [],
                "held": [],
                "as_of": null,
            },
        })
    );
    let text = run::status_text(&status);
    assert!(text.contains("weekly due: yes\n"), "{text}");
    assert!(text.contains("roadmaps seen: iannitram/demo\n"), "{text}");
    assert!(text.contains("satellite switch: off\n"), "{text}");
    assert_eq!(tree(&w.data), before, "a status read writes nothing");
}

#[test]
fn a_pass_by_hand_needs_its_role_and_is_refused_in_a_job_worktree() {
    let caller = |face: Face, role: Option<Role>, in_job_worktree: bool| Caller {
        face,
        role,
        in_job_worktree,
        confirmed_in_app: false,
    };
    let pm = Role::ProjectManager;
    let ask = |c: &Caller, needs: &Role| guard_autopilot(c, "autopilot merge", needs);
    assert_eq!(ask(&caller(Face::Gui, None, false), &pm), Verdict::Allow);
    for face in [Face::Cli, Face::Mcp] {
        assert_eq!(
            ask(&caller(face, Some(pm.clone()), false), &pm),
            Verdict::Allow
        );
        assert_eq!(
            ask(
                &caller(face, Some(Role::Satellite), false),
                &Role::Satellite
            ),
            Verdict::Allow
        );
        for wrong in [None, Some(Role::Satellite), Some(Role::BudgetManager)] {
            assert_eq!(
                ask(&caller(face, wrong, false), &pm),
                Verdict::Refuse(
                    "autopilot merge: only the project-manager role may run this pass".into()
                )
            );
        }
        assert_eq!(
            ask(&caller(face, Some(pm.clone()), true), &pm),
            Verdict::Refuse("autopilot merge: refused inside a job worktree".into())
        );
    }
}
