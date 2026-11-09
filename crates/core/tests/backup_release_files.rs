//! Locked tests for BAK-1 (release files under 1 GB in `releases/<tag>/`, and `write_releases`).
//! Builders never edit this file (R28). File bytes come from a stub `FileSource`; nothing
//! touches the network.

use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use switchboard_core::backup::{fetch_release_files, write_releases, FileSource, Manifest};
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

/// Serves `bytes` by URL, fails for any URL it does not hold, and records every URL asked for.
struct Stub {
    bytes: HashMap<String, Vec<u8>>,
    asked: RefCell<Vec<String>>,
}

impl Stub {
    fn new(pairs: &[(&str, &[u8])]) -> Stub {
        Stub {
            bytes: pairs
                .iter()
                .map(|(url, b)| (url.to_string(), b.to_vec()))
                .collect(),
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl FileSource for Stub {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, CoreError> {
        self.asked.borrow_mut().push(url.to_string());
        self.bytes
            .get(url)
            .cloned()
            .ok_or_else(|| CoreError::GitHub(format!("404 {url}")))
    }
}

const U: &str = "https://example.invalid";

fn asset(tag: &str, name: &str, size: u64) -> Value {
    json!({"name": name, "size": size, "browser_download_url": format!("{U}/{tag}/{name}")})
}

/// Two releases with three files each; v1.0's `big.iso` is exactly 1,000,000,000 bytes.
fn releases() -> Vec<Value> {
    vec![
        json!({"tag_name": "v1.0", "assets": [
            asset("v1.0", "app.zip", 3),
            asset("v1.0", "notes.txt", 5),
            asset("v1.0", "big.iso", 1_000_000_000)
        ]}),
        json!({"tag_name": "v0.9", "assets": [
            asset("v0.9", "app.zip", 2),
            asset("v0.9", "notes.txt", 4),
            asset("v0.9", "sum.sha", 1)
        ]}),
    ]
}

fn stub() -> Stub {
    Stub::new(&[
        ("https://example.invalid/v1.0/app.zip", b"one"),
        ("https://example.invalid/v1.0/notes.txt", b"notes"),
        ("https://example.invalid/v1.0/big.iso", b"never fetched"),
        ("https://example.invalid/v0.9/app.zip", b"v9"),
        ("https://example.invalid/v0.9/notes.txt", b"nine"),
        ("https://example.invalid/v0.9/sum.sha", b"s"),
    ])
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn files_under_one_gb_land_in_releases_tag_name() {
    let dir = temp_dir("bak-relf-write");
    let n = fetch_release_files(&releases(), &stub(), &dir).unwrap();
    let r = dir.join("releases");
    let got = [
        read(&r.join("v1.0").join("app.zip")),
        read(&r.join("v1.0").join("notes.txt")),
        read(&r.join("v0.9").join("app.zip")),
        read(&r.join("v0.9").join("notes.txt")),
        read(&r.join("v0.9").join("sum.sha")),
    ];
    let big = r.join("v1.0").join("big.iso").exists();
    let stray = dir.join("releases.new").exists();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(n, 5, "five files are under 1 GB");
    let want: [&[u8]; 5] = [b"one", b"notes", b"v9", b"nine", b"s"];
    for (g, w) in got.iter().zip(want) {
        assert_eq!(g.as_slice(), w);
    }
    assert!(!big, "a file of 1 GB or more is not written");
    assert!(!stray, "releases.new should be renamed to releases");
}

#[test]
fn a_file_of_one_gb_or_more_is_never_fetched_and_order_is_kept() {
    let dir = temp_dir("bak-relf-skip");
    let s = stub();
    fetch_release_files(&releases(), &s, &dir).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    let asked = s.asked.borrow().clone();
    assert_eq!(
        asked,
        vec![
            format!("{U}/v1.0/app.zip"),
            format!("{U}/v1.0/notes.txt"),
            format!("{U}/v0.9/app.zip"),
            format!("{U}/v0.9/notes.txt"),
            format!("{U}/v0.9/sum.sha"),
        ]
    );
}

#[test]
fn slashes_and_dot_names_become_safe_path_parts() {
    let dir = temp_dir("bak-relf-safe");
    let input = vec![
        json!({"tag_name": "release/1.0", "assets": [
            {"name": "a\\b.txt", "size": 1, "browser_download_url": format!("{U}/x")}
        ]}),
        json!({"tag_name": "..", "assets": [
            {"name": ".", "size": 1, "browser_download_url": format!("{U}/y")}
        ]}),
    ];
    let s = Stub::new(&[
        ("https://example.invalid/x", b"x"),
        ("https://example.invalid/y", b"y"),
    ]);
    let n = fetch_release_files(&input, &s, &dir).unwrap();
    let x = read(&dir.join("releases").join("release_1.0").join("a_b.txt"));
    let y = read(&dir.join("releases").join("_").join("_"));
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(n, 2);
    assert_eq!(x, b"x");
    assert_eq!(y, b"y");
}

#[test]
fn a_second_run_replaces_the_old_folder() {
    let dir = temp_dir("bak-relf-again");
    fetch_release_files(&releases(), &stub(), &dir).unwrap();
    let only_v09 = vec![releases()[1].clone()];
    let n = fetch_release_files(&only_v09, &stub(), &dir).unwrap();
    let old_gone = !dir.join("releases").join("v1.0").exists();
    let kept = read(&dir.join("releases").join("v0.9").join("app.zip"));
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(n, 3);
    assert!(old_gone, "releases/ should hold only this run's files");
    assert_eq!(kept, b"v9");
}

#[test]
fn a_failed_fetch_leaves_the_old_files_in_place() {
    let dir = temp_dir("bak-relf-fail");
    fetch_release_files(&releases(), &stub(), &dir).unwrap();
    let mut changed = releases();
    changed[1]["assets"][2]["browser_download_url"] = json!(format!("{U}/gone"));
    let result = fetch_release_files(&changed, &stub(), &dir);
    let old = read(&dir.join("releases").join("v1.0").join("app.zip"));
    let stray = dir.join("releases.new").exists();
    std::fs::remove_dir_all(&dir).ok();

    assert!(result.is_err(), "a failed fetch is an error");
    assert_eq!(old, b"one", "the old releases/ must be left as it was");
    assert!(!stray, "a failed run leaves no releases.new behind");
}

#[test]
fn an_asset_without_a_url_is_a_parse_error() {
    let dir = temp_dir("bak-relf-nourl");
    let input = vec![json!({"tag_name": "v1", "assets": [{"name": "a", "size": 1}]})];
    let result = fetch_release_files(&input, &stub(), &dir);
    std::fs::remove_dir_all(&dir).ok();

    assert!(matches!(result, Err(CoreError::Parse(_))), "got {result:?}");
}

#[test]
fn write_releases_writes_files_list_and_count() {
    let dir = temp_dir("bak-relf-both");
    let mut m = Manifest::default();
    write_releases(&releases(), &stub(), &dir, &mut m).unwrap();
    let list: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("releases.json")).unwrap()).unwrap();
    let file = dir.join("releases").join("v0.9").join("sum.sha").is_file();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(m.releases, 2);
    assert!(file);
    assert_eq!(list[0]["assets"][2]["skipped"], json!("over 1 GB"));
}

#[test]
fn write_releases_with_a_failed_fetch_changes_nothing() {
    let dir = temp_dir("bak-relf-both-fail");
    let mut m = Manifest::default();
    write_releases(&releases(), &stub(), &dir, &mut m).unwrap();
    let before = std::fs::read_to_string(dir.join("releases.json")).unwrap();
    let only_bad = vec![json!({"tag_name": "v2", "assets": [asset("v2", "x.zip", 1)]})];
    let result = write_releases(&only_bad, &stub(), &dir, &mut m);
    let after = std::fs::read_to_string(dir.join("releases.json")).unwrap();
    let old = dir.join("releases").join("v1.0").join("app.zip").is_file();
    std::fs::remove_dir_all(&dir).ok();

    assert!(result.is_err());
    assert_eq!(
        after, before,
        "releases.json is written only after every file"
    );
    assert!(old);
    assert_eq!(m.releases, 2);
}
