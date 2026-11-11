//! CORE-2, IT-2 (#375): the fleet errors the app records are kept on disk in the app data
//! folder, one record per line, bounded like the session log; a damaged line is skipped and
//! counted; a write from several processes at once loses no line and tears none. Also the IT
//! tickets the CLI and MCP list beside them.

use std::path::{Path, PathBuf};
use std::process::Command;
use switchboard_core::fleet::{FleetError, FleetErrorReason, FLEET_ERROR_LOG_CAP};
use switchboard_core::fleet_errors::{self, FLEET_ERRORS_FILE};
use switchboard_core::github::{Issue, RepoSnapshot};
use switchboard_core::it::open_tickets;

/// A fresh folder under the system temp folder.
fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sb-errlog-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn error(n: usize, reason: FleetErrorReason) -> FleetError {
    FleetError {
        command: vec!["python3".into(), "fleet.py".into(), format!("call-{n}")],
        exit_code: Some(3),
        stderr: format!("boom {n}\nsecond line\u{1b}[31m"),
        reason,
        undo_note: None,
    }
}

fn lines_on_disk(data: &Path) -> Vec<String> {
    std::fs::read_to_string(data.join(FLEET_ERRORS_FILE))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn each_reason_and_the_undo_note_come_back_as_recorded_newest_first() {
    let data = scratch("roundtrip").join("data-not-made-yet");
    let mut undo = error(4, FleetErrorReason::Exit);
    undo.undo_note = Some("config.json still has cap 40".into());
    let mut killed = error(5, FleetErrorReason::Exit);
    killed.exit_code = None;
    let recorded = [
        error(0, FleetErrorReason::Exit),
        error(
            1,
            FleetErrorReason::BadJson("expected value at line 1".into()),
        ),
        error(2, FleetErrorReason::Spawn("No such file".into())),
        error(3, FleetErrorReason::Timeout(120)),
        undo,
        killed,
    ];
    for err in &recorded {
        fleet_errors::record(&data, err).unwrap();
    }
    assert_eq!(lines_on_disk(&data).len(), recorded.len());
    let loaded = fleet_errors::load(&data).unwrap();
    assert_eq!(loaded.skipped, 0);
    let newest_first: Vec<FleetError> = recorded.iter().rev().cloned().collect();
    assert_eq!(loaded.errors, newest_first);
}

#[test]
fn no_file_is_no_errors() {
    let data = scratch("missing");
    let loaded = fleet_errors::load(&data).unwrap();
    assert!(loaded.errors.is_empty());
    assert_eq!(loaded.skipped, 0);
}

#[test]
fn the_file_keeps_the_last_fifty_like_the_session_log() {
    let data = scratch("cap");
    for n in 0..FLEET_ERROR_LOG_CAP + 12 {
        fleet_errors::record(&data, &error(n, FleetErrorReason::Exit)).unwrap();
    }
    assert_eq!(lines_on_disk(&data).len(), FLEET_ERROR_LOG_CAP);
    let loaded = fleet_errors::load(&data).unwrap();
    assert_eq!(loaded.errors.len(), FLEET_ERROR_LOG_CAP);
    assert_eq!(
        loaded.errors[0].command[2],
        format!("call-{}", FLEET_ERROR_LOG_CAP + 11)
    );
    assert_eq!(loaded.errors[FLEET_ERROR_LOG_CAP - 1].command[2], "call-12");
    let leftovers: Vec<_> = std::fs::read_dir(&data)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != FLEET_ERRORS_FILE)
        .collect();
    assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
}

#[test]
fn damaged_lines_are_skipped_and_counted_and_a_cut_line_does_not_swallow_the_next() {
    let data = scratch("damaged");
    fleet_errors::record(&data, &error(0, FleetErrorReason::Exit)).unwrap();
    let file = data.join(FLEET_ERRORS_FILE);
    let mut bytes = std::fs::read(&file).unwrap();
    bytes.extend_from_slice(b"not json at all\n");
    bytes.extend_from_slice(b"{\"command\":[\"x\"],\"exit_code\":1}\n");
    bytes.extend_from_slice(&[0xff, 0xfe, b'{', b'\n']);
    bytes.extend_from_slice(b"\n   \n");
    // A record cut short by a crash: no closing brace, no newline.
    bytes.extend_from_slice(b"{\"command\":[\"python3\"],\"exit_c");
    std::fs::write(&file, bytes).unwrap();
    fleet_errors::record(&data, &error(1, FleetErrorReason::Timeout(120))).unwrap();
    let loaded = fleet_errors::load(&data).unwrap();
    assert_eq!(loaded.skipped, 4);
    assert_eq!(
        loaded.errors,
        vec![
            error(1, FleetErrorReason::Timeout(120)),
            error(0, FleetErrorReason::Exit)
        ]
    );
}

#[test]
fn a_record_that_cannot_be_written_is_an_error_not_a_panic() {
    let base = scratch("unwritable");
    let data = base.join("data");
    std::fs::write(&data, "a file where the data folder should be").unwrap();
    let err = fleet_errors::record(&data, &error(0, FleetErrorReason::Exit)).unwrap_err();
    assert!(err.to_string().contains("data"), "{err}");
}

#[test]
fn a_file_that_cannot_be_read_is_an_error_not_a_panic() {
    let data = scratch("unreadable");
    std::fs::create_dir_all(data.join(FLEET_ERRORS_FILE)).unwrap();
    assert!(fleet_errors::load(&data).is_err());
}

/// The child the several-process test starts: record `RECORDS` errors into the folder named by
/// the env var. Passes at once when not started as a child.
#[test]
fn record_child() {
    let Ok(data) = std::env::var("SWITCHBOARD_TEST_ERRLOG_CHILD_DATA") else {
        return;
    };
    let index: usize = std::env::var("SWITCHBOARD_TEST_ERRLOG_CHILD_INDEX")
        .unwrap()
        .parse()
        .unwrap();
    for k in 0..RECORDS {
        let mut err = error(k, FleetErrorReason::Exit);
        err.command[2] = format!("child-{index}-{k}");
        // A long stderr, so an interleaved write would show as a torn line.
        err.stderr = format!("{index}-{k} ").repeat(2000);
        fleet_errors::record(Path::new(&data), &err).unwrap();
    }
}

const CHILDREN: usize = 5;
const RECORDS: usize = 12;

#[test]
fn records_from_several_processes_at_once_all_land_whole() {
    let data = scratch("procs");
    let exe = std::env::current_exe().unwrap();
    let children: Vec<_> = (0..CHILDREN)
        .map(|i| {
            Command::new(&exe)
                .args([
                    "--exact",
                    "record_child",
                    "--nocapture",
                    "--test-threads",
                    "1",
                ])
                .env("SWITCHBOARD_TEST_ERRLOG_CHILD_DATA", &data)
                .env("SWITCHBOARD_TEST_ERRLOG_CHILD_INDEX", i.to_string())
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let loaded = fleet_errors::load(&data).unwrap();
    assert_eq!(loaded.skipped, 0, "a line was torn");
    // 60 records into a file kept to 50: the 50 newest survive, none lost to a racing trim.
    assert_eq!(CHILDREN * RECORDS, FLEET_ERROR_LOG_CAP + 10);
    assert_eq!(lines_on_disk(&data).len(), FLEET_ERROR_LOG_CAP);
    assert_eq!(loaded.errors.len(), FLEET_ERROR_LOG_CAP);
    for i in 0..CHILDREN {
        let order: Vec<usize> = loaded
            .errors
            .iter()
            .filter_map(|e| {
                e.command[2]
                    .strip_prefix(&format!("child-{i}-"))?
                    .parse()
                    .ok()
            })
            .collect();
        // Each child's survivors are its newest, in its order (newest first), with no gap.
        let want: Vec<usize> = (RECORDS - order.len()..RECORDS).rev().collect();
        assert_eq!(order, want, "child {i} lost a record");
    }
}

#[test]
fn open_it_tickets_are_listed_by_repo_newest_first() {
    let issue = |number: u64, state: &str, labels: &[&str]| Issue {
        number,
        title: format!("IT: sig {number}"),
        state: state.into(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        ..Default::default()
    };
    let snaps = vec![
        RepoSnapshot {
            repo: "owner/app".into(),
            issues: vec![issue(3, "open", &["bug"])],
            ..Default::default()
        },
        RepoSnapshot {
            repo: "owner/pipeline".into(),
            issues: vec![
                issue(7, "open", &["it"]),
                issue(9, "closed", &["it"]),
                issue(12, "open", &["needs-ian", "it"]),
            ],
            ..Default::default()
        },
    ];
    let tickets = open_tickets(&snaps);
    let shown: Vec<(String, u64, String)> = tickets
        .iter()
        .map(|t| (t.repo.clone(), t.number, t.link.clone()))
        .collect();
    assert_eq!(
        shown,
        vec![
            (
                "owner/pipeline".into(),
                12,
                "https://github.com/owner/pipeline/issues/12".into()
            ),
            (
                "owner/pipeline".into(),
                7,
                "https://github.com/owner/pipeline/issues/7".into()
            ),
        ]
    );
    assert_eq!(tickets[0].title, "IT: sig 12");
    assert_eq!(tickets[0].state, "open");
}
