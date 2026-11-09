//! AUTO-5: once a week, after Sunday 6am local, the app starts the drift clear (one Project
//! Manager job per repo, after AUTO-1) and the weekly backup (BAK-1). Times are built from
//! civil dates (2026-11-08 is a Sunday), the offset is fixed, and a fake starter stands in
//! for the fleet; snapshots and the state live in a temp data folder.

use serde_json::Value;
use std::path::{Path, PathBuf};
use switchboard_core::autopilot::passes::{
    drift_pass, pm_brief, pm_job, weekly_run, weekly_take, AutopilotState, JobStarter, WeeklyRun,
    DRIFT_TASK,
};
use switchboard_core::autopilot::weekly::parse_utc_offset;
use switchboard_core::clock::UnixSeconds;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::RepoSnapshot;
use switchboard_core::CoreError;

const DAY: i64 = 86_400;
const WEEK: i64 = 7 * DAY;
/// Pacific standard time, as `date +%z` prints it in November: `-0800`.
const PACIFIC: i64 = -8 * 3_600;

/// Days from 1970-01-01 to the given civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The instant of a Pacific civil date and time.
fn pacific(y: i64, m: i64, d: i64, h: i64, min: i64) -> UnixSeconds {
    days_from_civil(y, m, d) * DAY + h * 3_600 + min * 60 - PACIFIC
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Says yes while `open`, logs every question and every start.
struct FakeStarter {
    open: bool,
    asked: Vec<String>,
    jobs: Vec<(Value, String)>,
}

impl FakeStarter {
    fn new(open: bool) -> Self {
        FakeStarter {
            open,
            asked: Vec::new(),
            jobs: Vec::new(),
        }
    }
}

impl JobStarter for FakeStarter {
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError> {
        self.asked.push(lane.to_string());
        Ok(self.open)
    }
    fn start(&mut self, job: &Value, brief: &str) -> Result<(), CoreError> {
        self.jobs.push((job.clone(), brief.to_string()));
        Ok(())
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-auto5-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A data folder with the given repos' snapshots and a state whose last weekly run was the
/// Sunday before, 2026-11-01 at 6:01am Pacific.
fn data_with(name: &str, repos: &[(&str, bool)]) -> PathBuf {
    let data = temp_dir(name);
    let store = SnapshotStore::new(&data);
    for (repo, archived) in repos {
        store
            .save(&RepoSnapshot {
                repo: repo.to_string(),
                archived: *archived,
                ..Default::default()
            })
            .unwrap();
    }
    let state = AutopilotState {
        weekly_last_run: Some(pacific(2026, 11, 1, 6, 1)),
        ..Default::default()
    };
    state.save(&data).unwrap();
    data
}

#[test]
fn the_offset_is_read_from_date_z() {
    assert_eq!(parse_utc_offset("-0800\n"), Some(-28_800));
    assert_eq!(parse_utc_offset("+0530"), Some(19_800));
    assert_eq!(parse_utc_offset(" +0000 "), Some(0));
    assert_eq!(parse_utc_offset("-0930"), Some(-34_200));
    assert_eq!(parse_utc_offset("0800"), None);
    assert_eq!(parse_utc_offset("+08:00"), None);
    assert_eq!(parse_utc_offset("+0860"), None);
    assert_eq!(parse_utc_offset("PST"), None);
    assert_eq!(parse_utc_offset(""), None);
}

#[test]
fn the_drift_task_is_fixed() {
    assert_eq!(
        DRIFT_TASK,
        "This is the weekly drift clear. Compare SPEC.md, ROADMAP.md, docs/code-map.md and the open issues with the code on main, then file or adjust an issue for each place they disagree."
    );
}

#[test]
fn sunday_559_starts_nothing_601_starts_one_drift_clear_and_one_backup() {
    let data = data_with("sunday", &[("iannitram/demo", false)]);
    let mut starter = FakeStarter::new(true);

    let early = pacific(2026, 11, 8, 5, 59);
    assert_eq!(
        weekly_run(&data, early, PACIFIC, &mut starter),
        Ok(WeeklyRun::default())
    );
    assert!(starter.asked.is_empty());

    let reset = pacific(2026, 11, 8, 6, 1);
    let run = weekly_run(&data, reset, PACIFIC, &mut starter).unwrap();
    assert_eq!(run.backup, strings(&["iannitram/demo"]));
    let id = format!("pm-drift-demo-{reset}");
    assert_eq!(run.drift_started, vec![id.clone()]);
    assert_eq!(run.drift_error, None);
    assert_eq!(starter.jobs.len(), 1);
    assert_eq!(starter.jobs[0].0, pm_job("iannitram/demo", "drift", reset));
    assert_eq!(starter.jobs[0].1, pm_brief("iannitram/demo", DRIFT_TASK));
    assert_eq!(starter.jobs[0].0["id"], id.as_str());
    let state = AutopilotState::load(&data).unwrap();
    assert_eq!(state.weekly_last_run, Some(reset));
    assert!(state.drift_pending.is_empty());
}

#[test]
fn a_second_tick_the_same_week_starts_neither_and_the_next_sunday_does() {
    let data = data_with("week", &[("iannitram/demo", false)]);
    let mut starter = FakeStarter::new(true);
    let reset = pacific(2026, 11, 8, 6, 1);
    weekly_run(&data, reset, PACIFIC, &mut starter).unwrap();

    for later in [
        reset + 60,
        pacific(2026, 11, 11, 12, 0),
        pacific(2026, 11, 15, 5, 59),
    ] {
        assert_eq!(
            weekly_run(&data, later, PACIFIC, &mut starter),
            Ok(WeeklyRun::default()),
            "{later}"
        );
    }
    assert_eq!(starter.jobs.len(), 1);

    let next = pacific(2026, 11, 15, 6, 1);
    assert_eq!(next, reset + WEEK);
    let run = weekly_run(&data, next, PACIFIC, &mut starter).unwrap();
    assert_eq!(run.backup, strings(&["iannitram/demo"]));
    assert_eq!(run.drift_started, vec![format!("pm-drift-demo-{next}")]);
    assert_eq!(starter.jobs.len(), 2);
}

#[test]
fn each_repo_not_archived_gets_its_own_drift_clear_in_store_order() {
    let data = data_with(
        "repos",
        &[
            ("iannitram/beta", false),
            ("iannitram/alpha", false),
            ("iannitram/old", true),
        ],
    );
    let mut starter = FakeStarter::new(true);
    let reset = pacific(2026, 11, 8, 6, 1);
    let run = weekly_run(&data, reset, PACIFIC, &mut starter).unwrap();
    assert_eq!(run.backup, strings(&["iannitram/alpha", "iannitram/beta"]));
    assert_eq!(
        run.drift_started,
        vec![
            format!("pm-drift-alpha-{reset}"),
            format!("pm-drift-beta-{reset}")
        ]
    );
}

#[test]
fn a_braked_drift_clear_waits_but_the_backup_still_runs() {
    let data = data_with("braked", &[("iannitram/demo", false)]);
    let reset = pacific(2026, 11, 8, 6, 1);
    let mut braked = FakeStarter::new(false);
    let run = weekly_run(&data, reset, PACIFIC, &mut braked).unwrap();
    assert_eq!(run.backup, strings(&["iannitram/demo"]));
    assert!(run.drift_started.is_empty());
    assert_eq!(braked.asked, strings(&["claudeapi"]));
    assert_eq!(
        AutopilotState::load(&data).unwrap().drift_pending,
        strings(&["iannitram/demo"])
    );

    let mut open = FakeStarter::new(true);
    let later = reset + 600;
    let run = weekly_run(&data, later, PACIFIC, &mut open).unwrap();
    assert!(run.backup.is_empty(), "the backup ran once this week");
    assert_eq!(run.drift_started, vec![format!("pm-drift-demo-{later}")]);
    assert!(AutopilotState::load(&data)
        .unwrap()
        .drift_pending
        .is_empty());
}

#[test]
fn weekly_take_queues_each_repo_once() {
    let mut state = AutopilotState {
        drift_pending: strings(&["o/b"]),
        ..Default::default()
    };
    let now = pacific(2026, 11, 8, 6, 1);
    assert!(weekly_take(
        now,
        PACIFIC,
        &strings(&["o/a", "o/b"]),
        &mut state
    ));
    assert_eq!(state.drift_pending, strings(&["o/b", "o/a"]));
    assert_eq!(state.weekly_last_run, Some(now));
    assert!(!weekly_take(
        now + 1,
        PACIFIC,
        &strings(&["o/c"]),
        &mut state
    ));
    assert_eq!(state.drift_pending, strings(&["o/b", "o/a"]));
}

/// Allows every lane and fails the start of `o/b`'s job.
struct FailsOnB(Vec<String>);

impl JobStarter for FailsOnB {
    fn may_spend(&mut self, _: &str) -> Result<bool, CoreError> {
        Ok(true)
    }
    fn start(&mut self, job: &Value, _: &str) -> Result<(), CoreError> {
        if job["repo"] == "o/b" {
            return Err(CoreError::Io("jobs: disk full".into()));
        }
        self.0.push(job["repo"].as_str().unwrap().to_string());
        Ok(())
    }
}

#[test]
fn a_failed_start_keeps_that_repo_and_the_rest_pending() {
    let mut state = AutopilotState {
        drift_pending: strings(&["o/a", "o/b", "o/c"]),
        ..Default::default()
    };
    let mut starter = FailsOnB(Vec::new());
    assert_eq!(
        drift_pass(100, &mut state, &mut starter),
        Err(CoreError::Io("jobs: disk full".into()))
    );
    assert_eq!(starter.0, strings(&["o/a"]));
    assert_eq!(state.drift_pending, strings(&["o/b", "o/c"]));
}

#[test]
fn the_code_map_names_the_weekly_run_present() {
    let map = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md"),
    )
    .unwrap();
    assert!(map.contains(
        "- AUTO-5 weekly run (`weekly_run`, `drift_pass` in `passes.rs`; the drift clear and the backup after Sunday 6am, from the app's poll thread): present."
    ));
}
