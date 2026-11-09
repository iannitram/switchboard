//! Locked tests for SKY-W (the `sky` command: the SkyModel the Sky view paints). Written before
//! the build (R28); the builder never edits this file.

use serde_json::{json, Value};
use std::path::PathBuf;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::{Issue, PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::sky::layout::place;
use switchboard_core::sky::model::{build, load, StarJob, PRIORITY_FILE};
use switchboard_core::sky::priority::{parse, PriorityCopy};
use switchboard_core::CoreError;

/// 2026-11-08T12:00:00Z
const NOW: i64 = 1_794_139_200;
/// 2026-11-08T00:00:00Z
const MIDNIGHT: i64 = 1_794_096_000;

fn issue(number: u64, title: &str, state: &str, labels: &[&str], body: &str) -> Issue {
    Issue {
        number,
        title: title.to_string(),
        state: state.to_string(),
        labels: labels.iter().map(|s| s.to_string()).collect(),
        milestone: Some("1.0".to_string()),
        body: body.to_string(),
        closed_by_pr: None,
    }
}

fn verdict(v: &str) -> ReviewVerdict {
    ReviewVerdict {
        verdict: v.to_string(),
        sha: "abc".to_string(),
        needs_me: false,
    }
}

fn pull(number: u64, state: &str, closes: &[u64], verdicts: &[&str]) -> PullRequest {
    PullRequest {
        number,
        title: format!("PR {number}"),
        state: state.to_string(),
        closes: closes.to_vec(),
        verdicts: verdicts.iter().map(|v| verdict(v)).collect(),
        ..Default::default()
    }
}

fn alpha() -> RepoSnapshot {
    let mut stray = issue(4, "Stray", "open", &[], "");
    stray.milestone = None;
    RepoSnapshot {
        repo: "iannitram/alpha".to_string(),
        read_at: NOW - 3_600,
        issues: vec![
            issue(1, "Core one", "closed", &["feature:core", "size:easy"], ""),
            issue(
                2,
                "Core two",
                "open",
                &["feature:core", "size:medium"],
                "Blocked by #1",
            ),
            issue(
                3,
                "Sky one",
                "open",
                &["feature:sky"],
                "## Blocked by\nBlocked by #2",
            ),
            stray,
        ],
        pulls: vec![
            pull(10, "open", &[2], &["FAIL", "PASS"]),
            pull(11, "merged", &[1], &[]),
        ],
        roadmap_md: Some(
            "# Roadmap\n\nUpdated: 2026-11-01\n\nfeatures:\n\
             - core: The core (release: 1.0)\n\
             - sky: The sky (release: 1.0)\n\
             - relay: A relay (release: after-1.0)\n"
                .to_string(),
        ),
        code_map_md: Some(
            "# Code map\n\nreuses:\n- beta: the chat parts\n- iannitram/gamma: the report\n\
             - nowhere: nothing\n"
                .to_string(),
        ),
        ..Default::default()
    }
}

fn beta() -> RepoSnapshot {
    RepoSnapshot {
        repo: "iannitram/beta".to_string(),
        read_at: NOW - 600,
        archived: true,
        issues: vec![issue(
            5,
            "Two homes",
            "open",
            &["feature:chat", "feature:sky"],
            "",
        )],
        roadmap_md: Some(
            "Updated: 2026-10-20\n\nfeatures:\n- chat: Chat (release: 1.0)\n- mail: Mail (release: 1.0)\n"
                .to_string(),
        ),
        ..Default::default()
    }
}

fn gamma() -> RepoSnapshot {
    RepoSnapshot {
        repo: "iannitram/gamma".to_string(),
        read_at: NOW - 7_200,
        ..Default::default()
    }
}

fn copy() -> PriorityCopy {
    PriorityCopy {
        updated_at: NOW - 3 * 86_400,
        order: vec![
            "iannitram/gamma".to_string(),
            "iannitram/missing".to_string(),
            "iannitram/alpha".to_string(),
        ],
    }
}

fn jobs() -> Vec<StarJob> {
    vec![
        StarJob {
            repo: "iannitram/alpha".to_string(),
            number: 3,
            job: "job-42".to_string(),
        },
        StarJob {
            repo: "iannitram/beta".to_string(),
            number: 2,
            job: "other".to_string(),
        },
    ]
}

/// The whole fixture sky as the face receives it.
fn sky() -> Value {
    let snaps = vec![beta(), alpha(), gamma()];
    let model = build(&snaps, Some(&copy()), &jobs(), NOW).unwrap();
    serde_json::to_value(&model).unwrap()
}

fn constellation(sky: &Value, repo: &str) -> Value {
    sky["constellations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["repo"] == repo)
        .unwrap_or_else(|| panic!("no constellation {repo}"))
        .clone()
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

fn sorted(names: &[&str]) -> Vec<String> {
    let mut k: Vec<String> = names.iter().map(|s| s.to_string()).collect();
    k.sort();
    k
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let d = std::env::temp_dir().join(format!("sb-sky-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

// --- The field names are exactly the TypeScript SkyModel's (app/src/sky/model.ts). ---

#[test]
fn every_object_has_exactly_the_face_field_names() {
    let sky = sky();
    assert_eq!(
        keys(&sky),
        sorted(&["ageMinutes", "priorityAgeDays", "constellations"])
    );
    let alpha = constellation(&sky, "iannitram/alpha");
    assert_eq!(
        keys(&alpha),
        sorted(&[
            "repo",
            "url",
            "rank",
            "radius",
            "size",
            "angle",
            "brightness",
            "features",
            "edges",
            "chain",
            "drift",
            "reuses"
        ])
    );
    let feature = &alpha["features"][0];
    assert_eq!(keys(feature), sorted(&["id", "title", "release", "stars"]));
    let star = &feature["stars"][1];
    assert_eq!(
        keys(star),
        sorted(&[
            "number",
            "title",
            "state",
            "url",
            "labels",
            "size",
            "milestone",
            "inFlight",
            "pr",
            "job"
        ])
    );
    assert_eq!(
        keys(&star["pr"]),
        sorted(&["number", "url", "state", "verdict"])
    );
    assert_eq!(keys(&alpha["drift"][0]), sorted(&["kind", "text", "link"]));
    assert_eq!(
        keys(&alpha["reuses"][0]),
        sorted(&["repo", "what", "warning"])
    );
}

// --- SkyModel ---

#[test]
fn the_age_is_the_oldest_snapshot_in_whole_minutes() {
    assert_eq!(sky()["ageMinutes"], json!(120));
}

#[test]
fn the_priority_age_is_the_copy_age_in_days_and_null_without_a_copy() {
    assert_eq!(sky()["priorityAgeDays"], json!(3));
    let model = build(&[alpha()], None, &[], NOW).unwrap();
    assert_eq!(
        serde_json::to_value(&model).unwrap()["priorityAgeDays"],
        Value::Null
    );
}

#[test]
fn constellations_follow_the_copy_then_the_rest_by_name() {
    let sky = sky();
    let repos: Vec<&str> = sky["constellations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["repo"].as_str().unwrap())
        .collect();
    assert_eq!(
        repos,
        vec!["iannitram/gamma", "iannitram/alpha", "iannitram/beta"]
    );
}

#[test]
fn without_a_copy_constellations_are_by_repo_name() {
    let model = build(&[gamma(), beta(), alpha()], None, &[], NOW).unwrap();
    let sky = serde_json::to_value(&model).unwrap();
    let repos: Vec<&str> = sky["constellations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["repo"].as_str().unwrap())
        .collect();
    assert_eq!(
        repos,
        vec!["iannitram/alpha", "iannitram/beta", "iannitram/gamma"]
    );
}

#[test]
fn rank_radius_size_and_angle_are_the_layout_placement() {
    let sky = sky();
    let order = vec![
        "iannitram/gamma".to_string(),
        "iannitram/alpha".to_string(),
        "iannitram/beta".to_string(),
    ];
    for p in place(&order) {
        let c = constellation(&sky, &p.repo);
        assert_eq!(c["rank"], json!(p.rank), "{}", p.repo);
        assert_eq!(c["radius"], json!(p.radius), "{}", p.repo);
        assert_eq!(c["size"], json!(p.size), "{}", p.repo);
        assert_eq!(c["angle"], json!(p.angle), "{}", p.repo);
    }
}

#[test]
fn no_snapshot_is_the_no_snapshot_error() {
    let err = build(&[], Some(&copy()), &[], NOW).unwrap_err();
    assert_eq!(err, CoreError::GitHub("no snapshot yet".to_string()));
    assert_eq!(err.to_string(), "GitHub: no snapshot yet");
}

#[test]
fn a_bad_roadmap_is_a_parse_error_naming_the_repo() {
    let bad = RepoSnapshot {
        repo: "iannitram/bad".to_string(),
        read_at: NOW,
        roadmap_md: Some("features:\n- Bad Id: x\n".to_string()),
        ..Default::default()
    };
    let err = build(&[alpha(), bad], None, &[], NOW).unwrap_err();
    assert_eq!(
        err,
        CoreError::Parse("iannitram/bad: ROADMAP.md line 2: - Bad Id: x".to_string())
    );
}

#[test]
fn the_inputs_are_not_changed() {
    let snaps = vec![beta(), alpha(), gamma()];
    let before = snaps.clone();
    let c = copy();
    let j = jobs();
    let j_before = j.clone();
    build(&snaps, Some(&c), &j, NOW).unwrap();
    assert_eq!(snaps, before);
    assert_eq!(c, copy());
    assert_eq!(j, j_before);
}

// --- Constellation ---

#[test]
fn a_constellation_links_its_repo() {
    assert_eq!(
        constellation(&sky(), "iannitram/alpha")["url"],
        json!("https://github.com/iannitram/alpha")
    );
}

#[test]
fn brightness_is_the_layout_brightness() {
    let sky = sky();
    // alpha: #1 closed of #1, #2, #3 in the 1.0 milestone; no 1.0 feature is dark.
    assert_eq!(
        constellation(&sky, "iannitram/alpha")["brightness"],
        json!(1.0 / 3.0)
    );
    // beta: #5 open, and `mail` is a dark 1.0 feature: 0 of 2.
    assert_eq!(
        constellation(&sky, "iannitram/beta")["brightness"],
        json!(0.0)
    );
    // gamma: nothing at all.
    assert_eq!(
        constellation(&sky, "iannitram/gamma")["brightness"],
        json!(0.0)
    );
}

#[test]
fn features_are_the_roadmap_block_in_order_with_their_stars() {
    let alpha = constellation(&sky(), "iannitram/alpha");
    let features = alpha["features"].as_array().unwrap();
    let summary: Vec<(String, String, String, Vec<u64>)> = features
        .iter()
        .map(|f| {
            (
                f["id"].as_str().unwrap().to_string(),
                f["title"].as_str().unwrap().to_string(),
                f["release"].as_str().unwrap().to_string(),
                f["stars"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s["number"].as_u64().unwrap())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("core".into(), "The core".into(), "1.0".into(), vec![1, 2]),
            ("sky".into(), "The sky".into(), "1.0".into(), vec![3]),
            ("relay".into(), "A relay".into(), "after-1.0".into(), vec![]),
        ]
    );
}

#[test]
fn a_star_on_two_features_sits_in_each_cluster_its_roadmap_has() {
    let beta = constellation(&sky(), "iannitram/beta");
    assert_eq!(beta["features"][0]["id"], json!("chat"));
    assert_eq!(beta["features"][0]["stars"][0]["number"], json!(5));
    assert_eq!(beta["features"][1]["id"], json!("mail"));
    assert_eq!(beta["features"][1]["stars"], json!([]));
}

#[test]
fn no_roadmap_is_no_features() {
    assert_eq!(
        constellation(&sky(), "iannitram/gamma")["features"],
        json!([])
    );
}

#[test]
fn edges_are_blocker_then_blocked() {
    assert_eq!(
        constellation(&sky(), "iannitram/alpha")["edges"],
        json!([[1, 2], [2, 3]])
    );
}

#[test]
fn chain_is_the_longest_open_chain() {
    let sky = sky();
    assert_eq!(
        constellation(&sky, "iannitram/alpha")["chain"],
        json!([2, 3])
    );
    assert_eq!(constellation(&sky, "iannitram/beta")["chain"], json!([]));
}

#[test]
fn reuses_warn_only_for_an_archived_repo_matched_by_full_or_short_name() {
    assert_eq!(
        constellation(&sky(), "iannitram/alpha")["reuses"],
        json!([
            { "repo": "beta", "what": "the chat parts", "warning": true },
            { "repo": "iannitram/gamma", "what": "the report", "warning": false },
            { "repo": "nowhere", "what": "nothing", "warning": false }
        ])
    );
    assert_eq!(
        constellation(&sky(), "iannitram/gamma")["reuses"],
        json!([])
    );
}

// --- Star ---

#[test]
fn a_star_carries_its_issue_size_pr_and_review() {
    let alpha = constellation(&sky(), "iannitram/alpha");
    assert_eq!(
        alpha["features"][0]["stars"][1],
        json!({
            "number": 2,
            "title": "Core two",
            "state": "open",
            "url": "https://github.com/iannitram/alpha/issues/2",
            "labels": ["feature:core", "size:medium"],
            "size": "medium",
            "milestone": "1.0",
            "inFlight": false,
            "pr": {
                "number": 10,
                "url": "https://github.com/iannitram/alpha/pull/10",
                "state": "open",
                "verdict": "PASS"
            },
            "job": null
        })
    );
}

#[test]
fn a_pr_with_no_verdict_has_a_null_verdict() {
    let alpha = constellation(&sky(), "iannitram/alpha");
    let one = &alpha["features"][0]["stars"][0];
    assert_eq!(one["state"], json!("closed"));
    assert_eq!(one["size"], json!("easy"));
    assert_eq!(
        one["pr"],
        json!({
            "number": 11,
            "url": "https://github.com/iannitram/alpha/pull/11",
            "state": "merged",
            "verdict": null
        })
    );
}

#[test]
fn a_star_with_a_job_in_flight_pulses_and_names_it() {
    let alpha = constellation(&sky(), "iannitram/alpha");
    let three = &alpha["features"][1]["stars"][0];
    assert_eq!(three["number"], json!(3));
    assert_eq!(three["inFlight"], json!(true));
    assert_eq!(three["job"], json!("job-42"));
    assert_eq!(three["size"], Value::Null);
    assert_eq!(three["pr"], Value::Null);
}

#[test]
fn a_job_on_another_repo_does_not_touch_the_same_number_here() {
    let model = build(
        &[alpha()],
        None,
        &[StarJob {
            repo: "iannitram/beta".to_string(),
            number: 2,
            job: "other".to_string(),
        }],
        NOW,
    )
    .unwrap();
    let sky = serde_json::to_value(&model).unwrap();
    let two = &sky["constellations"][0]["features"][0]["stars"][1];
    assert_eq!(two["number"], json!(2));
    assert_eq!(two["inFlight"], json!(false));
    assert_eq!(two["job"], Value::Null);
}

// --- Drift ---

#[test]
fn an_orphan_on_no_feature_links_its_issue() {
    assert_eq!(
        constellation(&sky(), "iannitram/alpha")["drift"],
        json!([{
            "kind": "orphan",
            "text": "#4 Stray: on no feature",
            "link": "https://github.com/iannitram/alpha/issues/4"
        }])
    );
}

#[test]
fn an_orphan_on_two_features_a_dark_feature_and_a_stale_roadmap() {
    assert_eq!(
        constellation(&sky(), "iannitram/beta")["drift"],
        json!([
            {
                "kind": "orphan",
                "text": "#5 Two homes: on feature:chat, feature:sky",
                "link": "https://github.com/iannitram/beta/issues/5"
            },
            {
                "kind": "dark",
                "text": "mail: Mail has no issue",
                "link": "https://github.com/iannitram/beta/blob/HEAD/ROADMAP.md"
            },
            {
                "kind": "stale",
                "text": "ROADMAP.md updated 2026-10-20, 19 days ago",
                "link": "https://github.com/iannitram/beta/blob/HEAD/ROADMAP.md"
            }
        ])
    );
}

#[test]
fn no_updated_line_is_stale_drift() {
    assert_eq!(
        constellation(&sky(), "iannitram/gamma")["drift"],
        json!([{
            "kind": "stale",
            "text": "ROADMAP.md has no Updated: line",
            "link": "https://github.com/iannitram/gamma/blob/HEAD/ROADMAP.md"
        }])
    );
}

fn drift_with(updated: &str, now: i64) -> Value {
    let snap = RepoSnapshot {
        repo: "iannitram/delta".to_string(),
        read_at: now,
        roadmap_md: Some(format!("Updated: {updated}\n")),
        ..Default::default()
    };
    let model = build(&[snap], None, &[], now).unwrap();
    serde_json::to_value(&model).unwrap()["constellations"][0]["drift"].clone()
}

#[test]
fn an_updated_line_that_is_not_a_date_is_stale_drift() {
    assert_eq!(
        drift_with("soon", NOW),
        json!([{
            "kind": "stale",
            "text": "ROADMAP.md Updated: soon is not a date",
            "link": "https://github.com/iannitram/delta/blob/HEAD/ROADMAP.md"
        }])
    );
}

#[test]
fn today_is_the_utc_date_of_now() {
    // 2026-11-08T00:00:00Z: 2026-10-25 is 14 days old (fresh), 2026-10-24 is 15 (stale).
    assert_eq!(drift_with("2026-10-25", MIDNIGHT), json!([]));
    assert_eq!(
        drift_with("2026-10-24", MIDNIGHT)[0]["text"],
        json!("ROADMAP.md updated 2026-10-24, 15 days ago")
    );
    // One second earlier it is still 2026-11-07, so 2026-10-24 is 14 days old.
    assert_eq!(drift_with("2026-10-24", MIDNIGHT - 1), json!([]));
}

// --- load: the snapshot store and the priority copy in the app data folder ---

#[test]
fn load_with_no_snapshot_is_the_no_snapshot_error() {
    let dir = temp_dir("empty");
    assert_eq!(
        load(&dir, NOW).unwrap_err(),
        CoreError::GitHub("no snapshot yet".to_string())
    );
}

#[test]
fn load_reads_the_store_and_the_priority_copy() {
    let dir = temp_dir("full");
    let store = SnapshotStore::new(&dir);
    store.save(&alpha()).unwrap();
    store.save(&gamma()).unwrap();
    assert_eq!(PRIORITY_FILE, "priority.json");
    let text = r#"{"updated_at": "2026-11-05T12:00:00Z", "order": ["iannitram/gamma"]}"#;
    std::fs::write(dir.join(PRIORITY_FILE), text).unwrap();
    let loaded = serde_json::to_value(load(&dir, NOW).unwrap()).unwrap();
    let built = build(&[alpha(), gamma()], Some(&parse(text).unwrap()), &[], NOW).unwrap();
    assert_eq!(loaded, serde_json::to_value(built).unwrap());
    assert_eq!(loaded["priorityAgeDays"], json!(3));
    assert_eq!(
        loaded["constellations"][0]["repo"],
        json!("iannitram/gamma")
    );
}

#[test]
fn load_without_a_priority_copy_has_no_priority_age() {
    let dir = temp_dir("nocopy");
    SnapshotStore::new(&dir).save(&alpha()).unwrap();
    let loaded = serde_json::to_value(load(&dir, NOW).unwrap()).unwrap();
    assert_eq!(loaded["priorityAgeDays"], Value::Null);
}

#[test]
fn load_with_a_bad_priority_copy_is_its_parse_error() {
    let dir = temp_dir("badcopy");
    SnapshotStore::new(&dir).save(&alpha()).unwrap();
    std::fs::write(dir.join(PRIORITY_FILE), "{ not json").unwrap();
    match load(&dir, NOW).unwrap_err() {
        CoreError::Parse(msg) => assert!(msg.starts_with("priority copy: "), "{msg}"),
        other => panic!("got {other:?}"),
    }
}

// --- The shell registers the command; the code map says so ---

#[test]
fn the_shell_registers_the_sky_command() {
    let main = include_str!("../../../app/src-tauri/src/main.rs");
    let handler = main
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("generate_handler! in main.rs");
    assert!(handler.contains("commands::sky,"), "{handler}");

    let commands = include_str!("../../../app/src-tauri/src/commands.rs");
    assert!(
        commands.contains("pub fn sky("),
        "commands.rs has no `pub fn sky(`"
    );
    assert!(
        commands.contains("FaceError::new(\"sky\""),
        "commands.rs: the sky command's errors must name `sky`"
    );
}

#[test]
fn the_code_map_lists_sky_model_present() {
    let map = include_str!("../../../docs/code-map.md");
    let line = map
        .lines()
        .skip_while(|l| *l != "### sky-model")
        .nth(1)
        .expect("a `### sky-model` section in docs/code-map.md");
    assert!(line.contains("crates/core/src/sky/model.rs"), "{line}");
    assert!(line.contains("· present."), "{line}");
}
