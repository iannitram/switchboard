//! Review addition for BAK-1 (#226): a size written as a float (1e9 bytes or more) is still a
//! number, so the file is marked skipped, as the issue's expectation 3 says.

use serde_json::{json, Value};
use std::path::PathBuf;
use switchboard_core::backup::{write_release_list, Manifest};

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

#[test]
fn a_float_size_at_or_over_the_limit_is_marked_skipped() {
    let dir = temp_dir("bak-rel-float");
    let input = vec![json!({
        "tag_name": "v3",
        "assets": [
            {"name": "over.bin", "size": 1_000_000_000.0, "browser_download_url": "https://example.invalid/over.bin"},
            {"name": "under.bin", "size": 999_999_999.0, "browser_download_url": "https://example.invalid/under.bin"}
        ]
    })];
    let mut m = Manifest::default();
    write_release_list(&input, &dir, &mut m).unwrap();
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("releases.json")).unwrap()).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(written[0]["assets"][0]["skipped"], json!("over 1 GB"));
    assert!(written[0]["assets"][1].get("skipped").is_none());
}
