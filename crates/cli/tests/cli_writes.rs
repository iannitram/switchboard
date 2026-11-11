//! Locked tests for #242 (CLI-3: the real merge and the Waiting write). Qwen never edits this
//! file (R28). `CliWrites` runs in-process through `run_with`, over a temp app data folder with
//! a stored snapshot, a temp pipeline root whose `tools/merge-pr/merge_pr.py` is a stub, and a
//! temp dev root that holds the clone. The re-read after a merge goes to a `Recorded`
//! transport with no answers, so it fails and is ignored. The binary tests never reach a merge.

// The binary crate has no library target, so the sources are compiled in directly.
#[path = "../src/main.rs"]
mod app;
// `cli.rs` reads `super::parse_args`, so it sits at this root beside the parser's import.
#[path = "../src/cli.rs"]
#[allow(dead_code, clippy::duplicate_mod)]
mod cli;

#[allow(unused_imports)]
use app::{parse_args, Args};

use std::path::PathBuf;
use std::process::{Command, ExitCode, Output};

use cli::{run_with, CliWrites};
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::Recorded;
use switchboard_core::github::{Issue, PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::waiting::{LocalItems, Source};

const NOW: i64 = 1_700_000_000;
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// Logs its arguments to `calls.log`; exits with the code in `exit-dry-run` or `exit-push`
/// (0 when there is none), printing `REFUSED: branch is behind main` when that code is not 0.
const STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(args) + "\n")
code = os.path.join(here, "exit-" + args[-1].lstrip("-"))
code = int(open(code).read()) if os.path.exists(code) else 0
if code:
    print("REFUSED: branch is behind main")
sys.exit(code)
"#;

/// #346: a bare name is stored as the `owner/name` it names, so the link is the repo's.
const ITEM_LINE: &str = "I was asked from the CLI to archive o/switchboard | Asked for from the CLI or MCP; only the app can do it, after I confirm (CORE-3). | Confirm in the app | https://github.com/o/switchboard\n";
const ISSUE_LINE: &str =
    "o/r#5: Pick the sync rule | Labelled needs-ian. | Open issue | https://github.com/o/r/issues/5\n";

/// One test's folders: the app data folder, the pipeline root and its stub folder, the dev
/// root, the clone `<dev>/r`, and a plain working folder (not a job worktree).
struct Setup {
    data: PathBuf,
    pipeline: PathBuf,
    stub_dir: PathBuf,
    dev: PathBuf,
    clone: PathBuf,
    cwd: PathBuf,
}

fn setup(tag: &str) -> Setup {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("sb-cliw-{tag}-{}-{nanos}", std::process::id()));
    let s = Setup {
        data: base.join("data"),
        pipeline: base.join("pipeline"),
        stub_dir: base.join("pipeline").join("tools").join("merge-pr"),
        dev: base.join("dev"),
        clone: base.join("dev").join("r"),
        cwd: base.join("plain"),
    };
    for dir in [&s.data, &s.stub_dir, &s.clone, &s.cwd] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(s.stub_dir.join("merge_pr.py"), STUB).unwrap();
    s
}

/// PR 71 on `feat-x`, closing #12, with these verdicts (each `(verdict, sha)`) and CI state.
fn pr(verdicts: &[(&str, &str)], ci: &str) -> PullRequest {
    PullRequest {
        number: 71,
        title: "Feat X".to_string(),
        state: "open".to_string(),
        head_sha: HEAD.to_string(),
        head_ref: "feat-x".to_string(),
        closes: vec![12],
        verdicts: verdicts
            .iter()
            .map(|(verdict, sha)| ReviewVerdict {
                verdict: verdict.to_string(),
                sha: sha.to_string(),
                needs_me: false,
            })
            .collect(),
        ci_on_head: ci.to_string(),
        ..Default::default()
    }
}

/// Store `o/r`'s snapshot: these PRs and one open needs-ian issue, read at 2000.
fn save(s: &Setup, pulls: Vec<PullRequest>) {
    let snap = RepoSnapshot {
        repo: "o/r".to_string(),
        read_at: 2000,
        pulls,
        issues: vec![Issue {
            number: 5,
            title: "Pick the sync rule".to_string(),
            state: "open".to_string(),
            labels: vec!["needs-ian".to_string()],
            ..Default::default()
        }],
        ..Default::default()
    };
    SnapshotStore::new(&s.data).save(&snap).unwrap();
}

/// Store an empty snapshot of `o/switchboard`, so `delete switchboard` names a known repo.
fn save_switchboard(s: &Setup) {
    let snap = RepoSnapshot {
        repo: "o/switchboard".to_string(),
        read_at: 2000,
        ..Default::default()
    };
    SnapshotStore::new(&s.data).save(&snap).unwrap();
}

fn writes(s: &Setup) -> CliWrites {
    CliWrites {
        data: Some(s.data.clone()),
        pipeline: Some(s.pipeline.clone()),
        dev_root: Some(s.dev.clone()),
        in_job_worktree: false,
        transport: Some(Box::new(Recorded::new(vec![], vec![]))),
    }
}

/// Run one command in-process in the plain folder; the exit code and what went to stderr.
fn run(argv: &[&str], s: &Setup, w: &CliWrites) -> (ExitCode, String) {
    let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
    let mut err: Vec<u8> = Vec::new();
    let code = run_with(&argv, &s.cwd, NOW, w, &mut err);
    (code, String::from_utf8(err).unwrap())
}

fn calls(s: &Setup) -> Vec<Vec<String>> {
    match std::fs::read_to_string(s.stub_dir.join("calls.log")) {
        Ok(t) => t
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn args(s: &Setup, flag: &str) -> Vec<String> {
    let clone = s.clone.display().to_string();
    [
        "merge",
        &clone,
        "main",
        "origin/feat-x",
        "--pr",
        "71",
        "--repo",
        "o/r",
        flag,
    ]
    .iter()
    .map(|a| a.to_string())
    .collect()
}

const PM: [&str; 2] = ["--role", "project-manager"];

fn merge(target: &str) -> Vec<&str> {
    [&PM[..], &["merge", target]].concat()
}

#[test]
fn a_pass_on_head_with_green_ci_merges_dry_run_then_push() {
    let s = setup("ok");
    save(&s, vec![pr(&[("PASS", HEAD)], "success")]);
    let (code, err) = run(&merge("o/r#71"), &s, &writes(&s));
    assert_eq!(err, "");
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(calls(&s), vec![args(&s, "--dry-run"), args(&s, "--push")]);
}

#[test]
fn the_bare_repo_name_finds_the_owner_repo_snapshot() {
    let s = setup("bare");
    save(&s, vec![pr(&[("PASS", HEAD)], "success")]);
    let (code, err) = run(&merge("r#71"), &s, &writes(&s));
    assert_eq!(err, "");
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(calls(&s), vec![args(&s, "--dry-run"), args(&s, "--push")]);
}

/// A refused merge: exit 1, exactly this stderr line, and merge_pr.py never ran.
fn assert_refused(s: &Setup, w: &CliWrites, target: &str, text: &str) {
    let (code, err) = run(&merge(target), s, w);
    assert_eq!(err, format!("{text}\n"));
    assert_eq!(code, ExitCode::from(1));
    assert!(calls(s).is_empty(), "merge_pr.py ran: {:?}", calls(s));
}

#[test]
fn no_pass_is_refused_with_the_gh5_text() {
    let s = setup("no-pass");
    save(&s, vec![pr(&[("FAIL", HEAD)], "success")]);
    let text = "merge o/r#71: no VERDICT: PASS on this PR";
    assert_refused(&s, &writes(&s), "o/r#71", text);
}

#[test]
fn a_pass_on_an_older_head_is_refused() {
    let s = setup("old-pass");
    save(&s, vec![pr(&[("PASS", OLD)], "success")]);
    let text = "merge o/r#71: the latest VERDICT: PASS is not on the head";
    assert_refused(&s, &writes(&s), "o/r#71", text);
}

#[test]
fn ci_not_green_is_refused_with_its_state() {
    let s = setup("ci");
    save(&s, vec![pr(&[("PASS", HEAD)], "failure")]);
    let text = "merge o/r#71: CI on the head is failure";
    assert_refused(&s, &writes(&s), "o/r#71", text);
}

#[test]
fn a_pr_missing_from_the_snapshot_is_refused() {
    let s = setup("no-pr");
    save(&s, vec![pr(&[("PASS", HEAD)], "success")]);
    let text = "merge o/r#99: not in the snapshot; run switchboard refresh";
    assert_refused(&s, &writes(&s), "o/r#99", text);
}

#[test]
fn a_repo_with_no_snapshot_is_refused() {
    let s = setup("no-snap");
    save(&s, vec![pr(&[("PASS", HEAD)], "success")]);
    let text = "merge o/other#71: no snapshot of o/other; run switchboard refresh";
    assert_refused(&s, &writes(&s), "o/other#71", text);
}

#[test]
fn a_missing_clone_is_refused_before_merge_pr_runs() {
    let s = setup("no-clone");
    save(&s, vec![pr(&[("PASS", HEAD)], "success")]);
    std::fs::remove_dir_all(&s.clone).unwrap();
    let text = format!("merge o/r#71: no clone at {}", s.clone.display());
    assert_refused(&s, &writes(&s), "o/r#71", &text);
}

#[test]
fn each_unset_folder_is_named() {
    let s = setup("unset");
    save(&s, vec![pr(&[("PASS", HEAD)], "success")]);
    let no_data = CliWrites {
        data: None,
        ..writes(&s)
    };
    let text = "merge o/r#71: set SWITCHBOARD_DATA to the app data folder";
    assert_refused(&s, &no_data, "o/r#71", text);
    let no_pipeline = CliWrites {
        pipeline: None,
        ..writes(&s)
    };
    let text = "merge o/r#71: set SWITCHBOARD_PIPELINE to your pipeline checkout";
    assert_refused(&s, &no_pipeline, "o/r#71", text);
    let no_dev = CliWrites {
        dev_root: None,
        ..writes(&s)
    };
    let text = "merge o/r#71: set SWITCHBOARD_DEV_ROOT to the folder that holds the clones";
    assert_refused(&s, &no_dev, "o/r#71", text);
}

#[test]
fn a_dry_run_refusal_stops_before_the_push() {
    let s = setup("dry-refused");
    save(&s, vec![pr(&[("PASS", HEAD)], "success")]);
    std::fs::write(s.stub_dir.join("exit-dry-run"), "1").unwrap();
    let (code, err) = run(&merge("o/r#71"), &s, &writes(&s));
    assert_eq!(err, "merge o/r#71: branch is behind main\n");
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(calls(&s), vec![args(&s, "--dry-run")]);
}

#[test]
fn delete_adds_one_item_to_the_local_items() {
    let s = setup("delete");
    save_switchboard(&s);
    let w = writes(&s);
    let (code, err) = run(&["delete", "switchboard"], &s, &w);
    assert_eq!(err, "archive o/switchboard: needs a confirm in the app\n");
    assert_eq!(code, ExitCode::from(1));
    let items = LocalItems::new(&s.data).load().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].source,
        Source::GuardedAction {
            text: "I was asked from the CLI to archive o/switchboard".to_string()
        }
    );
    assert_eq!(items[0].link, "https://github.com/o/switchboard");
    assert_eq!(items[0].since, NOW);
    // Asked again, by either spelling (#346): the same source is replaced in place, one item.
    run(&["delete", "switchboard"], &s, &w);
    run(&["delete", "o/switchboard"], &s, &w);
    assert_eq!(LocalItems::new(&s.data).load().unwrap().len(), 1);
}

#[test]
fn delete_with_no_data_folder_cannot_tell_and_writes_nothing() {
    let s = setup("delete-no-data");
    let w = CliWrites {
        data: None,
        ..writes(&s)
    };
    let (code, err) = run(&["delete", "switchboard"], &s, &w);
    assert_eq!(
        err,
        "switchboard: delete: set SWITCHBOARD_DATA to the app data folder\n"
    );
    assert_eq!(code, ExitCode::from(1));
}

#[test]
fn delete_and_restore_of_an_unknown_repo_write_no_item() {
    let s = setup("unknown");
    save(&s, vec![]);
    let w = writes(&s);
    for command in ["delete", "restore"] {
        let (code, err) = run(&[command, "nope"], &s, &w);
        assert_eq!(err, "switchboard: no such repo: nope\n");
        assert_eq!(code, ExitCode::from(1));
    }
    assert!(LocalItems::new(&s.data).load().unwrap().is_empty());
}

/// The built binary in `s.cwd` with `SWITCHBOARD_DATA` set and no pipeline or dev root.
fn switchboard(argv: &[&str], s: &Setup) -> Output {
    Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(argv)
        .current_dir(&s.cwd)
        .env("SWITCHBOARD_DATA", &s.data)
        .env_remove("SWITCHBOARD_PIPELINE")
        .env_remove("SWITCHBOARD_DEV_ROOT")
        .output()
        .expect("run switchboard")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_binary_delete_writes_the_item_and_waiting_lists_it_first() {
    let s = setup("bin-waiting");
    save(&s, vec![]);
    save_switchboard(&s);
    let out = switchboard(&["delete", "switchboard"], &s);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        "archive o/switchboard: needs a confirm in the app\n"
    );
    let out = switchboard(&["waiting"], &s);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), format!("{ITEM_LINE}{ISSUE_LINE}"));
    let out = switchboard(&["waiting", "--json"], &s);
    let items: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    assert_eq!(items.as_array().map(Vec::len), Some(2));
}

#[test]
fn waiting_with_only_local_items_and_no_snapshot_lists_them() {
    let s = setup("bin-local-only");
    save_switchboard(&s);
    run(&["delete", "switchboard"], &s, &writes(&s));
    let out = switchboard(&["waiting"], &s);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), ITEM_LINE);
}

#[test]
fn the_binary_reads_the_pipeline_and_dev_root_from_the_environment() {
    let s = setup("bin-env");
    save(&s, vec![pr(&[("PASS", HEAD)], "success")]);
    let out = switchboard(&[&PM[..], &["merge", "o/r#71"]].concat(), &s);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        "merge o/r#71: set SWITCHBOARD_PIPELINE to your pipeline checkout\n"
    );
    let empty_dev = s.dev.join("empty");
    std::fs::create_dir_all(&empty_dev).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_switchboard"))
        .args(merge("o/r#71"))
        .current_dir(&s.cwd)
        .env("SWITCHBOARD_DATA", &s.data)
        .env("SWITCHBOARD_PIPELINE", &s.pipeline)
        .env("SWITCHBOARD_DEV_ROOT", &empty_dev)
        .output()
        .expect("run switchboard");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!(
            "merge o/r#71: no clone at {}\n",
            empty_dev.join("r").display()
        )
    );
    assert!(calls(&s).is_empty());
}
