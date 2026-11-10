// Tests for the fleet ledger the `sky` command reads (SKY-4, #343). `commands.rs` takes this
// file in with `#[cfg(test)] #[path = "sky_ledger_tests.rs"] mod sky_ledger_tests;`.
use super::*;
use serde_json::json;

/// A fresh folder for one test, under the system temp folder.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "switchboard-app-sky-ledger-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn no_pipeline_repo_in_the_settings_is_no_events() {
    let dir = scratch("unset");
    assert_eq!(
        ledger_events(&dir.join("settings.json"), None).unwrap(),
        Vec::<serde_json::Value>::new()
    );
}

#[test]
fn the_sky_shows_the_job_in_flight_that_the_ledger_names() {
    let dir = scratch("inflight");
    let pipeline = dir.join("pipeline");
    std::fs::create_dir_all(&pipeline).unwrap();
    let state = dir.join("fleet-state");
    std::fs::create_dir_all(state.join("ledger")).unwrap();
    std::fs::write(
        state.join("ledger").join("m1.jsonl"),
        "{\"event\":\"start\",\"id\":\"owner/alpha#3\",\"lane\":\"qwen\",\"model\":\"q3\",\"machine\":\"m1\",\"attempt\":2}\n",
    )
    .unwrap();
    let settings_path = dir.join("settings.json");
    settings::save(
        &settings_path,
        &Settings {
            pipeline_repo: Some(pipeline),
            ..Settings::default()
        },
    )
    .unwrap();

    let events = ledger_events(&settings_path, Some(state.as_os_str())).unwrap();
    assert_eq!(events.len(), 1);

    let data = dir.join("data");
    let snap = switchboard_core::github::RepoSnapshot {
        repo: "owner/alpha".to_string(),
        issues: vec![switchboard_core::github::Issue {
            number: 3,
            title: "Three".to_string(),
            state: "open".to_string(),
            labels: vec!["feature:core".to_string()],
            ..Default::default()
        }],
        roadmap_md: Some("features:\n- core: The core (release: 1.0)\n".to_string()),
        ..Default::default()
    };
    switchboard_core::github::snapshot::SnapshotStore::new(&data)
        .save(&snap)
        .unwrap();
    let model =
        switchboard_core::sky::model::load_with_ledger(&data, &events, 1_800_000_000).unwrap();
    let star = serde_json::to_value(&model).unwrap()["constellations"][0]["features"][0]["stars"]
        [0]
    .clone();
    assert_eq!(star["number"], json!(3), "{star}");
    assert_eq!(star["inFlight"], json!(true), "{star}");
    assert_eq!(star["job"], json!("qwen q3 on m1, attempt 2"), "{star}");
}
