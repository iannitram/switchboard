//! R28 probe for issue 4: behaviour the issue states that the locked tests in `settings_file.rs`
//! do not pin down. Added by the R25 review; the locked file is not edited.

use std::path::{Path, PathBuf};
use switchboard_core::settings::{load, save, temp_path, Settings};
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
    // A read-only folder blocks the temp-file write (Linux, macOS), and a read-only settings
    // file blocks the rename over it (Windows ignores a folder's read-only flag), so the save
    // must fail either way. (Each save has a temp name of its own since #371, so a folder at
    // one name no longer blocks it.)
    let lock = |path: &Path, on: bool| {
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(on);
        std::fs::set_permissions(path, perms).unwrap();
    };
    lock(&p, true);
    lock(&d, true);
    let new = Settings {
        pipeline_repo: Some(PathBuf::from("/srv/new")),
        ..Settings::default()
    };
    let saved = save(&p, &new);
    lock(&d, false);
    lock(&p, false);
    assert!(matches!(saved, Err(CoreError::Io(_))), "{saved:?}");
    // The failed save left no temp file behind.
    let leftovers: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert_eq!(load(&p), Ok(old));
}

#[test]
fn saves_at_once_from_many_threads_all_succeed() {
    // #371: every save used to write the same `settings.json.tmp`, so one save renamed the
    // file another was still writing and most saves failed.
    let d = scratch("many-threads");
    let p = d.join("settings.json");
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let p = p.clone();
            std::thread::spawn(move || {
                let mut errs = Vec::new();
                for i in 0..50 {
                    let s = Settings {
                        pipeline_repo: Some(PathBuf::from(format!("/srv/{t}-{i}"))),
                        ..Settings::default()
                    };
                    if let Err(e) = save(&p, &s) {
                        errs.push(e);
                    }
                }
                errs
            })
        })
        .collect();
    let errs: Vec<CoreError> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    assert!(
        errs.is_empty(),
        "{} saves failed: {:?}",
        errs.len(),
        errs.first()
    );
    assert!(load(&p).unwrap().pipeline_repo.is_some());
    let leftovers: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn two_temp_paths_for_one_file_differ_and_sit_beside_it() {
    let p = Path::new("/cfg/settings.json");
    let (a, b) = (temp_path(p), temp_path(p));
    assert_ne!(a, b);
    assert_eq!(a.parent(), p.parent());
    let name = a.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("settings.json.") && name.ends_with(".tmp"),
        "{name}"
    );
}
