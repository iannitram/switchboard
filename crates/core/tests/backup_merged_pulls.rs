//! `Manifest.merged_pulls` (owner ruling on #92, 2026-11-08): the merged-PR count BAK-2
//! discounts, filled by `write_json`. Kept out of the locked `backup_json.rs`.

use serde_json::json;
use switchboard_core::backup::{write_json, RecordedAnswers};

#[test]
fn merged_pulls_counts_pulls_with_merged_at_set() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("sb-bak-merged-{}-{nanos}", std::process::id()));
    let answers = RecordedAnswers {
        repo: "example/merged".to_string(),
        pulls: vec![
            json!({"number": 1, "state": "closed", "merged_at": "2026-11-01T10:00:00Z", "body": ""}),
            json!({"number": 2, "state": "closed", "merged_at": null, "body": ""}),
            json!({"number": 3, "state": "open", "merged_at": null, "body": ""}),
            json!({"number": 4, "state": "closed", "merged_at": "2026-11-03T08:15:00Z", "body": ""}),
            // No `merged_at` at all counts as not merged.
            json!({"number": 5, "state": "closed", "body": ""}),
        ],
        ..RecordedAnswers::default()
    };
    let manifest = write_json(&answers, &dir, "2026-11-08T07:00:00Z").unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(manifest.merged_pulls, 2);
    assert_eq!(manifest.pulls, 5);
    assert_eq!(manifest.open_pulls, 1);
}
