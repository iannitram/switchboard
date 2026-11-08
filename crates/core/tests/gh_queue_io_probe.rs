//! R28 probe for #20, io case on its own binary: TMPDIR is changed here, so no other test may run.

use std::path::{Path, PathBuf};
use switchboard_core::fleet::FleetBridge;
use switchboard_core::github::queue::{send, Create};
use switchboard_core::CoreError;

const STUB: &str = "import sys\n";

fn stub_root(tag: &str) -> (PathBuf, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("sb-ghq-io-{tag}-{}-{nanos}", std::process::id()));
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
fn a_batch_file_that_cannot_be_written_is_an_io_error_and_runs_nothing() {
    // Own test binary with one test, so changing TMPDIR cannot race another test.
    let (root, dir) = stub_root("io");
    let missing = root.join("no-such-dir");
    std::env::set_var("TMPDIR", &missing);
    let res = send(&FleetBridge::new(&root), &[issue()]);
    std::env::remove_var("TMPDIR");
    match res {
        Err(CoreError::Io(why)) => assert!(why.contains("switchboard-ghq-"), "{why}"),
        other => panic!("expected an io error, got {other:?}"),
    }
    assert!(calls(&dir).is_empty());
}
