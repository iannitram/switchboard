// Locked tests for IT-2 (#245): each fleet error the app records hands this session's whole
// fleet error log to `fleet errors file --records -`, on its own thread, and a failed filing is
// never recorded again. The builder never edits this file (R28). `commands.rs` takes it in with
// `#[cfg(test)] #[path = "it_tickets_tests.rs"] mod it_tickets_tests;`.
//
// The stub `fleet.py` is written into a fresh pipeline root per test. It appends
// `{"argv": [...], "stdin": "..."}` (one JSON line per call) to `<root>/calls.jsonl`, writes
// `<root>/stderr.txt` to stderr when it exists, and exits with the number in `<root>/exit.txt`
// (0 when there is none). It reads no env, so the tests can run in parallel. Run them with
// `FLEET_ROOT` unset (it would point the bridge somewhere else).
use super::*;
use serde_json::{json, Value};
use switchboard_core::fleet::FleetErrorReason;
use switchboard_core::settings::Settings;

/// A fresh folder for one test, under the system temp folder.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("switchboard-it2-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A pipeline root holding the stub `tools/fleet/fleet.py`, and a settings file naming it.
/// Returns (root, settings path).
fn stub_pipeline(name: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    let root = dir.join("pipeline");
    let fleet = root.join("tools").join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(
        fleet.join("fleet.py"),
        "import json, os, sys\n\
         root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))\n\
         data = sys.stdin.read()\n\
         with open(os.path.join(root, \"calls.jsonl\"), \"a\") as f:\n\
         \x20   f.write(json.dumps({\"argv\": sys.argv[1:], \"stdin\": data}) + \"\\n\")\n\
         err = os.path.join(root, \"stderr.txt\")\n\
         if os.path.exists(err):\n\
         \x20   sys.stderr.buffer.write(open(err, \"rb\").read())\n\
         \x20   sys.stderr.flush()\n\
         code = os.path.join(root, \"exit.txt\")\n\
         sys.exit(int(open(code).read()) if os.path.exists(code) else 0)\n",
    )
    .unwrap();
    let settings_path = dir.join("settings.json");
    let settings = Settings {
        pipeline_repo: Some(root.clone()),
        nextcloud_backups: None,
        vault_agents: None,
    };
    settings::save(&settings_path, &settings).unwrap();
    (root, settings_path)
}

/// One call the stub recorded: its argv and the stdin it read, parsed as JSON.
struct Call {
    argv: Vec<String>,
    records: Value,
}

/// Each call the stub recorded, in order (empty when it never ran).
fn calls(root: &Path) -> Vec<Call> {
    let text = std::fs::read_to_string(root.join("calls.jsonl")).unwrap_or_default();
    text.lines()
        .map(|line| {
            let v: Value = serde_json::from_str(line).unwrap();
            Call {
                argv: v["argv"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap().to_string())
                    .collect(),
                records: serde_json::from_str(v["stdin"].as_str().unwrap()).unwrap(),
            }
        })
        .collect()
}

/// The argv the bridge gives `fleet.py` for IT-2.
fn records_argv() -> Vec<String> {
    ["errors", "file", "--records", "-", "--json"]
        .iter()
        .map(|a| a.to_string())
        .collect()
}

/// A fleet error from `fleet <cmd>` with `stderr`, exit 3.
fn fleet_error(cmd: &str, stderr: &str) -> FleetError {
    FleetError {
        command: vec![
            "python3".into(),
            "-I".into(),
            "/srv/pipeline/tools/fleet/fleet.py".into(),
            cmd.into(),
            "--json".into(),
        ],
        exit_code: Some(3),
        stderr: stderr.into(),
        reason: FleetErrorReason::Exit,
        undo_note: None,
    }
}

/// The record `fleet errors file --records` reads for `fleet_error(cmd, stderr)`.
fn record(cmd: &str, stderr: &str) -> Value {
    json!({
        "command": ["python3", "-I", "/srv/pipeline/tools/fleet/fleet.py", cmd, "--json"],
        "exit_code": 3,
        "stderr": stderr,
    })
}

/// Record `err` and wait for its filing to finish; the filing's answer.
fn record_and_wait(state: &AppState, err: FleetError) -> Result<usize, CoreError> {
    record_fleet_error(state, &CoreError::Fleet(err))
        .expect("a fleet error starts a filing")
        .join()
        .expect("the filing thread does not panic")
}

#[test]
fn a_first_fleet_error_is_recorded_and_handed_to_fleet_errors_file() {
    let (root, settings_path) = stub_pipeline("first");
    let state = AppState::new(settings_path);
    let filed = record_and_wait(&state, fleet_error("budget", "lock timeout\n"));
    assert_eq!(filed, Ok(1));
    assert_eq!(state.errors.lock().unwrap().errors.len(), 1);
    let got = calls(&root);
    assert_eq!(got.len(), 1, "one errors file run per recorded error");
    assert_eq!(got[0].argv, records_argv());
    assert_eq!(got[0].records, json!([record("budget", "lock timeout\n")]));
}

#[test]
fn the_same_error_again_hands_over_both_records_so_fleet_comments_on_the_one_ticket() {
    let (root, settings_path) = stub_pipeline("repeat");
    let state = AppState::new(settings_path);
    assert_eq!(
        record_and_wait(&state, fleet_error("budget", "lock timeout\n")),
        Ok(1)
    );
    assert_eq!(
        record_and_wait(&state, fleet_error("budget", "lock timeout\n")),
        Ok(2)
    );
    let got = calls(&root);
    assert_eq!(got.len(), 2);
    // Fleet skips the first record (already handed over today) and sees the second as the
    // second identical record: a comment on the one `IT: <signature>` ticket, never a second.
    assert_eq!(got[1].argv, records_argv());
    assert_eq!(
        got[1].records,
        json!([
            record("budget", "lock timeout\n"),
            record("budget", "lock timeout\n")
        ])
    );
}

#[test]
fn a_new_signature_hands_over_the_whole_log_oldest_first() {
    let (root, settings_path) = stub_pipeline("new");
    let state = AppState::new(settings_path);
    record_and_wait(&state, fleet_error("budget", "lock timeout\n")).unwrap();
    record_and_wait(&state, fleet_error("budget", "lock timeout\n")).unwrap();
    assert_eq!(
        record_and_wait(&state, fleet_error("status", "no such lane\n")),
        Ok(3)
    );
    let got = calls(&root);
    assert_eq!(got.len(), 3);
    assert_eq!(
        got[2].records,
        json!([
            record("budget", "lock timeout\n"),
            record("budget", "lock timeout\n"),
            record("status", "no such lane\n")
        ])
    );
}

#[test]
fn an_error_that_is_not_a_fleet_error_is_neither_recorded_nor_filed() {
    let (root, settings_path) = stub_pipeline("not-fleet");
    let state = AppState::new(settings_path);
    assert!(record_fleet_error(&state, &CoreError::Io("disk full".into())).is_none());
    assert!(record_fleet_error(&state, &CoreError::MissingPath("pipeline_repo")).is_none());
    assert!(state.errors.lock().unwrap().errors.is_empty());
    assert!(calls(&root).is_empty());
}

#[test]
fn a_failed_filing_is_its_fleet_error_and_is_not_recorded_again() {
    let (root, settings_path) = stub_pipeline("fail");
    std::fs::write(root.join("exit.txt"), "3").unwrap();
    std::fs::write(root.join("stderr.txt"), "gh: rate limited\n").unwrap();
    let state = AppState::new(settings_path);
    let filed = record_and_wait(&state, fleet_error("budget", "lock timeout\n"));
    match filed {
        Err(CoreError::Fleet(e)) => {
            assert_eq!(e.exit_code, Some(3));
            assert_eq!(e.stderr, "gh: rate limited\n");
            assert_eq!(e.command[e.command.len() - 5..], records_argv()[..]);
        }
        other => panic!("expected the filing's fleet error, got {other:?}"),
    }
    let log = state.errors.lock().unwrap();
    assert_eq!(
        log.errors,
        vec![fleet_error("budget", "lock timeout\n")],
        "only the error itself is in the log; the failed filing is not"
    );
    assert_eq!(calls(&root).len(), 1, "a failed filing is not retried");
}

#[test]
fn with_no_pipeline_repo_set_the_error_is_recorded_and_nothing_runs() {
    let dir = scratch("no-settings");
    let state = AppState::new(dir.join("settings.json"));
    let filed = record_and_wait(&state, fleet_error("budget", "lock timeout\n"));
    assert_eq!(filed, Err(CoreError::MissingPath("pipeline_repo")));
    assert_eq!(state.errors.lock().unwrap().errors.len(), 1);
}

#[test]
fn file_tickets_hands_over_exactly_the_log_it_is_given() {
    let (root, settings_path) = stub_pipeline("direct");
    let mut log = FleetErrorLog::default();
    log.record(fleet_error("budget", "first\n"));
    log.record(fleet_error("pick", "second\n"));
    let before = log.errors.clone();
    assert_eq!(file_tickets(&settings_path, &log), Ok(2));
    assert_eq!(log.errors, before, "the log is only read");
    let got = calls(&root);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].argv, records_argv());
    assert_eq!(
        got[0].records,
        json!([record("budget", "first\n"), record("pick", "second\n")])
    );
}

#[test]
fn file_tickets_with_an_empty_log_runs_nothing() {
    let (root, settings_path) = stub_pipeline("empty");
    assert_eq!(
        file_tickets(&settings_path, &FleetErrorLog::default()),
        Ok(0)
    );
    assert!(calls(&root).is_empty());
}

#[test]
fn the_code_map_marks_it_present() {
    let code_map = include_str!("../../../docs/code-map.md");
    let it = code_map
        .split("\n### ")
        .find(|section| section.starts_with("it\n"))
        .expect("the code map has an `### it` section");
    assert!(
        it.lines().any(|line| line
            == "- IT-2 tickets for the app's fleet errors (`file_tickets`, `record_fleet_error`): present."),
        "the it section lists IT-2's wiring as present"
    );
}
