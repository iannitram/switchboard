//! #331: unreadable threads warn without hiding readable reports.
use std::path::PathBuf;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::frontdoor::thread::ThreadStore;
use switchboard_core::joblog::{JobRow, RowSource};
use switchboard_core::reports::{load, thread_rows_with_warnings, Filter, ReportsAnswer};

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("sb-331-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn append(&self, id: &str) {
        ThreadStore::new(&self.0)
            .append(
                id,
                &JobRow {
                    handoff_id: Some(id.into()),
                    outcome: Some("passed".into()),
                    source: RowSource::Chat,
                    ..JobRow::default()
                },
            )
            .unwrap();
    }

    fn answer(&self, filter: Option<&Filter>) -> ReportsAnswer {
        let (threads, warnings) = thread_rows_with_warnings(&self.0);
        let state = self.0.join("state");
        let reports = load(
            &FleetBridge::new(&self.0.join("pipeline")),
            Some(state.as_os_str()),
            &threads,
            filter,
        )
        .unwrap();
        ReportsAnswer { reports, warnings }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn an_unreadable_thread_warns_with_its_id_and_reason_and_keeps_other_reports() {
    let fixture = Fixture::new("mixed");
    fixture.append("chat-a");
    fixture.append("chat-z");
    let bad = fixture.0.join("threads").join("chat-bad.jsonl");
    std::fs::write(bad, "not a job row\n").unwrap();
    let done = fixture.0.join("state").join("jobs").join("done");
    std::fs::create_dir_all(&done).unwrap();
    std::fs::write(done.join("job.json"), r#"{"id":"job","outcome":"passed"}"#).unwrap();

    let answer = fixture.answer(None);
    let ids: Vec<_> = answer.reports.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["job", "chat-a", "chat-z"]);
    assert_eq!(answer.warnings.len(), 1);
    let reason = ThreadStore::new(&fixture.0)
        .read("chat-bad")
        .unwrap_err()
        .to_string();
    assert_eq!(
        answer.warnings[0],
        format!("Skipped unreadable thread chat-bad: {reason}")
    );
    assert!(answer.warnings[0].contains("line 1"));
    let json = serde_json::to_value(&answer).unwrap();
    assert_eq!(json["reports"].as_array().unwrap().len(), 3);
    assert_eq!(json["warnings"][0], answer.warnings[0]);

    // A filter matching nothing must not hide a warning about the source data.
    let filtered = fixture.answer(Some(&Filter {
        outcome: Some("failed".into()),
        ..Filter::default()
    }));
    assert!(filtered.reports.is_empty());
    assert_eq!(filtered.warnings, answer.warnings);
}

#[test]
fn readable_threads_produce_reports_without_warnings() {
    let fixture = Fixture::new("readable");
    fixture.append("chat-a");
    fixture.append("chat-b");
    let answer = fixture.answer(None);
    assert_eq!(answer.reports.len(), 2);
    assert!(answer.warnings.is_empty());
    assert_eq!(
        serde_json::to_value(answer).unwrap()["warnings"],
        serde_json::json!([])
    );
}

#[test]
fn missing_and_empty_stores_have_no_warnings() {
    let fixture = Fixture::new("empty");
    let answer = fixture.answer(None);
    assert!(answer.reports.is_empty());
    assert!(answer.warnings.is_empty());
    std::fs::create_dir(fixture.0.join("threads")).unwrap();
    let answer = fixture.answer(None);
    assert!(answer.reports.is_empty());
    assert!(answer.warnings.is_empty());
}

#[test]
fn a_store_that_cannot_be_listed_warns() {
    let fixture = Fixture::new("list-failure");
    std::fs::write(fixture.0.join("threads"), "not a directory").unwrap();
    let answer = fixture.answer(None);
    assert!(answer.reports.is_empty());
    assert_eq!(answer.warnings.len(), 1);
    assert!(answer.warnings[0].starts_with("Could not list chat threads: file:"));
}
