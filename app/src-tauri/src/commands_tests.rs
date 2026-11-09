// Locked tests for the settings commands (CORE-5). The builder never edits this file.
// `commands.rs` takes it in with `#[cfg(test)] #[path = "commands_tests.rs"] mod tests;`.
use super::*;
use serde_json::json;
use switchboard_core::settings::Settings;

/// A fresh folder for one test, under the system temp folder.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("switchboard-app-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn views_end_with_settings() {
    assert_eq!(
        views(),
        vec![
            "Sky",
            "Waiting on you",
            "Reports",
            "Budget",
            "Chat",
            "Settings"
        ]
    );
}

#[test]
fn a_missing_file_is_every_path_unset() {
    let dir = scratch("missing");
    let view = settings_view(&dir.join("settings.json")).unwrap();
    assert_eq!(
        serde_json::to_value(&view).unwrap(),
        json!({
            "settings": { "pipeline_repo": null, "nextcloud_backups": null, "vault_agents": null },
            "missing": ["pipeline_repo", "nextcloud_backups", "vault_agents"]
        })
    );
}

#[test]
fn saving_writes_the_file_and_answers_what_was_saved() {
    let dir = scratch("save");
    let path = dir.join("nested").join("settings.json");
    let settings = Settings {
        pipeline_repo: Some(PathBuf::from("/srv/pipeline")),
        nextcloud_backups: None,
        vault_agents: Some(PathBuf::from("/srv/agents")),
    };
    let view = save_settings(&path, &settings).unwrap();
    let expected = json!({
        "settings": { "pipeline_repo": "/srv/pipeline", "nextcloud_backups": null, "vault_agents": "/srv/agents" },
        "missing": ["nextcloud_backups"]
    });
    assert_eq!(serde_json::to_value(&view).unwrap(), expected);
    assert!(path.is_file());
    assert_eq!(
        serde_json::to_value(settings_view(&path).unwrap()).unwrap(),
        expected
    );
}

#[test]
fn every_path_set_is_nothing_missing() {
    let dir = scratch("all");
    let path = dir.join("settings.json");
    let settings = Settings {
        pipeline_repo: Some(PathBuf::from("/a")),
        nextcloud_backups: Some(PathBuf::from("/b")),
        vault_agents: Some(PathBuf::from("/c")),
    };
    let view = save_settings(&path, &settings).unwrap();
    assert_eq!(serde_json::to_value(&view).unwrap()["missing"], json!([]));
}

#[test]
fn a_file_that_does_not_parse_is_a_parse_error_naming_it() {
    let dir = scratch("bad");
    let path = dir.join("settings.json");
    std::fs::write(&path, "{ not json").unwrap();
    let err = settings_view(&path).unwrap_err();
    assert!(matches!(err, CoreError::Parse(_)), "got {err:?}");
    let face = FaceError::new("settings_get", &err);
    assert_eq!(face.command, "settings_get");
    assert!(face.lines[0].starts_with("parse: "), "got {:?}", face.lines);
    assert!(
        face.lines[0].contains("settings.json"),
        "got {:?}",
        face.lines
    );
}

#[test]
fn a_save_that_cannot_write_is_an_io_error() {
    let dir = scratch("blocked");
    let blocker = dir.join("not-a-folder");
    std::fs::write(&blocker, "a file where the folder should be").unwrap();
    let err = save_settings(&blocker.join("settings.json"), &Settings::default()).unwrap_err();
    assert!(matches!(err, CoreError::Io(_)), "got {err:?}");
    assert!(FaceError::new("settings_save", &err).lines[0].starts_with("file: "));
}
