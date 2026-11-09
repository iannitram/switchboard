//! Locked tests for BAK-1 (the releases as `releases.json`, every file of 1 GB or more marked
//! skipped, and `manifest.releases`). Builders never edit this file (R28).

use serde_json::{json, Value};
use std::path::PathBuf;
use switchboard_core::backup::{write_release_list, Manifest, RELEASE_FILE_LIMIT};
use switchboard_core::CoreError;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-{tag}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Two releases as GitHub's REST API returns them: v1.0 with two small files and one of
/// exactly 1,000,000,000 bytes; v0.9 with one small file and one of 2,500,000,000 bytes.
fn releases() -> Vec<Value> {
    vec![
        json!({
            "tag_name": "v1.0",
            "name": "One",
            "assets": [
                {"name": "app.zip", "size": 1200, "browser_download_url": "https://example.invalid/v1.0/app.zip"},
                {"name": "notes.txt", "size": 999_999_999u64, "browser_download_url": "https://example.invalid/v1.0/notes.txt"},
                {"name": "big.iso", "size": 1_000_000_000u64, "browser_download_url": "https://example.invalid/v1.0/big.iso"}
            ]
        }),
        json!({
            "tag_name": "v0.9",
            "name": "Nine",
            "assets": [
                {"name": "app.zip", "size": 800, "browser_download_url": "https://example.invalid/v0.9/app.zip"},
                {"name": "huge.img", "size": 2_500_000_000u64, "browser_download_url": "https://example.invalid/v0.9/huge.img"}
            ]
        }),
    ]
}

fn manifest() -> Manifest {
    Manifest {
        repo: "example/fixture".to_string(),
        issues: 7,
        wiki: true,
        ..Manifest::default()
    }
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_limit_is_one_billion_bytes() {
    assert_eq!(RELEASE_FILE_LIMIT, 1_000_000_000);
}

#[test]
fn releases_json_holds_every_release_with_big_files_marked_skipped() {
    let dir = temp_dir("bak-rel-list");
    let input = releases();
    let mut m = manifest();
    write_release_list(&input, &dir, &mut m).unwrap();
    let written = read_json(&dir.join("releases.json"));
    std::fs::remove_dir_all(&dir).ok();

    let mut expected = releases();
    expected[0]["assets"][2]["skipped"] = json!("over 1 GB");
    expected[1]["assets"][1]["skipped"] = json!("over 1 GB");
    assert_eq!(written, Value::Array(expected));
    assert_eq!(input, releases(), "the input slice must not be changed");
}

#[test]
fn files_under_the_limit_carry_no_skipped_key() {
    let dir = temp_dir("bak-rel-small");
    let mut m = manifest();
    write_release_list(&releases(), &dir, &mut m).unwrap();
    let written = read_json(&dir.join("releases.json"));
    std::fs::remove_dir_all(&dir).ok();

    for (r, a) in [(0, 0), (0, 1), (1, 0)] {
        let asset = &written[r]["assets"][a];
        assert!(
            asset.get("skipped").is_none(),
            "asset {r}/{a} is under 1 GB: {asset}"
        );
    }
}

#[test]
fn an_asset_without_a_numeric_size_is_left_as_it_is() {
    let dir = temp_dir("bak-rel-nosize");
    let input = vec![json!({
        "tag_name": "v2",
        "assets": [
            {"name": "a.bin", "browser_download_url": "https://example.invalid/a.bin"},
            {"name": "b.bin", "size": "huge", "browser_download_url": "https://example.invalid/b.bin"}
        ]
    })];
    let mut m = manifest();
    write_release_list(&input, &dir, &mut m).unwrap();
    let written = read_json(&dir.join("releases.json"));
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(written, Value::Array(input));
}

#[test]
fn manifest_releases_counts_releases_and_nothing_else_changes() {
    let dir = temp_dir("bak-rel-manifest");
    let mut m = manifest();
    write_release_list(&releases(), &dir, &mut m).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    let expected = Manifest {
        releases: 2,
        ..manifest()
    };
    assert_eq!(m, expected);
}

#[test]
fn no_releases_writes_an_empty_array_and_zero() {
    let dir = temp_dir("bak-rel-empty");
    let mut m = Manifest {
        releases: 9,
        ..manifest()
    };
    write_release_list(&[], &dir, &mut m).unwrap();
    let written = read_json(&dir.join("releases.json"));
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(written, json!([]));
    assert_eq!(m.releases, 0);
}

#[test]
fn a_missing_out_dir_is_created_and_no_new_file_is_left() {
    let dir = temp_dir("bak-rel-mkdir");
    let out = dir.join("Backups").join("github").join("demo");
    let mut m = manifest();
    write_release_list(&releases(), &out, &mut m).unwrap();
    let exists = out.join("releases.json").is_file();
    let stray = out.join("releases.json.new").exists();
    std::fs::remove_dir_all(&dir).ok();

    assert!(exists, "releases.json should be written in the new dir");
    assert!(!stray, "releases.json.new should be renamed into place");
}

#[test]
fn a_second_run_replaces_the_first() {
    let dir = temp_dir("bak-rel-again");
    let mut m = manifest();
    write_release_list(&releases(), &dir, &mut m).unwrap();
    let only_one = vec![releases()[1].clone()];
    write_release_list(&only_one, &dir, &mut m).unwrap();
    let written = read_json(&dir.join("releases.json"));
    std::fs::remove_dir_all(&dir).ok();

    let mut expected = only_one;
    expected[0]["assets"][1]["skipped"] = json!("over 1 GB");
    assert_eq!(written, Value::Array(expected));
    assert_eq!(m.releases, 1);
}

#[test]
fn a_failed_write_keeps_the_old_file_and_the_old_count() {
    let dir = temp_dir("bak-rel-fail");
    let mut m = manifest();
    write_release_list(&releases(), &dir, &mut m).unwrap();
    let before = std::fs::read_to_string(dir.join("releases.json")).unwrap();

    // A directory where `releases.json.new` should go makes the write fail.
    std::fs::create_dir(dir.join("releases.json.new")).unwrap();
    let result = write_release_list(&[], &dir, &mut m);
    let after = std::fs::read_to_string(dir.join("releases.json")).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert!(matches!(result, Err(CoreError::Io(_))), "got {result:?}");
    assert_eq!(
        after, before,
        "the old releases.json must be left as it was"
    );
    assert_eq!(m.releases, 2, "a failed write must not change the manifest");
}
