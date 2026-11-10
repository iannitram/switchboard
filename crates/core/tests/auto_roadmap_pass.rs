//! AUTO-2: a ROADMAP.md change seen by a poll starts one Project Manager job for that repo,
//! after AUTO-1; a braked pool starts none and the change is tried again at the next poll. A
//! fake starter stands in for the fleet.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use switchboard_core::autopilot::passes::{
    pm_brief, pm_job, roadmap_pass, AutopilotState, JobStarter, PM_LANES, ROADMAP_TASK,
};
use switchboard_core::autopilot::{after_poll, AfterPoll};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::RepoSnapshot;
use switchboard_core::CoreError;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Says yes while `open`, logs every question and every start.
struct FakeStarter {
    open: bool,
    /// The only lane that may spend; any lane when `None` and `open`.
    only: Option<&'static str>,
    /// What `finished` answers.
    done: bool,
    asked: Vec<String>,
    jobs: Vec<(Value, String)>,
}

impl FakeStarter {
    fn new(open: bool) -> Self {
        FakeStarter {
            open,
            only: None,
            done: true,
            asked: Vec::new(),
            jobs: Vec::new(),
        }
    }
}

impl JobStarter for FakeStarter {
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError> {
        self.asked.push(lane.to_string());
        Ok(self.open && self.only.map_or(true, |only| only == lane))
    }
    fn start(&mut self, job: &Value, brief: &str) -> Result<(), CoreError> {
        self.jobs.push((job.clone(), brief.to_string()));
        Ok(())
    }
    fn finished(&mut self, _: &str) -> Result<bool, CoreError> {
        Ok(self.done)
    }
}

fn snap(repo: &str, read_at: i64, roadmap: Option<&str>) -> RepoSnapshot {
    RepoSnapshot {
        repo: repo.to_string(),
        read_at,
        roadmap_md: roadmap.map(str::to_string),
        ..Default::default()
    }
}

/// A state that last acted on `text` for `repo`.
fn seen(repo: &str, text: Option<&str>) -> AutopilotState {
    let mut state = AutopilotState::default();
    state
        .roadmaps
        .push((repo.to_string(), text.map(str::to_string)));
    state
}

#[test]
fn the_pm_job_is_a_judgement_planning_job_on_the_pm_lane() {
    assert_eq!(
        PM_LANES,
        ["qwen", "gemini", "chatgpt", "deepseek", "claudeapi"]
    );
    assert_eq!(
        pm_job("iannitram/demo", "roadmap", 1_000, "qwen"),
        json!({
            "id": "pm-roadmap-demo-1000",
            "title": "Project Manager: roadmap for iannitram/demo",
            "kind": "judgement-planning",
            "size": "M",
            "repo": "iannitram/demo",
            "base": "main",
            "est_minutes": 30,
            "no_issue": "Project Manager job (roadmap)",
            "verify": [],
            "files_allowed": [],
            "lane": "qwen",
            "brief": "open"
        })
    );
}

#[test]
fn the_pm_brief_says_who_what_and_how_it_files() {
    assert_eq!(
        pm_brief("iannitram/demo", "Do the thing."),
        "You are the Project Manager for iannitram/demo.\n\
         Do the thing.\n\
         File every new issue, label or comment through `fleet ghq add-batch` (never `gh issue create`), sized and labelled by RUBRIC.md, written in the owner's first person.\n"
    );
    assert_eq!(
        ROADMAP_TASK,
        "ROADMAP.md changed since the last poll. Read it, SPEC.md and the open issues, then file or adjust the issues the change needs."
    );
}

#[test]
fn a_repo_seen_for_the_first_time_is_recorded_and_starts_nothing() {
    let mut state = AutopilotState::default();
    let mut starter = FakeStarter::new(true);
    let started = roadmap_pass(
        &[snap("iannitram/demo", 1_000, Some("v1"))],
        &mut state,
        &mut starter,
    )
    .unwrap();
    assert!(started.is_empty());
    assert!(starter.asked.is_empty());
    assert_eq!(
        state.roadmaps,
        vec![("iannitram/demo".to_string(), Some("v1".to_string()))]
    );
}

#[test]
fn a_changed_roadmap_starts_exactly_one_pm_job_for_that_repo() {
    let mut state = seen("iannitram/demo", Some("v1"));
    let mut starter = FakeStarter::new(true);
    let snaps = [snap("iannitram/demo", 1_000, Some("v2"))];
    let started = roadmap_pass(&snaps, &mut state, &mut starter).unwrap();
    assert_eq!(started, strings(&["pm-roadmap-demo-1000"]));
    assert_eq!(starter.asked, strings(&["qwen"]));
    assert_eq!(starter.jobs.len(), 1);
    assert_eq!(
        starter.jobs[0].0,
        pm_job("iannitram/demo", "roadmap", 1_000, "qwen")
    );
    assert_eq!(state.pm_running.as_deref(), Some("pm-roadmap-demo-1000"));
    assert_eq!(starter.jobs[0].1, pm_brief("iannitram/demo", ROADMAP_TASK));
    assert_eq!(
        state.roadmaps,
        vec![("iannitram/demo".to_string(), Some("v2".to_string()))]
    );

    let mut again = FakeStarter::new(true);
    let started = roadmap_pass(&snaps, &mut state, &mut again).unwrap();
    assert!(started.is_empty(), "the same text on the next poll");
    assert!(again.asked.is_empty());
}

#[test]
fn an_unchanged_or_removed_roadmap_starts_none() {
    for text in [Some("v1"), None] {
        let mut state = seen("iannitram/demo", Some("v1"));
        let mut starter = FakeStarter::new(true);
        let started = roadmap_pass(
            &[snap("iannitram/demo", 1_000, text)],
            &mut state,
            &mut starter,
        )
        .unwrap();
        assert!(started.is_empty(), "{text:?}");
        assert!(starter.asked.is_empty(), "{text:?}");
        assert_eq!(state.roadmaps, seen("iannitram/demo", Some("v1")).roadmaps);
    }
}

#[test]
fn a_braked_pool_starts_none_and_the_change_is_tried_at_the_next_poll() {
    let mut state = seen("iannitram/demo", Some("v1"));
    let snaps = [snap("iannitram/demo", 1_000, Some("v2"))];
    let mut braked = FakeStarter::new(false);
    let started = roadmap_pass(&snaps, &mut state, &mut braked).unwrap();
    assert!(started.is_empty());
    assert!(braked.jobs.is_empty());
    assert_eq!(
        braked.asked,
        strings(&["qwen", "gemini", "chatgpt", "deepseek", "claudeapi"])
    );
    assert_eq!(state.roadmaps, seen("iannitram/demo", Some("v1")).roadmaps);

    let later = [snap("iannitram/demo", 1_600, Some("v2"))];
    let mut open = FakeStarter::new(true);
    let started = roadmap_pass(&later, &mut state, &mut open).unwrap();
    assert_eq!(started, strings(&["pm-roadmap-demo-1600"]));
}

#[test]
fn a_closed_local_lane_falls_back_to_the_smallest_open_lane() {
    let mut state = seen("iannitram/demo", Some("v1"));
    let snaps = [snap("iannitram/demo", 1_000, Some("v2"))];
    let mut starter = FakeStarter::new(true);
    starter.only = Some("gemini");
    let started = roadmap_pass(&snaps, &mut state, &mut starter).unwrap();
    assert_eq!(started, strings(&["pm-roadmap-demo-1000"]));
    assert_eq!(starter.asked, strings(&["qwen", "gemini"]));
    assert_eq!(starter.jobs[0].0["lane"], "gemini");
}

#[test]
fn a_running_pm_job_blocks_the_next_start_and_the_change_is_kept() {
    let mut state = seen("iannitram/demo", Some("v1"));
    state.pm_running = Some("pm-drift-x-1".into());
    let snaps = [snap("iannitram/demo", 1_000, Some("v2"))];
    let mut starter = FakeStarter::new(true);
    starter.done = false;
    let started = roadmap_pass(&snaps, &mut state, &mut starter).unwrap();
    assert!(started.is_empty());
    assert!(starter.asked.is_empty());
    assert_eq!(state.roadmaps, seen("iannitram/demo", Some("v1")).roadmaps);

    starter.done = true;
    let started = roadmap_pass(&snaps, &mut state, &mut starter).unwrap();
    assert_eq!(started, strings(&["pm-roadmap-demo-1000"]));
}

#[test]
fn two_changed_repos_start_one_job_per_call() {
    let mut state = seen("iannitram/alpha", Some("a1"));
    state
        .roadmaps
        .push(("iannitram/beta".to_string(), Some("b1".to_string())));
    let snaps = [
        snap("iannitram/alpha", 1_000, Some("a2")),
        snap("iannitram/beta", 1_000, Some("b2")),
    ];
    let mut starter = FakeStarter::new(true);
    let started = roadmap_pass(&snaps, &mut state, &mut starter).unwrap();
    assert_eq!(started, strings(&["pm-roadmap-alpha-1000"]));
    assert_eq!(state.roadmaps[1].1.as_deref(), Some("b1"), "beta is kept");
    let started = roadmap_pass(&snaps, &mut state, &mut starter).unwrap();
    assert_eq!(started, strings(&["pm-roadmap-beta-1000"]));
}

#[test]
fn only_the_changed_repo_starts_and_archived_repos_are_skipped() {
    let mut state = seen("iannitram/alpha", Some("a1"));
    state
        .roadmaps
        .push(("iannitram/beta".to_string(), Some("b1".to_string())));
    let mut archived = snap("iannitram/gamma", 1_000, Some("g2"));
    archived.archived = true;
    let snaps = [
        snap("iannitram/alpha", 1_000, Some("a1")),
        snap("iannitram/beta", 1_000, Some("b2")),
        archived,
    ];
    let mut starter = FakeStarter::new(true);
    let started = roadmap_pass(&snaps, &mut state, &mut starter).unwrap();
    assert_eq!(started, strings(&["pm-roadmap-beta-1000"]));
    assert_eq!(state.roadmaps.len(), 2, "an archived repo is not recorded");
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-auto2-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn after_poll_runs_the_roadmap_pass_and_saves_what_it_acted_on() {
    let data = temp_dir("after-poll");
    SnapshotStore::new(&data)
        .save(&snap("iannitram/demo", 1_000, Some("v2")))
        .unwrap();
    seen("iannitram/demo", Some("v1")).save(&data).unwrap();
    let mut starter = FakeStarter::new(true);
    let started = after_poll(&mut AfterPoll {
        data_dir: &data,
        machine: None,
        starter: &mut starter,
    });
    assert_eq!(started, Ok(strings(&["pm-roadmap-demo-1000"])));
    assert_eq!(
        AutopilotState::load(&data).unwrap().roadmaps,
        vec![("iannitram/demo".to_string(), Some("v2".to_string()))]
    );
}

#[test]
fn the_code_map_names_the_roadmap_pass_present() {
    let map = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md"),
    )
    .unwrap();
    assert!(map.contains(
        "- AUTO-2 roadmap pass (`roadmap_pass`, `pm_job` in `passes.rs`; run by `after_poll`): present."
    ));
}
