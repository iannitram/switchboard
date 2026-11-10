// Tests for the sky the `sky` command answers with the fleet ledger's jobs in flight (SKY-4,
// #343), through `sky_at`, the function the command calls. `commands.rs` takes this file in
// with `#[cfg(test)] #[path = "sky_ledger_tests.rs"] mod sky_ledger_tests;`.
use super::*;
use serde_json::json;

const NOW: UnixSeconds = 1_800_000_000;
const START: &str = "{\"event\":\"start\",\"id\":\"owner/alpha#3\",\"lane\":\"qwen\",\"model\":\"q3\",\"machine\":\"m1\",\"attempt\":2}\n";

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

/// The folders of one test: settings naming a pipeline repo, a fleet state folder whose
/// ledger file `m1.jsonl` holds `ledger`, and a data folder with `owner/alpha`'s open #3.
struct Setup {
    settings: PathBuf,
    state: PathBuf,
    data: PathBuf,
}

fn setup(name: &str, ledger: &[u8]) -> Setup {
    let dir = scratch(name);
    let pipeline = dir.join("pipeline");
    std::fs::create_dir_all(&pipeline).unwrap();
    let state = dir.join("fleet-state");
    std::fs::create_dir_all(state.join("ledger")).unwrap();
    std::fs::write(state.join("ledger").join("m1.jsonl"), ledger).unwrap();
    let settings_path = dir.join("settings.json");
    settings::save(
        &settings_path,
        &Settings {
            pipeline_repo: Some(pipeline),
            ..Settings::default()
        },
    )
    .unwrap();
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
    Setup {
        settings: settings_path,
        state,
        data,
    }
}

/// `sky_at` over `s`: alpha#3 as the sky draws it, and the jobs error beside it.
fn star(s: &Setup) -> (serde_json::Value, Option<CoreError>) {
    let (model, jobs_error) =
        sky_at(&s.settings, Some(s.state.as_os_str()), &s.data, NOW).expect("the sky loads");
    let sky = serde_json::to_value(&model).unwrap();
    let star = sky["constellations"][0]["features"][0]["stars"][0].clone();
    assert_eq!(star["number"], json!(3), "{star}");
    (star, jobs_error)
}

#[test]
fn the_sky_shows_the_job_in_flight_that_the_ledger_names() {
    let (star, jobs_error) = star(&setup("inflight", START.as_bytes()));
    assert_eq!(star["inFlight"], json!(true), "{star}");
    assert_eq!(star["job"], json!("qwen q3 on m1, attempt 2"), "{star}");
    assert!(jobs_error.is_none(), "{jobs_error:?}");
}

#[test]
fn no_pipeline_repo_in_the_settings_is_the_sky_with_no_job_and_no_error() {
    let s = setup("unset", START.as_bytes());
    std::fs::remove_file(&s.settings).unwrap();
    let (star, jobs_error) = star(&s);
    assert_eq!(star["inFlight"], json!(false), "{star}");
    assert!(jobs_error.is_none(), "{jobs_error:?}");
}

#[test]
fn a_ledger_that_does_not_read_still_gives_the_sky_without_jobs_and_says_why() {
    // Not UTF-8, so `read_ledger` cannot read the file.
    let (star, jobs_error) = star(&setup("badledger", &[0xff, 0xfe, 0xfd, b'\n']));
    assert_eq!(star["inFlight"], json!(false), "{star}");
    assert_eq!(star["job"], json!(null), "{star}");
    let why = jobs_error.expect("the ledger error").to_string();
    assert!(why.contains("m1.jsonl"), "{why}");
}

#[test]
fn a_settings_file_that_does_not_read_still_gives_the_sky_without_jobs_and_says_why() {
    let s = setup("badsettings", START.as_bytes());
    std::fs::write(&s.settings, "{trunc").unwrap();
    let (star, jobs_error) = star(&s);
    assert_eq!(star["inFlight"], json!(false), "{star}");
    let why = jobs_error.expect("the settings error").to_string();
    assert!(why.contains("settings.json"), "{why}");
}
