//! AUTO-3: after each poll, the core starts the one R25 review of each new PR this machine
//! opened, on another service than the builder's, after AUTO-1. A fake starter stands in for
//! the fleet; snapshots are built in the test or saved to a temp data folder.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use switchboard_core::autopilot::passes::{
    builder_lane, canon_machine, review_brief, review_fleet_job, review_pass, this_machine,
    write_job, AutopilotState, JobStarter, REVIEW_LANES, STATE_FILE,
};
use switchboard_core::autopilot::review::Rung;
use switchboard_core::autopilot::{after_poll, AfterPoll};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::{Issue, PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::CoreError;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-auto3-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Says yes to the lanes in `allow`, logs every question and every start.
struct FakeStarter {
    allow: Vec<String>,
    asked: Vec<String>,
    jobs: Vec<(Value, String)>,
}

impl FakeStarter {
    fn allowing(lanes: &[&str]) -> Self {
        FakeStarter {
            allow: strings(lanes),
            asked: Vec::new(),
            jobs: Vec::new(),
        }
    }
}

impl JobStarter for FakeStarter {
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError> {
        self.asked.push(lane.to_string());
        Ok(self.allow.iter().any(|l| l == lane))
    }
    fn start(&mut self, job: &Value, brief: &str) -> Result<(), CoreError> {
        self.jobs.push((job.clone(), brief.to_string()));
        Ok(())
    }
    fn finished(&mut self, _: &str) -> Result<bool, CoreError> {
        Ok(true)
    }
}

fn pr(number: u64, labels: &[&str], closes: &[u64]) -> PullRequest {
    PullRequest {
        draft: false,
        number,
        title: format!("PR {number}"),
        state: "open".into(),
        head_sha: format!("sha{number}"),
        head_ref: format!("sat/{number}"),
        labels: strings(labels),
        closes: closes.to_vec(),
        verdicts: vec![],
        ci_on_head: "success".into(),
    }
}

fn issue(number: u64, labels: &[&str]) -> Issue {
    Issue {
        number,
        title: format!("Issue {number}"),
        state: "open".into(),
        labels: strings(labels),
        ..Default::default()
    }
}

/// `iannitram/demo` with issue 12 (`size:easy`) and PR 7 from the Strix Halo computer, built
/// by Qwen, closing 12.
fn demo() -> RepoSnapshot {
    RepoSnapshot {
        repo: "iannitram/demo".into(),
        read_at: 1_000,
        issues: vec![issue(12, &["size:easy", "kind:wiring"])],
        pulls: vec![pr(7, &["by:strix-halo", "model:qwen3-coder"], &[12])],
        ..Default::default()
    }
}

// ---- the machine and the builder ----

#[test]
fn machine_names_are_canonical() {
    assert_eq!(canon_machine("Z13"), "strix-halo");
    assert_eq!(canon_machine(" hyperion\n"), "desktop-3090ti");
    assert_eq!(canon_machine("LegionGo"), "amd-handheld");
    assert_eq!(canon_machine("mac"), "mac-mini");
    assert_eq!(canon_machine("shuttlecraft6"), "mac-mini");
    assert_eq!(canon_machine("strix-halo"), "strix-halo");
    assert_eq!(canon_machine("Other-Box"), "other-box");
}

#[test]
fn this_machine_is_the_env_else_the_machine_file() {
    let home = temp_dir("home");
    assert_eq!(this_machine(Some("z13"), &home), Some("strix-halo".into()));
    assert_eq!(this_machine(None, &home), None);
    let dir = home.join(".config").join("software-depot");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("machine"), "  \n").unwrap();
    assert_eq!(this_machine(None, &home), None);
    std::fs::write(dir.join("machine"), "Hyperion\nsecond line\n").unwrap();
    assert_eq!(this_machine(None, &home), Some("desktop-3090ti".into()));
    assert_eq!(
        this_machine(Some("  "), &home),
        Some("desktop-3090ti".into())
    );
    assert_eq!(this_machine(Some("Z13"), &home), Some("strix-halo".into()));
}

#[test]
fn the_builder_lane_comes_from_the_model_label() {
    let lane = |labels: &[&str]| builder_lane(&strings(labels));
    assert_eq!(lane(&["by:strix-halo", "model:qwen3-coder"]), Some("qwen"));
    assert_eq!(lane(&["model:GPT-5.5"]), Some("chatgpt"));
    assert_eq!(lane(&["model:gemini-3.8-flash"]), Some("gemini"));
    assert_eq!(lane(&["model:deepseek-v4"]), Some("deepseek"));
    assert_eq!(lane(&["model:sonnet-4.6"]), Some("claudeapi"));
    assert_eq!(lane(&["model:claude-haiku"]), Some("claudeapi"));
    assert_eq!(lane(&["model:opus"]), Some("claudeapi"));
    assert_eq!(lane(&["model:haiku"]), Some("claudeapi"));
    assert_eq!(lane(&["model:fable"]), Some("claudeapi"));
    assert_eq!(lane(&["model:llama-4"]), None);
    assert_eq!(lane(&["by:strix-halo"]), None);
    assert_eq!(REVIEW_LANES, ["chatgpt", "gemini", "claudeapi"]);
}

// ---- the job ----

#[test]
fn the_review_job_is_a_fleet_job_on_the_reviewer_lane() {
    assert_eq!(
        review_fleet_job("iannitram/demo", 7, 12, Rung::Easy, "qwen", "chatgpt"),
        json!({
            "kind": "adversarial-review-pr-diff",
            "repo": "iannitram/demo",
            "pr": 7,
            "issue": 12,
            "rung": "easy",
            "try": 1,
            "not_service": "qwen",
            "brief": "exact",
            "reads": ["diff", "issue"],
            "id": "review-demo-7",
            "title": "Review iannitram/demo#7 (closes #12)",
            "size": "S",
            "base": "main",
            "est_minutes": 20,
            "verify": [],
            "files_allowed": [],
            "lane": "chatgpt"
        })
    );
    let hard = review_fleet_job("o/my repo", 3, 4, Rung::Hard, "", "gemini");
    assert_eq!(hard["id"], "review-my-repo-3");
    assert_eq!(hard["size"], "L");
    assert_eq!(
        review_fleet_job("o/r", 3, 4, Rung::Medium, "", "gemini")["size"],
        "M"
    );
}

#[test]
fn the_review_brief_names_the_pr_the_head_and_the_verdict_line() {
    assert_eq!(
        review_brief("iannitram/demo", 7, 12, "sha7"),
        "Review PR #7 in iannitram/demo at head sha7 against issue #12.\n\
         Read the PR's diff and the issue's body. Check every Expectation and every Done when line, and that the PR changed no locked test.\n\
         Post one comment on the PR. Its last line is exactly `VERDICT: PASS sha7` or `VERDICT: FAIL sha7`.\n"
    );
}

// ---- the pass ----

#[test]
fn a_new_pr_from_this_machine_starts_one_review_on_another_service() {
    let mut state = AutopilotState::default();
    let mut starter = FakeStarter::allowing(&["chatgpt", "gemini", "claudeapi"]);
    let started = review_pass(&[demo()], "strix-halo", &mut state, &mut starter).unwrap();
    assert_eq!(started, strings(&["review-demo-7"]));
    assert_eq!(starter.asked, strings(&["chatgpt"]));
    assert_eq!(starter.jobs.len(), 1);
    let (job, brief) = &starter.jobs[0];
    assert_eq!(
        *job,
        review_fleet_job("iannitram/demo", 7, 12, Rung::Easy, "qwen", "chatgpt")
    );
    assert_eq!(*brief, review_brief("iannitram/demo", 7, 12, "sha7"));
    assert_eq!(state.reviews_started, strings(&["iannitram/demo#7"]));
}

#[test]
fn the_same_pr_on_the_next_poll_starts_none() {
    let mut state = AutopilotState::default();
    let mut starter = FakeStarter::allowing(&["chatgpt"]);
    review_pass(&[demo()], "strix-halo", &mut state, &mut starter).unwrap();
    let mut again = FakeStarter::allowing(&["chatgpt"]);
    let started = review_pass(&[demo()], "strix-halo", &mut state, &mut again).unwrap();
    assert!(started.is_empty());
    assert!(again.asked.is_empty());
    assert!(again.jobs.is_empty());
    assert_eq!(state.reviews_started, strings(&["iannitram/demo#7"]));
}

#[test]
fn a_braked_pool_starts_none_and_the_next_poll_tries_again() {
    let mut state = AutopilotState::default();
    let mut braked = FakeStarter::allowing(&[]);
    let started = review_pass(&[demo()], "strix-halo", &mut state, &mut braked).unwrap();
    assert!(started.is_empty());
    assert!(braked.jobs.is_empty());
    assert_eq!(braked.asked, strings(&["chatgpt", "gemini", "claudeapi"]));
    assert!(state.reviews_started.is_empty());

    let mut open = FakeStarter::allowing(&["claudeapi"]);
    let started = review_pass(&[demo()], "strix-halo", &mut state, &mut open).unwrap();
    assert_eq!(started, strings(&["review-demo-7"]));
    assert_eq!(open.jobs[0].0["lane"], "claudeapi");
}

#[test]
fn the_builders_own_service_is_never_the_reviewer() {
    let mut snap = demo();
    snap.pulls[0].labels = strings(&["by:strix-halo", "model:gpt-5.5"]);
    snap.issues[0].labels = strings(&["size:medium"]);
    let mut state = AutopilotState::default();
    let mut starter = FakeStarter::allowing(&["chatgpt", "gemini"]);
    review_pass(&[snap], "strix-halo", &mut state, &mut starter).unwrap();
    assert_eq!(starter.asked, strings(&["gemini"]));
    let job = &starter.jobs[0].0;
    assert_eq!(job["lane"], "gemini");
    assert_eq!(job["not_service"], "chatgpt");
    assert_eq!(job["size"], "M");
    assert_eq!(job["rung"], "medium");
}

#[test]
fn an_issue_without_a_size_or_missing_from_the_snapshot_is_hard() {
    let mut snap = demo();
    snap.issues.clear();
    let mut state = AutopilotState::default();
    let mut starter = FakeStarter::allowing(&["chatgpt"]);
    review_pass(&[snap], "strix-halo", &mut state, &mut starter).unwrap();
    assert_eq!(starter.jobs[0].0["size"], "L");
    assert_eq!(starter.jobs[0].0["rung"], "hard");
}

#[test]
fn prs_that_do_not_qualify_start_none() {
    let mut other = demo();
    other.pulls[0].labels = strings(&["by:desktop-3090ti", "model:qwen3-coder"]);
    let mut reviewed = demo();
    reviewed.pulls[0].verdicts = vec![ReviewVerdict {
        verdict: "FAIL".into(),
        sha: "sha7".into(),
        needs_me: false,
    }];
    let mut merged = demo();
    merged.pulls[0].state = "merged".into();
    let mut no_issue = demo();
    no_issue.pulls[0].closes.clear();
    let mut archived = demo();
    archived.archived = true;
    for (why, snap) in [
        ("another machine", other),
        ("already reviewed", reviewed),
        ("not open", merged),
        ("closes nothing", no_issue),
        ("archived", archived),
    ] {
        let mut state = AutopilotState::default();
        let mut starter = FakeStarter::allowing(&["chatgpt"]);
        let started = review_pass(&[snap], "strix-halo", &mut state, &mut starter).unwrap();
        assert!(started.is_empty(), "{why}");
        assert!(starter.asked.is_empty(), "{why}");
        assert!(state.reviews_started.is_empty(), "{why}");
    }
}

#[test]
fn repos_go_in_the_order_given_and_prs_smallest_first() {
    let mut a = demo();
    a.pulls = vec![
        pr(9, &["by:strix-halo"], &[12]),
        pr(7, &["by:strix-halo"], &[12]),
    ];
    let mut b = demo();
    b.repo = "iannitram/beta".into();
    b.pulls = vec![pr(2, &["by:strix-halo"], &[12])];
    let mut state = AutopilotState::default();
    let mut starter = FakeStarter::allowing(&["chatgpt"]);
    let started = review_pass(&[a, b], "strix-halo", &mut state, &mut starter).unwrap();
    assert_eq!(
        started,
        strings(&["review-demo-7", "review-demo-9", "review-beta-2"])
    );
    assert_eq!(
        state.reviews_started,
        strings(&["iannitram/demo#7", "iannitram/demo#9", "iannitram/beta#2"])
    );
}

struct BrokenBudget;

impl JobStarter for BrokenBudget {
    fn may_spend(&mut self, lane: &str) -> Result<bool, CoreError> {
        Err(CoreError::Refused(format!("budget allow {lane}: exit 3")))
    }
    fn start(&mut self, _: &Value, _: &str) -> Result<(), CoreError> {
        panic!("nothing may start when the budget answer failed");
    }
    fn finished(&mut self, _: &str) -> Result<bool, CoreError> {
        Ok(true)
    }
}

#[test]
fn a_budget_error_stops_the_pass_and_records_nothing() {
    let mut state = AutopilotState::default();
    assert_eq!(
        review_pass(&[demo()], "strix-halo", &mut state, &mut BrokenBudget),
        Err(CoreError::Refused("budget allow chatgpt: exit 3".into()))
    );
    assert!(state.reviews_started.is_empty());
}

// ---- the state file and after_poll ----

#[test]
fn the_state_file_round_trips_and_a_missing_one_is_the_default() {
    let dir = temp_dir("state");
    assert_eq!(STATE_FILE, "autopilot.json");
    assert_eq!(AutopilotState::load(&dir), Ok(AutopilotState::default()));
    let mut state = AutopilotState::default();
    state.reviews_started.push("iannitram/demo#7".into());
    state.save(&dir).unwrap();
    assert_eq!(AutopilotState::load(&dir), Ok(state));
    std::fs::write(dir.join(STATE_FILE), "{}").unwrap();
    assert_eq!(AutopilotState::load(&dir), Ok(AutopilotState::default()));
    std::fs::write(dir.join(STATE_FILE), "[oops").unwrap();
    match AutopilotState::load(&dir) {
        Err(CoreError::Parse(why)) => assert!(why.contains("autopilot.json"), "{why}"),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

fn data_with_demo(name: &str) -> PathBuf {
    let data = temp_dir(name);
    SnapshotStore::new(&data).save(&demo()).unwrap();
    data
}

#[test]
fn after_poll_starts_the_review_once_across_polls() {
    let data = data_with_demo("after-poll");
    let mut starter = FakeStarter::allowing(&["chatgpt"]);
    let first = after_poll(&mut AfterPoll {
        data_dir: &data,
        machine: Some("strix-halo"),
        starter: &mut starter,
    });
    assert_eq!(first, Ok(strings(&["review-demo-7"])));
    assert_eq!(
        AutopilotState::load(&data).unwrap().reviews_started,
        strings(&["iannitram/demo#7"])
    );
    let second = after_poll(&mut AfterPoll {
        data_dir: &data,
        machine: Some("strix-halo"),
        starter: &mut starter,
    });
    assert_eq!(second, Ok(vec![]));
    assert_eq!(starter.jobs.len(), 1);
}

#[test]
fn after_poll_without_a_machine_name_starts_no_review() {
    let data = data_with_demo("no-machine");
    let mut starter = FakeStarter::allowing(&["chatgpt"]);
    let started = after_poll(&mut AfterPoll {
        data_dir: &data,
        machine: None,
        starter: &mut starter,
    });
    assert_eq!(started, Ok(vec![]));
    assert!(starter.asked.is_empty());
}

// ---- writing the job for the fleet ----

#[test]
fn write_job_puts_the_job_and_its_brief_side_by_side() {
    let dir = temp_dir("write-job");
    let jobs = dir.join("jobs");
    let dev = Path::new("/srv/dev");
    let job = review_fleet_job("iannitram/demo", 7, 12, Rung::Easy, "qwen", "chatgpt");
    let path = write_job(&jobs, dev, &job, "the brief\n").unwrap();
    assert_eq!(path, jobs.join("review-demo-7.json"));
    assert_eq!(
        std::fs::read_to_string(jobs.join("review-demo-7.md")).unwrap(),
        "the brief\n"
    );
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        written["prompt_file"],
        jobs.join("review-demo-7.md").to_string_lossy().as_ref()
    );
    assert_eq!(
        written["clone"],
        dev.join("demo").to_string_lossy().as_ref()
    );
    assert_eq!(written["lane"], "chatgpt");
    assert!(matches!(
        write_job(&jobs, dev, &json!({"repo": "o/r"}), ""),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn the_code_map_names_the_review_start_present() {
    let map = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/code-map.md"),
    )
    .unwrap();
    assert!(map.contains(
        "- AUTO-3 review start (`after_poll`, `review_pass` in `passes.rs`; the app's poll thread starts it): present."
    ));
}
