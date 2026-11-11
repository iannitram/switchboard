//! #385: `switchboard autopilot <pass> [--dry-run] [--json]` and `switchboard autopilot
//! status`. The built binary runs against a stub `fleet.py`; the merge and the weekly run,
//! which read GitHub and write a backup, run in-process (`command_with`) over fakes.

// The binary crate has no library target, so the sources are compiled in directly.
#[path = "../src/main.rs"]
mod app;
mod autopilot_support;

use std::path::Path;
use std::process::{Command, ExitCode, Output, Stdio};
use std::time::Duration;

use app::autopilot::{command_with, Headless};
use autopilot_support::{fake_backup, fake_github, headless, world, Seams, World};
use serde_json::Value;
use switchboard_core::autopilot::run::{passes_lock_target, satellite_lock_target};
use switchboard_core::lock::{self, FileLock};

const PM: [&str; 2] = ["--role", "project-manager"];

/// The built binary with every folder it reads set to `w`, run in `cwd`.
fn run_in(argv: &[&str], w: &World, cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(argv)
        .current_dir(cwd)
        .env("SWITCHBOARD_PIPELINE", &w.pipeline)
        .env("SWITCHBOARD_DATA", &w.data)
        .env("SWITCHBOARD_CONFIG", &w.config)
        .env("SWITCHBOARD_STATE", w.base.join("no-state"))
        .env("SWITCHBOARD_DEV_ROOT", &w.dev)
        .env("FLEET_MACHINE", "test-box")
        .env_remove("FLEET_ROOT")
        .env_remove("FLEET_STATE_DIR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run switchboard")
}

fn run(argv: &[&str], w: &World) -> Output {
    run_in(argv, w, &w.cwd)
}

/// `--role <the pass's role> autopilot <pass> <more>`.
fn pass(name: &str, more: &[&str], w: &World) -> Output {
    let role = if name == "satellite" {
        "satellite"
    } else {
        "project-manager"
    };
    let mut argv = vec!["--role", role, "autopilot", name];
    argv.extend(more);
    run(&argv, w)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn code(out: &Output) -> Option<i32> {
    out.status.code()
}

/// `command_with` over a test's headless core, outside a job worktree: the exit code as
/// text, stdout and stderr.
fn in_process(argv: &[&str], headless: &Headless<'_>) -> (String, String, String) {
    let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = command_with(&argv, headless, false, &mut out, &mut err);
    (
        format!("{code:?}"),
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn exit(code: u8) -> String {
    format!("{:?}", Some(ExitCode::from(code)))
}

// ---- the command line ----

#[test]
fn a_missing_or_unknown_pass_is_a_usage_error() {
    let w = world("usage");
    let before = w.files();
    for (argv, says) in [
        (vec!["autopilot"], "autopilot needs a pass"),
        (
            vec!["--role", "project-manager", "autopilot", "sweep"],
            "unknown autopilot pass: sweep",
        ),
        (
            vec!["autopilot", "merge", "now"],
            "autopilot takes one word, got also now",
        ),
        (
            vec!["autopilot", "status", "--dry-run"],
            "--dry-run is for a pass",
        ),
    ] {
        let out = run(&argv, &w);
        assert_eq!(code(&out), Some(2), "{argv:?}: {}", stderr(&out));
        assert!(stderr(&out).contains(says), "{argv:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("usage: switchboard"),
            "{}",
            stderr(&out)
        );
        assert_eq!(stdout(&out), "");
    }
    assert_eq!(w.files(), before);
    assert!(w.calls("fleet").is_empty());
}

#[test]
fn the_usage_line_names_the_autopilot_command() {
    let w = world("help");
    let out = run(&["--help"], &w);
    assert_eq!(code(&out), Some(0));
    let text = stdout(&out);
    assert!(
        text.contains("autopilot <roadmap | review | merge | weekly | satellite> [--dry-run]"),
        "{text}"
    );
    assert!(text.contains("autopilot status"), "{text}");
}

#[test]
fn a_pass_needs_its_role_and_is_refused_in_a_job_worktree() {
    let w = world("role");
    let before = w.files();
    let pm = "only the project-manager role may run this pass";
    let sat = "autopilot satellite: only the satellite role may run this pass";
    for (argv, says) in [
        (vec!["autopilot", "merge"], pm),
        (vec!["autopilot", "merge", "--dry-run"], pm),
        (vec!["--role", "satellite", "autopilot", "review"], pm),
        (vec!["--role", "budget-manager", "autopilot", "weekly"], pm),
        (
            vec!["--role", "project-manager", "autopilot", "satellite"],
            sat,
        ),
        (vec!["autopilot", "satellite", "--dry-run"], sat),
    ] {
        let out = run(&argv, &w);
        assert_eq!(code(&out), Some(1), "{argv:?}: {}", stderr(&out));
        assert!(stderr(&out).contains(says), "{argv:?}: {}", stderr(&out));
        assert_eq!(stdout(&out), "");
    }
    // Inside a job worktree even the right role is refused.
    let job = w.base.join("demo-wt").join("fleet-job-1");
    std::fs::create_dir_all(&job).unwrap();
    let out = run_in(&[PM[0], PM[1], "autopilot", "merge"], &w, &job);
    assert_eq!(code(&out), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("autopilot merge: refused inside a job worktree"),
        "{}",
        stderr(&out)
    );
    assert_eq!(w.files(), before);
    assert!(w.calls("fleet").is_empty() && w.calls("merge-pr").is_empty());
}

// ---- dry runs ----

#[test]
fn a_dry_run_of_each_pass_says_what_it_would_do_and_changes_nothing() {
    let w = world("dry");
    w.satellite_on();
    let before = w.files();
    for (name, says) in [
        ("review", "autopilot: would start "),
        ("roadmap", "autopilot: would start "),
        (
            "merge",
            "autopilot: would merge iannitram/demo [8], closing [12]",
        ),
        ("weekly", "weekly backup: would back up iannitram/demo"),
        ("satellite", "satellite: would start gemini"),
    ] {
        let out = pass(name, &["--dry-run"], &w);
        assert_eq!(code(&out), Some(0), "{name}: {}", stderr(&out));
        assert!(stdout(&out).contains(says), "{name}: {}", stdout(&out));
        assert_eq!(w.files(), before, "{name}: a dry run writes nothing");
    }
    assert!(w.runs().is_empty(), "a dry run starts no job and no loop");
    assert!(w.calls("merge-pr").is_empty(), "a dry run merges nothing");
    assert!(!lock::held(&passes_lock_target(&w.data)));
    assert!(!lock::held(&satellite_lock_target(&w.data)));
}

#[test]
fn a_dry_run_as_json_is_the_pass_report() {
    let w = world("dry-json");
    let out = pass("review", &["--dry-run", "--json"], &w);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let report: Value = serde_json::from_str(&stdout(&out)).expect("JSON");
    assert_eq!(report["pass"], "review");
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["started"].as_array().map(Vec::len), Some(1));
    assert_eq!(report["errors"], serde_json::json!([]));
    for key in [
        "merged",
        "refused",
        "backup",
        "backups",
        "lanes_started",
        "loops",
        "notes",
    ] {
        assert!(report[key].is_array(), "{key}: {report}");
    }
    assert!(w.runs().is_empty());
}

// ---- real passes through the binary ----

#[test]
fn the_review_pass_starts_the_review_once_and_leaves_its_exit_code() {
    let w = world("review");
    let out = pass("review", &[], &w);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    let id = text
        .trim()
        .strip_prefix("autopilot: started ")
        .unwrap_or_else(|| panic!("{text}"));
    assert_eq!(w.state().reviews_started, ["iannitram/demo#7"]);
    let runs = w.runs();
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert!(runs[0][1].ends_with(&format!("{id}.json")), "{runs:?}");
    // #379: the command waited for its launcher, so the job's exit code is on disk.
    let exit = w.data.join("autopilot/jobs").join(format!("{id}.exit"));
    assert_eq!(std::fs::read_to_string(&exit).unwrap(), "0");
    assert!(!lock::held(&passes_lock_target(&w.data)));

    let again = pass("review", &[], &w);
    assert_eq!(code(&again), Some(0));
    assert_eq!(stdout(&again), "autopilot review: nothing to do\n");
    assert_eq!(w.runs().len(), 1);
}

#[test]
fn the_roadmap_pass_starts_the_project_manager() {
    let w = world("roadmap");
    let out = pass("roadmap", &["--json"], &w);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let report: Value = serde_json::from_str(&stdout(&out)).expect("JSON");
    assert_eq!(report["pass"], "roadmap");
    assert_eq!(report["dry_run"], false);
    let started = report["started"].as_array().unwrap();
    assert_eq!(started.len(), 1, "{report}");
    let state = w.state();
    assert_eq!(state.pm_running.as_deref(), started[0].as_str());
    assert_eq!(state.roadmaps[0].1.as_deref(), Some("# the new plan"));
    assert_eq!(w.runs().len(), 1);
}

#[test]
fn a_satellite_tick_starts_its_lane_and_waits_for_the_loop() {
    let w = world("satellite");
    // Off: nothing to start, said on stdout.
    let off = pass("satellite", &[], &w);
    assert_eq!(code(&off), Some(0), "{}", stderr(&off));
    assert!(
        stdout(&off).contains("the satellite switch is off"),
        "{}",
        stdout(&off)
    );
    assert!(w.calls("fleet").is_empty());

    w.satellite_on();
    let out = pass("satellite", &["--json"], &w);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let report: Value = serde_json::from_str(&stdout(&out)).expect("JSON");
    assert_eq!(report["lanes_started"], serde_json::json!(["gemini"]));
    assert_eq!(report["loops"][0]["lane"], "gemini");
    assert_eq!(report["loops"][0]["exit_code"], 0);
    assert_eq!(w.runs().len(), 1);
    assert!(!lock::held(&satellite_lock_target(&w.data)));
}

/// `fleet run --next` waits (up to a minute) for a file `go` beside the script, then prints
/// one card; everything else as the support stub. Every call is logged.
const WAITING_FLEET: &str = r#"import json, os, sys, time
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:2] == ["run", "--next"]:
    give_up = time.time() + 60
    while not os.path.exists(os.path.join(here, "go")) and time.time() < give_up:
        time.sleep(0.02)
    print(json.dumps({"id": "sb-1", "outcome": "done"}))
"#;

/// The built binary as `run_in` sets it up, started and not waited for.
fn spawn(argv: &[&str], w: &World) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(argv)
        .current_dir(&w.cwd)
        .env("SWITCHBOARD_PIPELINE", &w.pipeline)
        .env("SWITCHBOARD_DATA", &w.data)
        .env("SWITCHBOARD_CONFIG", &w.config)
        .env("SWITCHBOARD_STATE", w.base.join("no-state"))
        .env("SWITCHBOARD_DEV_ROOT", &w.dev)
        .env("FLEET_MACHINE", "test-box")
        .env_remove("FLEET_ROOT")
        .env_remove("FLEET_STATE_DIR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run switchboard")
}

/// Wait (up to a minute) until `done` says so.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let give_up = std::time::Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(
            std::time::Instant::now() < give_up,
            "waited a minute for {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// #432 (ruling 2): a headless satellite run that is killed while its loop runs leaves the loop
/// on disk; the next run adopts it and watches it to its end, and no second copy starts.
#[test]
fn a_killed_satellite_run_leaves_its_loop_to_the_next_run() {
    use switchboard_core::autopilot::loops;
    let w = world("adopt");
    let tools = w.pipeline.join("tools").join("fleet");
    std::fs::write(tools.join("fleet.py"), WAITING_FLEET).unwrap();
    w.satellite_on();
    let dir = satellite_lock_target(&w.data);
    let argv = ["--role", "satellite", "autopilot", "satellite"];

    let mut first = spawn(&argv, &w);
    let first_pid = first.id();
    wait_until("the first run's loop on disk", || {
        loops::read(&dir)
            .loops
            .iter()
            .any(|r| r.lane == "gemini" && r.owner_pid == first_pid)
    });
    first.kill().unwrap();
    first.wait().unwrap();
    let loop_pid = loops::read(&dir).loops[0].pid;
    assert!(loops::alive(loop_pid, None), "the loop outlives its run");
    // TEMP diagnostics (Windows CI)
    let mut diag = format!("record: {:?}\n", loops::read(&dir).loops);
    for i in 0..10 {
        diag.push_str(&format!(
            "t+{}ms alive={} token={:?}\n",
            i * 200,
            loops::alive(loop_pid, None),
            loops::process_token(loop_pid)
        ));
        std::thread::sleep(Duration::from_millis(200));
    }
    if cfg!(windows) {
        let tl = Command::new("tasklist").args(["/FO", "CSV", "/NH"]).output().unwrap();
        for line in String::from_utf8_lossy(&tl.stdout).lines() {
            let l = line.to_lowercase();
            if l.contains("py") || l.contains("switchboard") {
                diag.push_str(&format!("tasklist: {line}\n"));
            }
        }
    }
    diag.push_str(&format!(
        "calls: {:?}\nerr: {}\nout: {}\n",
        w.calls("fleet"),
        std::fs::read_to_string(dir.join("gemini.err")).unwrap_or_default(),
        std::fs::read_to_string(dir.join("gemini.out")).unwrap_or_default()
    ));

    let second = spawn(&argv, &w);
    let second_pid = second.id();
    wait_until("the second run to adopt the loop", || {
        loops::read(&dir)
            .loops
            .iter()
            .any(|r| r.pid == loop_pid && r.owner_pid == second_pid)
    });
    std::fs::write(tools.join("go"), "").unwrap();
    let out = second.wait_with_output().unwrap();
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("adopted the gemini loop"), "{text}");
    assert!(
        !text.contains("started gemini"),
        "{text}\n{}\nDIAG:\n{diag}\ncalls after: {:?}",
        stderr(&out),
        w.calls("fleet")
    );
    assert_eq!(w.runs().len(), 1, "one loop, never a second copy");
    assert!(loops::read(&dir).loops.is_empty());
    assert!(!lock::held(&dir));
}

#[test]
fn a_pass_is_refused_while_another_run_holds_the_lock() {
    let w = world("lock");
    // The window (or another headless run) holds the passes and the satellite.
    w.satellite_on();
    let passes = FileLock::take(&passes_lock_target(&w.data), Duration::ZERO).unwrap();
    let satellite = FileLock::take(&satellite_lock_target(&w.data), Duration::ZERO).unwrap();
    let before = w.files();

    let out = pass("review", &[], &w);
    assert_eq!(code(&out), Some(1));
    assert!(
        stderr(&out).contains("another run holds the passes"),
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), "");

    let out = pass("satellite", &[], &w);
    assert_eq!(code(&out), Some(1));
    assert!(
        stdout(&out).contains("another run holds the satellite"),
        "{}",
        stdout(&out)
    );
    assert!(w.runs().is_empty());
    assert_eq!(w.files(), before);

    // The status read says so, and a dry run still answers.
    let status: Value =
        serde_json::from_str(&stdout(&run(&["autopilot", "status", "--json"], &w))).unwrap();
    assert_eq!(status["passes_running"], true);
    assert_eq!(status["satellite"]["live"], true);
    assert_eq!(code(&pass("review", &["--dry-run"], &w)), Some(0));

    drop((passes, satellite));
    assert_eq!(code(&pass("review", &[], &w)), Some(0));
}

#[test]
fn a_pass_that_fails_prints_its_error_and_exits_1() {
    let w = world("failing");
    w.break_the_fleet();
    let out = pass("review", &[], &w);
    assert_eq!(code(&out), Some(1), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("budget allow"),
        "the error is the command's output: {}",
        stdout(&out)
    );
    assert!(w.runs().is_empty());
    assert!(w.state().reviews_started.is_empty());
}

// ---- the status read ----

#[test]
fn status_needs_no_role_and_reads_only() {
    let w = world("status");
    let before = w.files();
    let out = run(&["autopilot", "status"], &w);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    for line in [
        "passes running now: no\n",
        "running jobs: none\n",
        "project manager job: none\n",
        "reviews started: 0\n",
        "roadmaps seen: iannitram/demo\n",
        "weekly due: yes\n",
        "satellite switch: off\n",
        "satellite loops running here: no\n",
        "satellite holds: none\n",
    ] {
        assert!(text.contains(line), "{line:?} not in:\n{text}");
    }

    let out = run(&["--json", "autopilot", "status"], &w);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let status: Value = serde_json::from_str(&stdout(&out)).expect("JSON");
    let keys = |v: &Value| -> Vec<String> { v.as_object().unwrap().keys().cloned().collect() };
    assert_eq!(
        keys(&status),
        [
            "state",
            "weekly_due",
            "passes_running",
            "running_jobs",
            "satellite"
        ]
    );
    assert_eq!(
        keys(&status["state"]),
        [
            "reviews_started",
            "roadmaps_seen",
            "weekly_last_run",
            "drift_pending",
            "pm_running"
        ]
    );
    assert_eq!(
        keys(&status["satellite"]),
        ["on", "lanes", "live", "running", "held", "as_of"]
    );
    assert_eq!(
        status["state"]["roadmaps_seen"],
        serde_json::json!(["iannitram/demo"])
    );
    assert_eq!(w.files(), before);
    assert!(w.calls("fleet").is_empty());
}

// ---- in-process: GitHub and the backup are fakes ----

#[test]
fn the_merge_pass_merges_what_passed_and_reads_the_repo() {
    let w = world("merge");
    let seams = Seams::default();
    let (github, backup) = (fake_github(&seams), fake_backup(&seams));
    let headless = headless(&w, &seams, &github, &backup);
    let (code, out, err) = in_process(&[PM[0], PM[1], "autopilot", "merge"], &headless);
    assert_eq!(code, exit(0), "{err}");
    assert_eq!(out, "autopilot: merged iannitram/demo [8], closing [12]\n");
    let merges = w.calls("merge-pr");
    assert!(!merges.is_empty());
    assert!(
        merges.iter().all(|args| args.contains(&"8".to_string())),
        "{merges:?}"
    );
    assert_eq!(*seams.reads.lock().unwrap(), ["iannitram/demo"]);
}

#[test]
fn the_weekly_pass_starts_the_drift_clear_and_runs_the_backup() {
    let w = world("weekly");
    let seams = Seams::default();
    let (github, backup) = (fake_github(&seams), fake_backup(&seams));
    let headless = headless(&w, &seams, &github, &backup);

    let before = w.files();
    let (code, out, err) = in_process(
        &[PM[0], PM[1], "autopilot", "weekly", "--dry-run"],
        &headless,
    );
    assert_eq!(code, exit(0), "{err}");
    assert!(out.contains("autopilot: would start "), "{out}");
    assert!(
        out.contains("weekly backup: would back up iannitram/demo\n"),
        "{out}"
    );
    assert!(
        seams.backed_up.borrow().is_empty(),
        "a dry run backs nothing up"
    );
    assert_eq!(w.files(), before);

    let (code, out, err) = in_process(&[PM[0], PM[1], "autopilot", "weekly"], &headless);
    assert_eq!(code, exit(0), "{err}");
    assert!(out.contains("autopilot: started "), "{out}");
    assert!(
        out.contains("weekly backup: iannitram/demo: refused: stub\n"),
        "{out}"
    );
    assert_eq!(*seams.backed_up.borrow(), ["iannitram/demo"]);
    assert_eq!(
        w.state().weekly_last_run,
        Some(autopilot_support::SUNDAY_AFTER_SIX)
    );
    assert_eq!(w.runs().len(), 1);
}

#[test]
fn a_report_that_cannot_be_written_to_stdout_exits_1() {
    struct Closed;
    impl std::io::Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let w = world("closed");
    let seams = Seams::default();
    let (github, backup) = (fake_github(&seams), fake_backup(&seams));
    let headless = headless(&w, &seams, &github, &backup);
    for argv in [
        vec!["autopilot", "status"],
        vec![PM[0], PM[1], "autopilot", "merge", "--dry-run"],
    ] {
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        let mut err = Vec::new();
        let code = command_with(&argv, &headless, false, &mut Closed, &mut err);
        assert_eq!(format!("{code:?}"), exit(1), "{argv:?}");
    }
}

#[test]
fn every_other_command_is_left_to_the_command_table() {
    let w = world("other");
    let seams = Seams::default();
    let (github, backup) = (fake_github(&seams), fake_backup(&seams));
    let headless = headless(&w, &seams, &github, &backup);
    for argv in [
        vec!["sky"],
        vec!["--help"],
        vec!["autopilot", "--help"],
        vec!["help"],
        vec!["merge", "demo#8"],
    ] {
        let (code, out, err) = in_process(&argv, &headless);
        assert_eq!(code, "None", "{argv:?}");
        assert_eq!((out.as_str(), err.as_str()), ("", ""));
    }
}
