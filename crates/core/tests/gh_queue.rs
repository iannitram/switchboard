//! Locked tests for #20 (GH-4: creates go through `fleet ghq`). Qwen never edits this file (R28).
//! Each test writes its own stub `fleet.py` into a temp pipeline root. The stub logs its
//! arguments, copies the batch file it was given, answers `<subcommand>.json` and exits with
//! the code in `exit-<subcommand>` (0 when there is none).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::queue::{send, Create, QueueStatus};
use switchboard_core::CoreError;

const STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(args) + "\n")
sub = args[1] if len(args) > 1 else ""
if sub == "add-batch":
    with open(args[2]) as b, open(os.path.join(here, "batch.json"), "w") as o:
        o.write(b.read())
answer = os.path.join(here, sub + ".json")
print(open(answer).read() if os.path.exists(answer) else "{}")
code = os.path.join(here, "exit-" + sub)
sys.exit(int(open(code).read()) if os.path.exists(code) else 0)
"#;

/// A temp pipeline root with the stub at `tools/fleet/fleet.py`. Returns (root, stub dir).
fn stub_root(tag: &str) -> (PathBuf, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("sb-ghq-{tag}-{}-{nanos}", std::process::id()));
    let dir = root.join("tools").join("fleet");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("fleet.py"), STUB).unwrap();
    (root, dir)
}

/// Each logged call's arguments.
fn calls(dir: &Path) -> Vec<Vec<String>> {
    match std::fs::read_to_string(dir.join("calls.log")) {
        Ok(t) => t
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// The first two arguments of each call (`ghq <subcommand>`).
fn subcommands(dir: &Path) -> Vec<String> {
    calls(dir).iter().map(|c| c[..2].join(" ")).collect()
}

fn issue() -> Create {
    Create::Issue {
        repo: "o/r".to_string(),
        title: "T".to_string(),
        body: "B".to_string(),
        labels: vec!["x".to_string()],
        milestone: None,
    }
}

fn comment() -> Create {
    Create::Comment {
        repo: "o/r".to_string(),
        number: 7,
        body: "hi".to_string(),
        marker: None,
    }
}

#[test]
fn nothing_to_send_runs_no_fleet_command() {
    let (root, dir) = stub_root("empty");
    let st = send(&FleetBridge::new(&root), &[]).unwrap();
    assert_eq!(st, QueueStatus::default());
    assert!(calls(&dir).is_empty());
}

#[test]
fn send_adds_one_batch_then_runs_then_reads_the_status() {
    let (root, dir) = stub_root("order");
    send(&FleetBridge::new(&root), &[issue(), comment()]).unwrap();
    assert_eq!(
        subcommands(&dir),
        vec!["ghq add-batch", "ghq run", "ghq status"]
    );
    assert!(!calls(&dir)[0].contains(&"--coordinator".to_string()));
}

#[test]
fn the_batch_file_holds_the_creates_in_the_keys_ghq_reads() {
    let (root, dir) = stub_root("batch");
    send(&FleetBridge::new(&root), &[issue(), comment()]).unwrap();
    let batch: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("batch.json")).unwrap()).unwrap();
    assert_eq!(
        batch,
        json!([
            {"kind": "issue", "repo": "o/r", "title": "T", "body": "B", "labels": ["x"], "milestone": null},
            {"kind": "comment", "repo": "o/r", "number": 7, "body": "hi", "marker": null}
        ])
    );
}

#[test]
fn the_batch_file_is_removed_after_the_add() {
    let (root, dir) = stub_root("tmpfile");
    send(&FleetBridge::new(&root), &[issue()]).unwrap();
    let file = PathBuf::from(&calls(&dir)[0][2]);
    assert!(!file.exists(), "{} is still there", file.display());
}

#[test]
fn a_label_or_a_milestone_adds_coordinator() {
    let label = Create::Label {
        repo: "o/r".to_string(),
        name: "size:easy".to_string(),
        color: None,
        description: None,
    };
    let milestone = Create::Milestone {
        repo: "o/r".to_string(),
        title: "1.0".to_string(),
        description: None,
        due: None,
    };
    for create in [label, milestone] {
        let (root, dir) = stub_root("coord");
        send(&FleetBridge::new(&root), &[issue(), create]).unwrap();
        assert!(calls(&dir)[0].contains(&"--coordinator".to_string()));
    }
}

#[test]
fn the_status_comes_from_ghq_status_counts_and_a_missing_count_is_zero() {
    let (root, dir) = stub_root("counts");
    std::fs::write(
        dir.join("status.json"),
        r#"{"counts": {"pending": 2, "done": 3, "exists": 1}, "total": 6}"#,
    )
    .unwrap();
    let st = send(&FleetBridge::new(&root), &[issue()]).unwrap();
    assert_eq!(
        st,
        QueueStatus {
            pending: 2,
            done: 3,
            exists: 1,
            failed: 0
        }
    );
}

#[test]
fn a_run_that_stopped_on_a_rate_limit_or_had_a_failed_item_is_not_an_error() {
    for code in ["75", "1"] {
        let (root, dir) = stub_root("limit");
        std::fs::write(dir.join("exit-run"), code).unwrap();
        std::fs::write(
            dir.join("status.json"),
            r#"{"counts": {"pending": 4, "done": 0, "exists": 0, "failed": 1}}"#,
        )
        .unwrap();
        let st = send(&FleetBridge::new(&root), &[issue()]).unwrap();
        assert_eq!(st.pending, 4);
        assert_eq!(st.failed, 1);
        assert_eq!(subcommands(&dir).len(), 3);
    }
}

#[test]
fn any_other_run_exit_is_a_fleet_error_and_reads_no_status() {
    let (root, dir) = stub_root("runfail");
    std::fs::write(dir.join("exit-run"), "2").unwrap();
    match send(&FleetBridge::new(&root), &[issue()]) {
        Err(CoreError::Fleet(e)) => assert_eq!(e.exit_code, Some(2)),
        other => panic!("expected a fleet error, got {other:?}"),
    }
    assert_eq!(subcommands(&dir), vec!["ghq add-batch", "ghq run"]);
}

#[test]
fn a_failed_add_is_a_fleet_error_and_runs_nothing() {
    let (root, dir) = stub_root("addfail");
    std::fs::write(dir.join("exit-add-batch"), "2").unwrap();
    assert!(matches!(
        send(&FleetBridge::new(&root), &[issue()]),
        Err(CoreError::Fleet(_))
    ));
    assert_eq!(subcommands(&dir), vec!["ghq add-batch"]);
}

fn repo(private: bool) -> Create {
    Create::Repo {
        repo: "o/new".to_string(),
        private,
    }
}

#[test]
fn a_repo_item_serialises_with_kind_repo() {
    for private in [true, false] {
        let (root, dir) = stub_root("repo-batch");
        send(&FleetBridge::new(&root), &[repo(private)]).unwrap();
        let batch: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("batch.json")).unwrap())
                .unwrap();
        assert_eq!(
            batch,
            json!([{"kind": "repo", "repo": "o/new", "private": private}])
        );
    }
}

#[test]
fn a_repo_item_goes_out_with_coordinator() {
    let (root, dir) = stub_root("repo-coord");
    send(&FleetBridge::new(&root), &[issue(), repo(true)]).unwrap();
    assert!(calls(&dir)[0].contains(&"--coordinator".to_string()));
}

#[test]
fn a_repo_status_with_an_exists_count_is_exists_not_an_error() {
    let (root, dir) = stub_root("repo-exists");
    std::fs::write(
        dir.join("status.json"),
        r#"{"counts": {"pending": 0, "done": 0, "exists": 1, "failed": 0}, "total": 1}"#,
    )
    .unwrap();
    let st = send(&FleetBridge::new(&root), &[repo(true)])
        .expect("an exists answer is not a fleet error");
    assert_eq!(
        st,
        QueueStatus {
            pending: 0,
            done: 0,
            exists: 1,
            failed: 0
        }
    );
    assert_eq!(
        subcommands(&dir),
        vec!["ghq add-batch", "ghq run", "ghq status"]
    );
}
