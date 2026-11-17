//! The headless commands (#357, #374, #373, and the stdout-write part of `refresh` and
//! `backup`): `--role budget-manager set_pool_cap <pool> <cap>`, `pool_edit <pool> [--plan ..]`,
//! `budget --refresh` and `settings`, run as the built binary against the fleet stub under
//! `crates/core/tests/fixtures/fleet-stub/`; `refresh_with` and `backup_with` in-process over a
//! writer that fails.

// The binary crate has no library target, so the sources are compiled in directly.
#[path = "../src/main.rs"]
mod app;
// `cli.rs` reads `super::parse_args`, so it sits at this root beside the parser's import.
#[path = "../src/cli.rs"]
#[allow(dead_code, clippy::duplicate_mod)]
mod cli;

#[allow(unused_imports)]
use app::{parse_args, Args};

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output, Stdio};

use serde_json::{json, Value};
use switchboard_core::backup::{BackupOutcome, Manifest};
use switchboard_core::clock::FixedClock;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::RepoSnapshot;
use switchboard_core::settings::Settings;
use switchboard_core::waiting::{LocalItems, Source};
use switchboard_core::CoreError;

const CONFIG: &str = r#"{
  "version": 1,
  "pools": {
    "claude": { "label": "Claude", "kind": "weekly", "plan": "max-1x" },
    "deepseek": { "label": "DeepSeek", "kind": "fixed-period", "cap_usd": 10 }
  }
}
"#;

fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("sb-headless-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A pipeline checkout with the fleet stub and CONFIG under `dir`.
fn pipeline(dir: &Path) -> PathBuf {
    let root = dir.join("pipeline");
    let stub = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/fleet-stub");
    for sub in ["tools/fleet", "answers"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    std::fs::copy(
        stub.join("tools/fleet/fleet.py"),
        root.join("tools/fleet/fleet.py"),
    )
    .unwrap();
    std::fs::copy(
        stub.join("answers/budget.json"),
        root.join("answers/budget.json"),
    )
    .unwrap();
    std::fs::write(root.join("tools/fleet/config.json"), CONFIG).unwrap();
    root
}

fn config(root: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root.join("tools/fleet/config.json")).unwrap())
        .unwrap()
}

fn unchanged(root: &Path) -> bool {
    config(root) == serde_json::from_str::<Value>(CONFIG).unwrap()
}

/// The folders one binary run uses: a pipeline, an app data folder, a config folder and a cwd
/// that is not a job worktree.
struct Dirs {
    root: PathBuf,
    data: PathBuf,
    config: PathBuf,
    cwd: PathBuf,
}

fn dirs(tag: &str) -> Dirs {
    let base = scratch(tag);
    let dirs = Dirs {
        root: pipeline(&base),
        data: base.join("data"),
        config: base.join("config"),
        cwd: base.join("cwd"),
    };
    for dir in [&dirs.data, &dirs.config, &dirs.cwd] {
        std::fs::create_dir_all(dir).unwrap();
    }
    dirs
}

/// The built binary with every folder it reads set to `dirs`, run in `cwd`, stdout to `stdout`.
fn run_in(argv: &[&str], dirs: &Dirs, cwd: &Path, stdout: Stdio) -> Output {
    Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(argv)
        .current_dir(cwd)
        .env("SWITCHBOARD_PIPELINE", &dirs.root)
        .env("SWITCHBOARD_DATA", &dirs.data)
        .env("SWITCHBOARD_CONFIG", &dirs.config)
        .env("SWITCHBOARD_STATE", dirs.data.join("no-state"))
        .env("SWITCHBOARD_DEV_ROOT", dirs.data.join("no-dev-root"))
        .env_remove("FLEET_ROOT")
        .env_remove("FLEET_STATE_DIR")
        .env_remove("FLEET_STUB_EXIT")
        .env_remove("FLEET_STUB_BADJSON")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::piped())
        .output()
        .expect("run switchboard")
}

fn run(argv: &[&str], dirs: &Dirs) -> Output {
    run_in(argv, dirs, &dirs.cwd, Stdio::piped())
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn ledger(data: &Path) -> Vec<Value> {
    std::fs::read_to_string(data.join("pool-edits.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

const BM: [&str; 2] = ["--role", "budget-manager"];

fn with_role(rest: &[&'static str]) -> Vec<&'static str> {
    BM.iter().chain(rest).copied().collect()
}

// --- set_pool_cap (#357) ---

#[test]
fn set_pool_cap_saves_the_cap_and_says_what_it_was() {
    let d = dirs("cap");
    let out = run(&with_role(&["set_pool_cap", "deepseek", "25"]), &d);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "set_pool_cap deepseek: cap_usd is now 25.0 (was 10)\n"
    );
    assert_eq!(config(&d.root)["pools"]["deepseek"]["cap_usd"], json!(25.0));
    assert_eq!(ledger(&d.data).len(), 1);
    let items = LocalItems::new(&d.data).load().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].source,
        Source::PoolEdit {
            pool: "deepseek".into()
        }
    );
}

#[test]
fn set_pool_cap_json_is_the_ledger_row_and_the_budget_rows() {
    let d = dirs("cap-json");
    let out = run(
        &with_role(&["--json", "set_pool_cap", "deepseek", "7.5"]),
        &d,
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let answer: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(answer["edit"]["pool"], "deepseek");
    assert_eq!(answer["edit"]["set"], json!({ "cap_usd": 7.5 }));
    assert_eq!(answer["edit"]["before"], json!({ "cap_usd": 10 }));
    assert!(answer["budget"].is_array(), "{answer}");
}

#[test]
fn set_pool_cap_is_refused_without_the_budget_manager_role() {
    let d = dirs("cap-role");
    for argv in [
        vec!["set_pool_cap", "deepseek", "25"],
        vec![
            "--role",
            "project-manager",
            "set_pool_cap",
            "deepseek",
            "25",
        ],
        vec!["--role", "satellite", "set_pool_cap", "deepseek", "25"],
    ] {
        let out = run(&argv, &d);
        assert_eq!(out.status.code(), Some(1), "{argv:?}");
        assert_eq!(
            stderr(&out),
            "edit pool deepseek: only the budget-manager role may edit a pool\n",
            "{argv:?}"
        );
        assert_eq!(stdout(&out), "");
    }
    assert!(unchanged(&d.root));
    assert!(ledger(&d.data).is_empty());
    assert!(!LocalItems::new(&d.data).path().exists());
}

#[test]
fn set_pool_cap_is_refused_inside_a_job_worktree() {
    let d = dirs("cap-worktree");
    let worktree = d.cwd.join("switchboard-wt").join("fleet-12");
    std::fs::create_dir_all(&worktree).unwrap();
    let out = run_in(
        &with_role(&["set_pool_cap", "deepseek", "25"]),
        &d,
        &worktree,
        Stdio::piped(),
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "edit pool deepseek: refused inside a job worktree\n"
    );
    assert!(unchanged(&d.root));
}

#[test]
fn a_bad_set_pool_cap_line_is_exit_2_and_changes_nothing() {
    let d = dirs("cap-bad");
    for argv in [
        with_role(&["set_pool_cap"]),
        with_role(&["set_pool_cap", "deepseek"]),
        with_role(&["set_pool_cap", "deepseek", "lots"]),
        with_role(&["set_pool_cap", "deepseek", "25", "more"]),
        with_role(&["set_pool_cap", "deepseek", "25", "--refresh"]),
        with_role(&["set_pool_cap", "deepseek", "25", "--plan", "max-5x"]),
    ] {
        let out = run(&argv, &d);
        assert_eq!(out.status.code(), Some(2), "{argv:?}: {}", stderr(&out));
        assert!(stderr(&out).contains("usage: switchboard"), "{argv:?}");
    }
    assert!(unchanged(&d.root));
    assert!(ledger(&d.data).is_empty());
}

#[test]
fn a_cap_the_core_refuses_is_exit_1_and_changes_nothing() {
    let d = dirs("cap-core");
    let out = run(&with_role(&["set_pool_cap", "nope", "5"]), &d);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "set_pool_cap nope: parse: no pool nope in config.json\n"
    );
    assert!(unchanged(&d.root));
    assert!(ledger(&d.data).is_empty());
}

#[test]
fn a_waiting_item_that_cannot_be_added_leaves_the_pool_as_it_was() {
    // #340 from the CLI: `waiting/local.json` is a folder, so the edit must not land.
    let d = dirs("cap-no-waiting");
    std::fs::create_dir_all(LocalItems::new(&d.data).path()).unwrap();
    let out = run(&with_role(&["set_pool_cap", "deepseek", "25"]), &d);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).starts_with("set_pool_cap deepseek: "),
        "{}",
        stderr(&out)
    );
    assert!(unchanged(&d.root));
    assert!(ledger(&d.data).is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn a_saved_cap_whose_output_cannot_be_written_is_exit_1() {
    let d = dirs("cap-full");
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let out = run_in(
        &with_role(&["set_pool_cap", "deepseek", "25"]),
        &d,
        &d.cwd,
        Stdio::from(full),
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("cannot write the output"),
        "{}",
        stderr(&out)
    );
    assert!(!stderr(&out).contains("panicked"));
}

// --- pool_edit (#374) ---

#[test]
fn pool_edit_saves_the_plan_and_the_reset_and_says_each() {
    let d = dirs("edit");
    let out = run(
        &with_role(&[
            "pool_edit",
            "claude",
            "--plan",
            "max-5x",
            "--reset",
            "Sun 6:00am",
        ]),
        &d,
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "pool_edit claude: plan is now max-5x (was max-1x), pace.reset_weekday is now sun \
         (was not set), pace.reset_hour is now 6 (was not set), pace.reset_minute is now 0 \
         (was not set)\n"
    );
    assert_eq!(config(&d.root)["pools"]["claude"]["plan"], "max-5x");
    assert_eq!(LocalItems::new(&d.data).load().unwrap().len(), 1);
}

#[test]
fn pool_edit_json_is_the_ledger_row_and_the_budget_rows() {
    let d = dirs("edit-json");
    let out = run(
        &with_role(&["pool_edit", "deepseek", "--cap", "12", "--json"]),
        &d,
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let answer: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(answer["edit"]["set"], json!({ "cap_usd": 12.0 }));
    assert!(answer["budget"].is_array(), "{answer}");
}

#[test]
fn pool_edit_is_refused_without_the_budget_manager_role() {
    let d = dirs("edit-role");
    for argv in [
        vec!["pool_edit", "claude", "--plan", "max-5x"],
        vec![
            "--role",
            "project-manager",
            "pool_edit",
            "claude",
            "--plan",
            "max-5x",
        ],
    ] {
        let out = run(&argv, &d);
        assert_eq!(out.status.code(), Some(1), "{argv:?}");
        assert_eq!(
            stderr(&out),
            "edit pool claude: only the budget-manager role may edit a pool\n"
        );
    }
    assert!(unchanged(&d.root));
}

#[test]
fn a_bad_pool_edit_line_is_exit_2_and_changes_nothing() {
    let d = dirs("edit-bad");
    for argv in [
        with_role(&["pool_edit"]),
        with_role(&["pool_edit", "claude"]),
        with_role(&["pool_edit", "claude", "deepseek", "--plan", "max-5x"]),
        with_role(&["pool_edit", "claude", "--plan"]),
        with_role(&["pool_edit", "claude", "--cap", "lots"]),
        with_role(&["pool_edit", "claude", "--cost", "--plan", "max-5x"]),
        with_role(&["budget", "--plan", "max-5x"]),
    ] {
        let out = run(&argv, &d);
        assert_eq!(out.status.code(), Some(2), "{argv:?}: {}", stderr(&out));
        assert_eq!(stdout(&out), "", "{argv:?}");
    }
    assert!(unchanged(&d.root));
    assert!(ledger(&d.data).is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn a_pool_edit_whose_output_cannot_be_written_is_exit_1() {
    let d = dirs("edit-full");
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let out = run_in(
        &with_role(&["pool_edit", "claude", "--plan", "max-5x"]),
        &d,
        &d.cwd,
        Stdio::from(full),
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("cannot write the output"));
}

// --- budget --refresh (#374) ---

#[test]
fn budget_refresh_prints_the_rows_read_again() {
    let d = dirs("refresh");
    let plain = run(&["budget"], &d);
    let fresh = run(&["budget", "--refresh"], &d);
    assert_eq!(fresh.status.code(), Some(0), "{}", stderr(&fresh));
    assert_eq!(stdout(&fresh), stdout(&plain));
    let json = run(&["--json", "budget", "--refresh"], &d);
    assert_eq!(json.status.code(), Some(0), "{}", stderr(&json));
    assert!(serde_json::from_str::<Value>(&stdout(&json))
        .unwrap()
        .is_array());
}

#[test]
fn budget_refresh_asks_fleet_to_re_read_and_plain_budget_does_not() {
    // The stub answers both the same way, so the argv it was given is the evidence.
    let d = dirs("refresh-argv");
    let calls = d.data.join("fleet-calls.txt");
    std::fs::write(
        d.root.join("tools/fleet/fleet.py"),
        format!(
            "import pathlib, sys\n\
             here = pathlib.Path(__file__).resolve().parent\n\
             with open({calls:?}, 'a') as f:\n    f.write(' '.join(sys.argv[1:]) + '\\n')\n\
             sys.stdout.write((here.parent.parent / 'answers' / 'budget.json').read_text())\n"
        ),
    )
    .unwrap();
    assert_eq!(run(&["budget"], &d).status.code(), Some(0));
    assert_eq!(run(&["budget", "--refresh"], &d).status.code(), Some(0));
    let calls = std::fs::read_to_string(&calls).unwrap();
    let calls: Vec<&str> = calls.lines().collect();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(!calls[0].contains("--refresh"), "{calls:?}");
    assert!(calls[1].starts_with("budget --refresh"), "{calls:?}");
}

#[test]
fn budget_refresh_needs_no_role_and_is_refused_on_other_commands() {
    let d = dirs("refresh-bad");
    let out = run(&["waiting", "--refresh"], &d);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--refresh is for budget"));
    let out = run(&["budget", "--refresh", "extra"], &d);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn budget_refresh_that_fleet_fails_is_exit_1() {
    let d = dirs("refresh-fail");
    std::fs::remove_file(d.root.join("answers/budget.json")).unwrap();
    let out = run(&["budget", "--refresh"], &d);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).starts_with("switchboard budget: "),
        "{}",
        stderr(&out)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn budget_refresh_whose_output_cannot_be_written_is_exit_1() {
    let d = dirs("refresh-full");
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let out = run_in(&["budget", "--refresh"], &d, &d.cwd, Stdio::from(full));
    assert_eq!(out.status.code(), Some(1));
}

// --- settings (#373) ---

#[test]
fn settings_prints_each_setting_or_not_set() {
    let d = dirs("settings");
    let out = run(&["settings"], &d);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "pipeline_repo: not set\nnextcloud_backups: not set\nvault_agents: not set\n"
    );
    let repo = d.data.join("a-pipeline");
    std::fs::write(
        d.config.join("settings.json"),
        json!({ "pipeline_repo": repo }).to_string(),
    )
    .unwrap();
    let out = run(&["settings"], &d);
    assert_eq!(
        stdout(&out),
        format!(
            "pipeline_repo: {}\nnextcloud_backups: not set\nvault_agents: not set\n",
            repo.display()
        )
    );
}

#[test]
fn settings_json_is_the_settings_and_the_names_not_set() {
    let d = dirs("settings-json");
    let repo = d.data.join("a-pipeline");
    std::fs::write(
        d.config.join("settings.json"),
        json!({ "pipeline_repo": repo }).to_string(),
    )
    .unwrap();
    let out = run(&["settings", "--json"], &d);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let answer: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(answer["settings"]["pipeline_repo"], json!(repo));
    assert_eq!(
        answer["missing"],
        json!(["nextcloud_backups", "vault_agents"])
    );
}

#[test]
fn settings_that_do_not_read_or_extra_words_are_errors() {
    let d = dirs("settings-bad");
    let out = run(&["settings", "pipeline_repo"], &d);
    assert_eq!(out.status.code(), Some(2));
    std::fs::write(d.config.join("settings.json"), "not json").unwrap();
    let out = run(&["settings"], &d);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).starts_with("switchboard settings: "),
        "{}",
        stderr(&out)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn settings_whose_output_cannot_be_written_is_exit_1() {
    let d = dirs("settings-full");
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let out = run_in(&["settings"], &d, &d.cwd, Stdio::from(full));
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("cannot write the output"));
}

// --- refresh and backup over a stdout that fails ---

/// A writer that refuses every write, as a full disk does.
struct Full;

impl Write for Full {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("no space left"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Every repo answers 304, so a refresh keeps each snapshot.
struct Same;

impl GitHubTransport for Same {
    fn graphql(&self, _: &str, _: &Value) -> Result<Value, CoreError> {
        Err(CoreError::GitHub("not asked".to_string()))
    }
    fn rest_get(&self, _: &str, _: Option<&str>) -> Result<RestAnswer, CoreError> {
        Ok(RestAnswer {
            status: 304,
            etag: Some("\"same\"".to_string()),
            body: Value::Null,
        })
    }
}

fn snapshot(data: &Path, repo: &str) {
    let snap = RepoSnapshot {
        repo: repo.to_string(),
        read_at: 1,
        etag: Some("\"same\"".to_string()),
        ..Default::default()
    };
    SnapshotStore::new(data).save(&snap).unwrap();
}

#[test]
fn refresh_whose_output_cannot_be_written_is_exit_1() {
    let data = scratch("refresh-out");
    snapshot(&data, "a/one");
    snapshot(&data, "a/two");
    for json in [false, true] {
        let mut err = Vec::new();
        let code = cli::refresh_with(&Same, &data, &FixedClock::at(5), json, &mut Full, &mut err);
        assert_eq!(code, ExitCode::from(1), "json {json}");
    }
}

#[test]
fn backup_whose_output_cannot_be_written_is_exit_1() {
    let data = scratch("backup-out");
    let backups = scratch("backup-out-b");
    let settings = Settings {
        nextcloud_backups: Some(backups),
        ..Settings::default()
    };
    let back_up = |repo: &str| -> Result<BackupOutcome, CoreError> {
        Ok(BackupOutcome::Written(Manifest {
            repo: repo.to_string(),
            ..Manifest::default()
        }))
    };
    let mut err = Vec::new();
    let code = cli::backup_with(
        &settings,
        &data,
        &["a/one".to_string()],
        5,
        &back_up,
        &mut Full,
        &mut err,
    );
    assert_eq!(code, ExitCode::from(1));
}

// --- MCP reports filters (#358) ---

/// Three done cards: two projects, two outcomes, two pools.
const CARDS: &str = r#"[
  {"id": "job-a1", "title": "Add the settings screen", "repo": "owner/app-a", "outcome": "passed", "lane": "qwen", "at": "2026-11-06T13:00:43-07:00"},
  {"id": "job-a2", "title": "Fix the sync retry", "repo": "owner/app-a", "outcome": "failed", "lane": "gemini", "at": "2026-11-05T09:00:00-07:00"},
  {"id": "job-b1", "title": "Tidy the budget copy", "repo": "owner/app-b", "outcome": "passed", "lane": "gemini", "at": "2026-11-07T08:00:00-07:00"}
]"#;

/// `dirs` with CARDS in `<data>/fleet-state/jobs/done`.
fn report_dirs(tag: &str) -> Dirs {
    let d = dirs(tag);
    let done = d.data.join("fleet-state").join("jobs").join("done");
    std::fs::create_dir_all(&done).unwrap();
    let cards: Vec<Value> = serde_json::from_str(CARDS).unwrap();
    for card in cards {
        let id = card["id"].as_str().unwrap();
        std::fs::write(done.join(format!("{id}.json")), card.to_string()).unwrap();
    }
    d
}

/// The binary with `FLEET_STATE_DIR` at the cards and `lines` on stdin.
fn reports_run(argv: &[&str], d: &Dirs, lines: &[String]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(argv)
        .current_dir(&d.cwd)
        .env("SWITCHBOARD_PIPELINE", &d.root)
        .env("SWITCHBOARD_DATA", &d.data)
        .env("SWITCHBOARD_CONFIG", &d.config)
        .env("SWITCHBOARD_STATE", &d.data)
        .env("FLEET_STATE_DIR", d.data.join("fleet-state"))
        .env_remove("FLEET_ROOT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn switchboard");
    let mut stdin = child.stdin.take().unwrap();
    for line in lines {
        let _ = writeln!(stdin, "{line}");
    }
    drop(stdin);
    child.wait_with_output().unwrap()
}

fn ids(reports: &Value) -> Vec<String> {
    reports
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn mcp_reports_takes_the_cli_filters_and_answers_the_same_reports() {
    let d = report_dirs("mcp-reports");
    let cases: [(Value, [&str; 2]); 3] = [
        (
            json!({ "project": "owner/app-a" }),
            ["--project", "owner/app-a"],
        ),
        (json!({ "outcome": "passed" }), ["--outcome", "passed"]),
        (json!({ "pool": "gemini" }), ["--pool", "gemini"]),
    ];
    for (i, (args, flags)) in cases.iter().enumerate() {
        let cli = reports_run(&["--json", "reports", flags[0], flags[1]], &d, &[]);
        assert_eq!(cli.status.code(), Some(0), "{}", stderr(&cli));
        let cli: Value = serde_json::from_str(&stdout(&cli)).unwrap();
        let request = json!({ "jsonrpc": "2.0", "id": i, "method": "tools/call",
                              "params": { "name": "reports", "arguments": args } });
        let out = reports_run(&["mcp"], &d, &[request.to_string()]);
        let reply: Value = serde_json::from_str(stdout(&out).lines().next().unwrap()).unwrap();
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        let mcp: Value = serde_json::from_str(text).unwrap();
        assert_eq!(ids(&mcp), ids(&cli), "{args}");
        assert!(ids(&mcp).len() < 3, "{args} narrowed nothing");
    }
}

#[test]
fn mcp_reports_refuses_a_filter_that_is_not_a_string() {
    let d = report_dirs("mcp-reports-bad");
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                          "params": { "name": "reports", "arguments": { "pool": 5 } } });
    let out = reports_run(&["mcp"], &d, &[request.to_string()]);
    let reply: Value = serde_json::from_str(stdout(&out).lines().next().unwrap()).unwrap();
    assert_eq!(reply["error"]["code"], -32602, "{reply}");
}
