//! R28 probe for #20: behaviour the issue states that the locked tests in `gh_queue.rs` do not
//! cover. Written by the reviewer, not the builder.

use std::path::{Path, PathBuf};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::queue::{send, Create};
use switchboard_core::CoreError;

const STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(args) + "\n")
sub = args[1] if len(args) > 1 else ""
answer = os.path.join(here, sub + ".json")
print(open(answer).read() if os.path.exists(answer) else "{}")
code = os.path.join(here, "exit-" + sub)
sys.exit(int(open(code).read()) if os.path.exists(code) else 0)
"#;

fn stub_root(tag: &str) -> (PathBuf, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("sb-ghq-probe-{tag}-{}-{nanos}", std::process::id()));
    let dir = root.join("tools").join("fleet");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("fleet.py"), STUB).unwrap();
    (root, dir)
}

fn calls(dir: &Path) -> Vec<Vec<String>> {
    match std::fs::read_to_string(dir.join("calls.log")) {
        Ok(t) => t
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn subcommands(dir: &Path) -> Vec<String> {
    calls(dir).iter().map(|c| c[..2].join(" ")).collect()
}

fn issue() -> Create {
    Create::Issue {
        repo: "o/r".to_string(),
        title: "T".to_string(),
        body: "B".to_string(),
        labels: vec![],
        milestone: None,
    }
}

#[test]
fn the_batch_file_is_removed_even_when_the_add_fails() {
    let (root, dir) = stub_root("addfail-rm");
    std::fs::write(dir.join("exit-add-batch"), "2").unwrap();
    assert!(matches!(
        send(&FleetBridge::new(&root), &[issue()]),
        Err(CoreError::Fleet(_))
    ));
    let file = PathBuf::from(&calls(&dir)[0][2]);
    assert!(!file.exists(), "{} is still there", file.display());
}

#[test]
fn a_failed_status_read_is_a_fleet_error() {
    let (root, dir) = stub_root("status-fail");
    std::fs::write(dir.join("exit-status"), "2").unwrap();
    match send(&FleetBridge::new(&root), &[issue()]) {
        Err(CoreError::Fleet(e)) => assert_eq!(e.exit_code, Some(2)),
        other => panic!("expected a fleet error from status, got {other:?}"),
    }
    assert_eq!(
        subcommands(&dir),
        vec!["ghq add-batch", "ghq run", "ghq status"]
    );
}

#[test]
fn a_status_without_counts_gives_all_zero() {
    let (root, dir) = stub_root("nocounts");
    std::fs::write(dir.join("status.json"), r#"{"total": 0}"#).unwrap();
    let st = send(&FleetBridge::new(&root), &[issue()]).unwrap();
    assert_eq!(st.pending + st.done + st.exists + st.failed, 0);
}
