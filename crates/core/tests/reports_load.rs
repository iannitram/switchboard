//! REP-W: the `reports` command's core side: where the inputs are, reading the done cards, and
//! assembling the reports the Reports view asks for (REP-1, REP-2, REP-3). Locked tests (R28):
//! the builder never edits this file.

use serde_json::{json, Value};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::joblog::{read_all, JobRow};
use switchboard_core::reports::{
    assemble, from_card, from_thread, load, newest_first, read_done_cards, state_dir_from, Filter,
    Report, FLEET_STATE_DIR_ENV,
};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A fresh, empty folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-repw-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn ids(reports: &[Report]) -> Vec<&str> {
    reports.iter().map(|r| r.id.as_str()).collect()
}

/// Three done cards: two with a time (b newer than a) and one without.
fn cards() -> Vec<Value> {
    vec![
        json!({"id": "job-a", "title": "A", "repo": "owner/app-a", "lane": "qwen",
               "outcome": "passed", "at": "2026-11-06T13:00:00Z", "checks": []}),
        json!({"id": "job-b", "title": "B", "pr_url": "https://github.com/owner/app-b/pull/3",
               "lane": "gemini", "outcome": "failed", "failure_mode": "tests",
               "at": "2026-11-07T09:30:00-07:00",
               "checks": [{"name": "test", "status": "failed", "note": "1 of 4"}]}),
        json!({"id": "job-c", "title": "C", "repo": "owner/app-a", "lane": "gemini",
               "outcome": "passed"}),
    ]
}

/// Ledger events: an `end` row for job-a and job-b, a `start` row, and a rating.
fn events() -> Vec<Value> {
    vec![
        json!({"event": "start", "id": "job-a", "lane": "qwen"}),
        json!({"event": "end", "id": "job-a", "lane": "qwen", "model": "qwen3-coder",
               "files_changed": 2, "loc_changed": 40, "tokens": 1200, "outcome": "passed"}),
        json!({"event": "end", "id": "job-b", "lane": "gemini", "model": "gemini-3-flash",
               "files_changed": 5, "loc_changed": 210, "outcome": "failed"}),
        json!({"event": "rating", "id": "job-a", "rating": "up"}),
    ]
}

fn threads() -> Vec<(String, Vec<JobRow>)> {
    let row = |lane: &str, model: &str, outcome: &str| JobRow {
        lane: Some(lane.to_string()),
        model: Some(model.to_string()),
        outcome: Some(outcome.to_string()),
        tokens: Some(300),
        ..Default::default()
    };
    vec![(
        "thread-1".to_string(),
        vec![
            row("qwen", "qwen3-coder", "down"),
            row("claude", "sonnet", "up"),
        ],
    )]
}

// --- where the inputs are: state_dir_from ---

#[test]
fn a_set_fleet_state_dir_env_wins() {
    let root = scratch("env-root");
    fs::create_dir_all(root.join(".git")).unwrap();
    let got = state_dir_from(&root, Some(OsStr::new("/srv/state")), "");
    assert_eq!(got, PathBuf::from("/srv/state"));
    assert_eq!(FLEET_STATE_DIR_ENV, "FLEET_STATE_DIR");
}

#[test]
fn an_empty_env_is_not_set() {
    let base = scratch("env-empty");
    let root = base.join("potn-dev");
    fs::create_dir_all(&root).unwrap();
    let got = state_dir_from(&root, Some(OsStr::new("")), "");
    assert_eq!(got, base.join("potn-dev-fleet-state"));
}

#[test]
fn a_root_that_is_not_a_checkout_uses_the_sibling_folder() {
    let base = scratch("no-git");
    let root = base.join("potn-dev");
    fs::create_dir_all(&root).unwrap();
    let elsewhere = base.join("other");
    fs::create_dir_all(&elsewhere).unwrap();
    let porcelain = format!(
        "worktree {}\nHEAD abc\nbranch refs/heads/fleet-state\n\n",
        elsewhere.display()
    );
    assert_eq!(
        state_dir_from(&root, None, &porcelain),
        base.join("potn-dev-fleet-state")
    );
}

#[test]
fn a_sibling_checkout_wins_over_the_worktree_list() {
    let base = scratch("sibling");
    let root = base.join("potn-dev");
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(base.join("potn-dev-fleet-state").join(".git")).unwrap();
    let elsewhere = base.join("other");
    fs::create_dir_all(&elsewhere).unwrap();
    let porcelain = format!(
        "worktree {}\nHEAD abc\nbranch refs/heads/fleet-state\n\n",
        elsewhere.display()
    );
    assert_eq!(
        state_dir_from(&root, None, &porcelain),
        base.join("potn-dev-fleet-state")
    );
}

#[test]
fn otherwise_the_first_existing_fleet_state_worktree() {
    let base = scratch("worktrees");
    let root = base.join("potn-dev");
    fs::create_dir_all(root.join(".git")).unwrap();
    let gone = base.join("gone");
    let main_wt = base.join("main-wt");
    let live = base.join("coord-fleet-state");
    let later = base.join("later-fleet-state");
    for d in [&main_wt, &live, &later] {
        fs::create_dir_all(d).unwrap();
    }
    let porcelain = format!(
        "worktree {}\nHEAD 111\nbranch refs/heads/main\n\n\
         worktree {}\nHEAD 222\nbranch refs/heads/fleet-state\n\n\
         worktree {}\nHEAD 333\nbranch refs/heads/fleet-state\n\n\
         worktree {}\nHEAD 444\nbranch refs/heads/fleet-state\n",
        main_wt.display(),
        gone.display(),
        live.display(),
        later.display()
    );
    assert_eq!(state_dir_from(&root, None, &porcelain), live);
}

#[test]
fn a_checkout_with_no_fleet_state_worktree_uses_the_sibling_folder() {
    let base = scratch("none");
    let root = base.join("potn-dev");
    fs::create_dir_all(root.join(".git")).unwrap();
    let main_wt = base.join("main-wt");
    fs::create_dir_all(&main_wt).unwrap();
    let porcelain = format!(
        "worktree {}\nHEAD 111\nbranch refs/heads/main\n\n",
        main_wt.display()
    );
    assert_eq!(
        state_dir_from(&root, None, &porcelain),
        base.join("potn-dev-fleet-state")
    );
    assert_eq!(
        state_dir_from(&root, None, ""),
        base.join("potn-dev-fleet-state")
    );
}

// --- the done cards: read_done_cards ---

#[test]
fn no_done_folder_is_no_cards() {
    let state = scratch("no-done");
    assert_eq!(read_done_cards(&state).unwrap(), Vec::<Value>::new());
}

#[test]
fn done_cards_are_the_json_files_by_name_without_job_files() {
    let state = scratch("done");
    let done = state.join("jobs").join("done");
    write(&done.join("b.json"), r#"{"id": "b"}"#);
    write(&done.join("a.json"), r#"{"id": "a"}"#);
    write(&done.join("a.1.json"), r#"{"id": "a", "n": 1}"#);
    write(&done.join("a.job.json"), r#"{"id": "a-job"}"#);
    write(&done.join("notes.txt"), r#"{"id": "txt"}"#);
    write(&done.join("broken.json"), "{not json");
    write(&done.join("list.json"), "[1, 2]");
    fs::create_dir_all(done.join("dir.json")).unwrap();
    write(
        &state.join("jobs").join("ready").join("r.json"),
        r#"{"id": "ready"}"#,
    );

    let got = read_done_cards(&state).unwrap();
    assert_eq!(
        got,
        vec![
            json!({"id": "a", "n": 1}),
            json!({"id": "a"}),
            json!({"id": "b"})
        ]
    );
}

// --- assembling: assemble ---

#[test]
fn assemble_is_cards_then_threads_newest_first() {
    let (c, e, t) = (cards(), events(), threads());
    let rows = read_all(&e, &[]);
    let expected = newest_first(&[
        from_card(&c[0], &rows),
        from_card(&c[1], &rows),
        from_card(&c[2], &rows),
        from_thread("thread-1", &t[0].1),
    ]);
    let got = assemble(&c, &e, &t, None);
    assert_eq!(got, expected);
    assert_eq!(ids(&got), vec!["job-b", "job-a", "job-c", "thread-1"]);
}

#[test]
fn assemble_takes_files_and_lines_from_the_ledger_end_rows() {
    let got = assemble(&cards(), &events(), &[], None);
    let a = got.iter().find(|r| r.id == "job-a").unwrap();
    assert_eq!((a.changed.files, a.changed.lines), (Some(2), Some(40)));
    let b = got.iter().find(|r| r.id == "job-b").unwrap();
    assert_eq!((b.changed.files, b.changed.lines), (Some(5), Some(210)));
    assert_eq!(b.project.as_deref(), Some("owner/app-b"));
    let c = got.iter().find(|r| r.id == "job-c").unwrap();
    assert_eq!((c.changed.files, c.changed.lines), (None, None));
}

#[test]
fn assemble_with_a_filter_keeps_only_the_matches_in_order() {
    let f = Filter {
        project: Some("owner/app-a".to_string()),
        outcome: Some("passed".to_string()),
        pool: None,
    };
    let got = assemble(&cards(), &events(), &threads(), Some(&f));
    assert_eq!(ids(&got), vec!["job-a", "job-c"]);

    let pool = Filter {
        pool: Some("claude".to_string()),
        ..Default::default()
    };
    assert_eq!(
        ids(&assemble(&cards(), &events(), &threads(), Some(&pool))),
        vec!["thread-1"]
    );

    let none = Filter {
        outcome: Some("cancelled".to_string()),
        ..Default::default()
    };
    assert!(assemble(&cards(), &events(), &threads(), Some(&none)).is_empty());
}

#[test]
fn an_all_unset_filter_is_the_same_as_none() {
    let all = assemble(&cards(), &events(), &threads(), None);
    let unset = assemble(&cards(), &events(), &threads(), Some(&Filter::default()));
    assert_eq!(unset, all);
}

#[test]
fn nothing_in_is_nothing_out() {
    assert!(assemble(&[], &[], &[], None).is_empty());
}

#[test]
fn assemble_leaves_its_inputs_as_they_were() {
    let (c, e, t) = (cards(), events(), threads());
    let f = Filter {
        project: Some("owner/app-a".to_string()),
        ..Default::default()
    };
    let _ = assemble(&c, &e, &t, Some(&f));
    assert_eq!(c, cards());
    assert_eq!(e, events());
    assert_eq!(t, threads());
    assert_eq!(f.project.as_deref(), Some("owner/app-a"));
}

// --- the JSON the face reads (app/src/brocrew/ReportEntry.tsx `Report`) ---

#[test]
fn a_report_serialises_to_the_face_report_fields() {
    let got = assemble(&cards(), &events(), &threads(), None);
    let v = serde_json::to_value(&got).unwrap();
    let keys = |v: &Value| -> Vec<String> { v.as_object().unwrap().keys().cloned().collect() };
    let b = &v[0];
    assert_eq!(
        keys(b),
        vec![
            "id",
            "title",
            "project",
            "at",
            "outcome",
            "failure_mode",
            "changed",
            "checked",
            "not_checked",
            "cost",
            "moves"
        ]
    );
    assert_eq!(keys(&b["changed"]), vec!["pr", "files", "lines"]);
    assert_eq!(keys(&b["cost"]), vec!["pool", "tokens", "usd", "seconds"]);
    assert_eq!(b["id"], json!("job-b"));
    assert_eq!(b["at"], json!(1_794_069_000));
    assert_eq!(
        b["changed"]["pr"],
        json!("https://github.com/owner/app-b/pull/3")
    );
    assert_eq!(b["checked"], json!(["test: failed (1 of 4)"]));
    let thread = &v[3];
    assert_eq!(
        keys(&thread["moves"][0]),
        vec![
            "from_service",
            "from_model",
            "to_service",
            "to_model",
            "why"
        ]
    );
    assert_eq!(thread["at"], Value::Null);
}

#[test]
fn the_face_filter_json_reads_as_a_filter() {
    let f: Filter = serde_json::from_value(json!({
        "project": null, "outcome": "failed", "pool": null
    }))
    .unwrap();
    assert_eq!(
        f,
        Filter {
            outcome: Some("failed".to_string()),
            ..Default::default()
        }
    );
}

// --- reading from disk: load ---

/// A pipeline root (not a checkout) with its sibling fleet-state folder holding the cards and
/// a machine ledger, and the legacy ledger under the root.
fn pipeline(name: &str) -> (PathBuf, PathBuf) {
    let base = scratch(name);
    let root = base.join("potn-dev");
    let state = base.join("potn-dev-fleet-state");
    fs::create_dir_all(&root).unwrap();
    let c = cards();
    for (i, card) in c.iter().enumerate() {
        let id = card["id"].as_str().unwrap();
        write(
            &state.join("jobs").join("done").join(format!("{id}.json")),
            &card.to_string(),
        );
        write(
            &state
                .join("jobs")
                .join("done")
                .join(format!("{id}.job.json")),
            &json!({"id": format!("job-file-{i}")}).to_string(),
        );
    }
    let e = events();
    write(
        &root.join("logs").join("jobs.jsonl"),
        &format!("{}\n{}\n", e[0], e[1]),
    );
    write(
        &state.join("ledger").join("strix-halo.jsonl"),
        &format!("{}\n{}\n", e[2], e[3]),
    );
    (root, state)
}

#[test]
fn load_reads_the_cards_and_both_ledgers() {
    let (root, _) = pipeline("load");
    let fleet = FleetBridge::with_root(&root);
    let got = load(&fleet, None, &threads(), None).unwrap();
    assert_eq!(got, assemble(&cards(), &events(), &threads(), None));
    assert_eq!(ids(&got), vec!["job-b", "job-a", "job-c", "thread-1"]);
}

#[test]
fn load_applies_the_filter() {
    let (root, _) = pipeline("load-filter");
    let fleet = FleetBridge::with_root(&root);
    let f = Filter {
        pool: Some("gemini".to_string()),
        ..Default::default()
    };
    let got = load(&fleet, None, &[], Some(&f)).unwrap();
    assert_eq!(ids(&got), vec!["job-b", "job-c"]);
}

#[test]
fn load_follows_the_env_to_another_state_folder() {
    let (root, _) = pipeline("load-env");
    let other = scratch("load-env-other");
    write(
        &other.join("jobs").join("done").join("z.json"),
        r#"{"id": "job-z", "outcome": "passed"}"#,
    );
    let fleet = FleetBridge::with_root(&root);
    let got = load(&fleet, Some(other.as_os_str()), &[], None).unwrap();
    assert_eq!(ids(&got), vec!["job-z"]);
}

#[test]
fn load_with_no_state_folder_is_empty() {
    let root = scratch("load-empty").join("potn-dev");
    fs::create_dir_all(&root).unwrap();
    let fleet = FleetBridge::with_root(&root);
    assert!(load(&fleet, None, &[], None).unwrap().is_empty());
}

// --- the Tauri command (app/src-tauri, not built by `cargo test`) ---

fn shell(file: &str) -> String {
    fs::read_to_string(manifest().join("../../app/src-tauri/src").join(file)).unwrap()
}

#[test]
fn main_registers_the_reports_command() {
    let main = shell("main.rs");
    let start = main
        .find("tauri::generate_handler![")
        .expect("main.rs has generate_handler!");
    let end = start + main[start..].find(']').unwrap();
    let handler = &main[start..end];
    assert!(
        handler.contains("commands::reports,"),
        "generate_handler! lacks commands::reports: {handler}"
    );
    for kept in [
        "commands::views,",
        "commands::settings_missing,",
        "commands::fleet_errors,",
        "commands::budget_rows,",
        "commands::budget_refresh,",
        "commands::budget_save_edit,",
    ] {
        assert!(handler.contains(kept), "{kept} was dropped");
    }
}

#[test]
fn the_reports_command_is_a_thin_call_into_load() {
    let commands = shell("commands.rs");
    let at = commands
        .find("pub fn reports(")
        .expect("commands.rs has pub fn reports(");
    let before = &commands[..at];
    let attr = before.rfind("#[tauri::command]").expect("an attribute");
    assert!(
        !before[attr..].contains("fn "),
        "#[tauri::command] is not on reports"
    );
    let body_end = commands[at..]
        .find("\n}\n")
        .map_or(commands.len(), |i| at + i);
    let body = &commands[at..body_end];
    assert!(body.contains("filter: Option<Filter>"), "{body}");
    assert!(body.contains("reports::load("), "{body}");
    assert!(body.contains("FLEET_STATE_DIR_ENV"), "{body}");
    assert!(body.contains("FaceError::new(\"reports\""), "{body}");
    assert!(body.contains("record_fleet_error("), "{body}");
}

#[test]
fn the_code_map_marks_the_command_present() {
    let map = fs::read_to_string(manifest().join("../../docs/code-map.md")).unwrap();
    assert!(map.contains(
        "- REP-W command (`state_dir_from`, `read_done_cards`, `assemble`, `load`, `reports`): present."
    ));
}
