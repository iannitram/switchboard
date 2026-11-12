//! #343 (SKY-4, SKY-8): the jobs in flight come from the fleet ledger, so the sky and the star
//! card name the same job.

use serde_json::{json, Value};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::{Issue, RepoSnapshot};
use switchboard_core::sky::jobs::in_flight;
use switchboard_core::sky::model::{load_with_ledger, StarJob};

fn repos() -> Vec<String> {
    vec!["owner/alpha".to_string(), "owner/beta".to_string()]
}

fn job(repo: &str, number: u64, job: &str) -> StarJob {
    StarJob {
        repo: repo.to_string(),
        number,
        job: job.to_string(),
    }
}

#[test]
fn a_start_with_no_end_is_in_flight_with_the_star_card_text() {
    let events = [
        json!({"event":"start","id":"owner/alpha#3","lane":"qwen","model":"q3","machine":"m1","attempt":2}),
    ];
    assert_eq!(
        in_flight(&events, &repos()),
        vec![job("owner/alpha", 3, "qwen q3 on m1, attempt 2")]
    );
}

#[test]
fn a_bare_repo_name_is_the_snapshot_repo_it_ends() {
    let events = [json!({"event":"start","id":"beta#5","lane":"claude"})];
    assert_eq!(
        in_flight(&events, &repos()),
        vec![job("owner/beta", 5, "claude")]
    );
}

#[test]
fn an_end_from_the_same_machine_or_naming_none_ends_the_job() {
    let events = [
        json!({"event":"start","id":"alpha#1","lane":"qwen","machine":"m1"}),
        json!({"event":"end","id":"owner/alpha#1","machine":"m1"}),
        json!({"event":"start","id":"alpha#2","lane":"qwen","machine":"m1"}),
        json!({"event":"end","id":"alpha#2"}),
    ];
    assert!(in_flight(&events, &repos()).is_empty());
}

#[test]
fn an_end_from_another_machine_leaves_the_job_in_flight() {
    let events = [
        json!({"event":"start","id":"alpha#1","lane":"qwen","machine":"m1"}),
        json!({"event":"end","id":"alpha#1","machine":"m2"}),
    ];
    assert_eq!(
        in_flight(&events, &repos()),
        vec![job("owner/alpha", 1, "qwen on m1")]
    );
}

#[test]
fn a_restart_names_the_last_start_and_no_lane_or_model_is_a_job() {
    let events = [
        json!({"event":"start","id":"alpha#1","lane":"qwen","attempt":1}),
        json!({"event":"start","id":"alpha#1","attempt":2}),
        json!({"event":"rating","id":"alpha#4"}),
        json!({"id":"not-a-star"}),
    ];
    assert_eq!(
        in_flight(&events, &repos()),
        vec![job("owner/alpha", 1, "a job, attempt 2")]
    );
}

#[test]
fn the_sky_loaded_with_the_ledger_shows_the_star_in_flight() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let data = std::env::temp_dir().join(format!("sb-sky-jobs-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&data).unwrap();
    SnapshotStore::new(&data)
        .save(&RepoSnapshot {
            repo: "owner/alpha".to_string(),
            issues: vec![Issue {
                number: 3,
                title: "Three".to_string(),
                state: "open".to_string(),
                labels: vec!["feature:core".to_string()],
                ..Default::default()
            }],
            roadmap_md: Some("features:\n- core: The core (release: 1.0)\n".to_string()),
            ..Default::default()
        })
        .unwrap();
    let events = [
        json!({"event":"start","id":"owner/alpha#3","lane":"qwen","model":"q3","machine":"m1","attempt":2}),
    ];
    let sky =
        serde_json::to_value(load_with_ledger(&data, &events, 1_800_000_000).unwrap()).unwrap();
    let star: &Value = &sky["constellations"][0]["features"][0]["stars"][0];
    assert_eq!(star["number"], json!(3));
    assert_eq!(star["inFlight"], json!(true));
    assert_eq!(star["job"], json!("qwen q3 on m1, attempt 2"));
}
