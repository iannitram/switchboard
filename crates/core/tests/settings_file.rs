//! Locked tests for issue 4 (CORE-5: settings load, save and missing names). Qwen never edits
//! this file.

use std::path::{Path, PathBuf};
use switchboard_core::settings::{load, save, Settings, SETTING_NAMES};
use switchboard_core::CoreError;

/// A fresh, empty folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "switchboard-settings-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn all_set() -> Settings {
    Settings {
        pipeline_repo: Some(PathBuf::from("/srv/pipeline")),
        nextcloud_backups: Some(PathBuf::from("/srv/nextcloud/Backups")),
        vault_agents: Some(PathBuf::from("/srv/vault/agents")),
    }
}

#[test]
fn a_missing_file_loads_as_the_default() {
    let d = scratch("missing");
    assert_eq!(load(&d.join("settings.json")), Ok(Settings::default()));
}

#[test]
fn a_file_that_does_not_parse_is_a_parse_error_naming_the_file() {
    let d = scratch("bad");
    let p = d.join("settings.json");
    std::fs::write(&p, "{not json").unwrap();
    match load(&p) {
        Err(CoreError::Parse(msg)) => assert!(msg.contains("settings.json"), "{msg}"),
        other => panic!("expected CoreError::Parse, got {other:?}"),
    }
}

#[test]
fn a_path_that_cannot_be_read_is_an_io_error() {
    let d = scratch("unreadable");
    // A folder where the file should be: reading it fails, and not with "not found".
    let p = d.join("settings.json");
    std::fs::create_dir_all(&p).unwrap();
    assert!(matches!(load(&p), Err(CoreError::Io(_))), "{:?}", load(&p));
}

#[test]
fn load_reads_the_keys_that_are_there_and_leaves_the_rest_unset() {
    let d = scratch("partial");
    let p = d.join("settings.json");
    std::fs::write(
        &p,
        r#"{"pipeline_repo": "/srv/pipeline", "vault_agents": "/srv/vault/agents"}"#,
    )
    .unwrap();
    let want = Settings {
        pipeline_repo: Some(PathBuf::from("/srv/pipeline")),
        nextcloud_backups: None,
        vault_agents: Some(PathBuf::from("/srv/vault/agents")),
    };
    assert_eq!(load(&p), Ok(want));
}

#[test]
fn load_ignores_keys_it_does_not_know() {
    let d = scratch("unknown");
    let p = d.join("settings.json");
    std::fs::write(&p, r#"{"pipeline_repo": "/srv/pipeline", "theme": "dark"}"#).unwrap();
    assert_eq!(
        load(&p).unwrap().pipeline_repo,
        Some(PathBuf::from("/srv/pipeline"))
    );
}

#[test]
fn save_creates_the_folder_and_load_reads_back_the_same_settings() {
    let d = scratch("roundtrip");
    let p = d.join("nested").join("deeper").join("settings.json");
    let s = all_set();
    save(&p, &s).unwrap();
    assert_eq!(load(&p), Ok(s.clone()));
    assert_eq!(s, all_set(), "save must not change its input");
}

#[test]
fn save_writes_pretty_json_with_the_field_names_as_keys() {
    let d = scratch("pretty");
    let p = d.join("settings.json");
    save(&p, &all_set()).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains('\n'), "pretty JSON spans lines: {text}");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v,
        serde_json::json!({
            "pipeline_repo": "/srv/pipeline",
            "nextcloud_backups": "/srv/nextcloud/Backups",
            "vault_agents": "/srv/vault/agents"
        })
    );
}

#[test]
fn save_replaces_an_old_file_and_leaves_no_temp_file() {
    let d = scratch("replace");
    let p = d.join("settings.json");
    save(&p, &all_set()).unwrap();
    save(&p, &Settings::default()).unwrap();
    assert_eq!(load(&p), Ok(Settings::default()));
    let names: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["settings.json".to_string()]);
}

#[test]
fn save_under_a_file_is_an_io_error() {
    let d = scratch("underfile");
    std::fs::write(d.join("afile"), "x").unwrap();
    let p = d.join("afile").join("settings.json");
    assert!(matches!(save(&p, &all_set()), Err(CoreError::Io(_))));
}

#[test]
fn missing_names_every_unset_path_in_setting_names_order() {
    assert_eq!(Settings::default().missing(), SETTING_NAMES.to_vec());
    let one = Settings {
        nextcloud_backups: Some(PathBuf::from("/srv/nextcloud/Backups")),
        ..Settings::default()
    };
    assert_eq!(one.missing(), vec!["pipeline_repo", "vault_agents"]);
    assert!(all_set().missing().is_empty());
}

#[test]
fn an_empty_path_counts_as_missing() {
    let s = Settings {
        pipeline_repo: Some(PathBuf::new()),
        ..all_set()
    };
    assert_eq!(s.missing(), vec!["pipeline_repo"]);
    assert_eq!(
        s.pipeline_repo(),
        Err(CoreError::MissingPath("pipeline_repo"))
    );
}

#[test]
fn pipeline_repo_gives_the_path_when_set() {
    let s = all_set();
    assert_eq!(s.pipeline_repo(), Ok(Path::new("/srv/pipeline")));
}

#[test]
fn pipeline_repo_names_the_setting_when_unset() {
    assert_eq!(
        Settings::default().pipeline_repo(),
        Err(CoreError::MissingPath("pipeline_repo"))
    );
}
