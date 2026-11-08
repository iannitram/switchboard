//! R28 probe for issue 4: behaviour the issue states that the locked tests in `settings_file.rs`
//! do not pin down. Added by the R25 review; the locked file is not edited.

use std::path::PathBuf;
use switchboard_core::settings::{load, save, Settings};
use switchboard_core::CoreError;

/// A fresh, empty folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "switchboard-settings-probe-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_parse_error_names_the_full_path() {
    let d = scratch("parse-path");
    let p = d.join("settings.json");
    std::fs::write(&p, "{not json").unwrap();
    let shown = p.display().to_string();
    match load(&p) {
        Err(CoreError::Parse(msg)) => assert!(msg.contains(&shown), "{msg} does not name {shown}"),
        other => panic!("expected CoreError::Parse, got {other:?}"),
    }
}

#[test]
fn a_failed_save_leaves_the_old_file_in_place() {
    let d = scratch("tmp-blocked");
    let p = d.join("settings.json");
    let old = Settings {
        pipeline_repo: Some(PathBuf::from("/srv/old")),
        ..Settings::default()
    };
    save(&p, &old).unwrap();
    // A folder named settings.json.tmp blocks the temp-file write, so the save must fail.
    std::fs::create_dir_all(d.join("settings.json.tmp")).unwrap();
    let new = Settings {
        pipeline_repo: Some(PathBuf::from("/srv/new")),
        ..Settings::default()
    };
    assert!(matches!(save(&p, &new), Err(CoreError::Io(_))));
    assert_eq!(load(&p), Ok(old));
}
