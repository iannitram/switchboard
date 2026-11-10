//! CLI edge cases from the 2026-11-10 bug hunt, run as the built binary over temp folders (and
//! the seams in-process): a stdout that cannot be written (#334), a non-UTF-8 argument (#335),
//! control characters in titles (#352), `mcp` flags (#354), `--help` anywhere (#355), extra
//! words (#356), a short repo name that matches two owners (#346), and `pipeline_repo` from
//! `settings.json` (#359).

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

use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::{Issue, RepoSnapshot};
use switchboard_core::rules::Role;
use switchboard_core::waiting::LocalItems;

/// Fresh, empty folders for one test: data, config, state and dev root.
struct Dirs {
    root: PathBuf,
}

impl Dirs {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("sb-edges-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for sub in ["data", "config", "state", "dev"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        Self { root }
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    /// The built binary, isolated in these folders, with no pipeline set.
    fn command(&self, argv: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_switchboard"));
        cmd.args(argv)
            .current_dir(&self.root)
            .env("SWITCHBOARD_DATA", self.data())
            .env("SWITCHBOARD_CONFIG", self.config())
            .env("SWITCHBOARD_STATE", self.root.join("state"))
            .env("FLEET_STATE_DIR", self.root.join("state"))
            .env("SWITCHBOARD_DEV_ROOT", self.root.join("dev"))
            .env_remove("SWITCHBOARD_PIPELINE")
            .env_remove("FLEET_ROOT")
            .env_remove("FLEET_STUB_EXIT")
            .env_remove("FLEET_STUB_STDERR")
            .env_remove("FLEET_STUB_BADJSON")
            .stdin(Stdio::null());
        cmd
    }

    fn run(&self, argv: &[&str]) -> Output {
        self.command(argv).output().expect("run switchboard")
    }

    fn save(&self, repo: &str, issues: &[(u64, &str, &[&str])]) {
        let snap = RepoSnapshot {
            repo: repo.to_string(),
            issues: issues
                .iter()
                .map(|(number, title, labels)| Issue {
                    number: *number,
                    title: title.to_string(),
                    state: "open".to_string(),
                    labels: labels.iter().map(|l| l.to_string()).collect(),
                    milestone: None,
                    body: String::new(),
                    closed_by_pr: None,
                })
                .collect(),
            read_at: 2000,
            ..Default::default()
        };
        SnapshotStore::new(&self.data()).save(&snap).unwrap();
    }
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn fleet_stub() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/fleet-stub")
}

fn no_panic(out: &Output) {
    assert!(!stderr(out).contains("panicked"), "stderr: {}", stderr(out));
    assert_ne!(out.status.code(), Some(101), "stderr: {}", stderr(out));
}

// #334: a stdout that cannot be written.

/// A writer that fails every write with `kind`.
struct Failing(std::io::ErrorKind);

impl Write for Failing {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::from(self.0))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn emit_is_a_quiet_exit_0_when_the_reader_closed_the_pipe() {
    let code = cli::emit_to(&mut Failing(std::io::ErrorKind::BrokenPipe), "text\n");
    assert_eq!(code, ExitCode::SUCCESS);
}

#[test]
fn emit_is_exit_1_on_any_other_write_error() {
    let code = cli::emit_to(&mut Failing(std::io::ErrorKind::StorageFull), "text\n");
    assert_eq!(code, ExitCode::from(1));
}

#[test]
fn emit_writes_the_text() {
    let mut out = Vec::new();
    assert_eq!(cli::emit_to(&mut out, "text\n"), ExitCode::SUCCESS);
    assert_eq!(out, b"text\n");
}

#[cfg(target_os = "linux")]
#[test]
fn a_full_device_is_one_line_on_stderr_and_exit_1() {
    let dirs = Dirs::new("full");
    dirs.save("owner/alpha", &[(1, "One", &[])]);
    for argv in [
        &["--version"][..],
        &["sky"],
        &["--json", "sky"],
        &["star", "alpha#1"],
    ] {
        let full = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .unwrap();
        let out = dirs
            .command(argv)
            .stdout(full)
            .output()
            .expect("run switchboard");
        no_panic(&out);
        assert_eq!(out.status.code(), Some(1), "{argv:?}");
        assert!(
            stderr(&out).starts_with("switchboard: cannot write the output: "),
            "{argv:?}: {}",
            stderr(&out)
        );
        assert_eq!(stderr(&out).lines().count(), 1, "{argv:?}");
    }
}

#[test]
fn a_closed_pipe_is_a_quiet_exit() {
    let dirs = Dirs::new("pipe");
    // A title far bigger than a pipe's buffer, so the write meets the closed pipe.
    let title = "x".repeat(1 << 20);
    dirs.save("owner/alpha", &[(1, &title, &[])]);
    let mut child = dirs
        .command(&["--json", "star", "alpha#1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn switchboard");
    drop(child.stdout.take());
    let out = child.wait_with_output().expect("wait");
    no_panic(&out);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "");
}

// #335: a non-UTF-8 argument.

#[test]
fn utf8_args_names_a_word_that_is_not_utf8() {
    assert_eq!(
        app::utf8_args(["sky".into(), "--json".into()]),
        Ok(vec!["sky".to_string(), "--json".to_string()])
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let bad = std::ffi::OsString::from_vec(b"al\xffpha#1".to_vec());
        let err = app::utf8_args(["star".into(), bad]).unwrap_err();
        assert!(err.contains("not valid UTF-8"), "{err}");
        assert!(err.contains("al\u{FFFD}pha#1"), "{err}");
    }
}

#[cfg(unix)]
#[test]
fn a_non_utf8_argument_is_a_usage_error_not_a_panic() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let dirs = Dirs::new("utf8");
    for argv in [
        vec![OsStr::new("star"), OsStr::from_bytes(b"al\xffpha#1")],
        vec![OsStr::new("mcp"), OsStr::from_bytes(b"\xff")],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_switchboard"))
            .args(&argv)
            .env("SWITCHBOARD_DATA", dirs.data())
            .env("SWITCHBOARD_CONFIG", dirs.config())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        no_panic(&out);
        assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
        assert!(
            stderr(&out).starts_with("switchboard: an argument is not valid UTF-8"),
            "{}",
            stderr(&out)
        );
        assert_eq!(stdout(&out), "");
    }
}

// #352: control characters in titles.

const EVIL: &str = "evil\u{1b}]0;PWNED\u{7}\nfake | line";

#[test]
fn terminal_safe_escapes_control_characters() {
    assert_eq!(
        cli::terminal_safe(EVIL),
        "evil\\u{1b}]0;PWNED\\u{7}\\nfake | line"
    );
    assert_eq!(cli::terminal_safe("plain | text"), "plain | text");
    assert_eq!(cli::terminal_safe("tab\there"), "tab\\there");
}

#[test]
fn star_and_waiting_print_no_raw_control_characters() {
    let dirs = Dirs::new("evil");
    dirs.save("owner/alpha", &[(2, EVIL, &["needs-ian"])]);
    let star = dirs.run(&["star", "alpha#2"]);
    assert_eq!(star.status.code(), Some(0), "stderr: {}", stderr(&star));
    let waiting = dirs.run(&["waiting"]);
    assert_eq!(
        waiting.status.code(),
        Some(0),
        "stderr: {}",
        stderr(&waiting)
    );
    for out in [&star, &waiting] {
        assert!(
            !out.stdout.iter().any(|b| *b == 0x1b || *b == 0x07),
            "{:?}",
            stdout(out)
        );
    }
    assert!(stdout(&star).starts_with("owner/alpha#2: evil\\u{1b}]0;PWNED\\u{7}\\nfake | line\n"));
    // One item is one row: the title's newline cannot forge a second.
    assert_eq!(stdout(&waiting).lines().count(), 1, "{}", stdout(&waiting));
    // `--json` keeps the title as it is (JSON escapes it).
    let json = dirs.run(&["--json", "star", "alpha#2"]);
    let star: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(star["title"], EVIL);
}

// #354: `mcp` and its flags.

#[test]
fn mcp_call_reads_role_before_or_after_mcp() {
    let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let pm = Some(Ok(app::McpCall::Serve(Some(Role::ProjectManager))));
    assert_eq!(
        app::mcp_call(&words(&["--role", "project-manager", "mcp"])),
        pm
    );
    assert_eq!(
        app::mcp_call(&words(&["mcp", "--role", "project-manager"])),
        pm
    );
    assert_eq!(
        app::mcp_call(&words(&["mcp", "--role=project-manager"])),
        pm
    );
    assert_eq!(
        app::mcp_call(&words(&["mcp"])),
        Some(Ok(app::McpCall::Serve(None)))
    );
    assert_eq!(
        app::mcp_call(&words(&["mcp", "--help"])),
        Some(Ok(app::McpCall::Help))
    );
    assert_eq!(
        app::mcp_call(&words(&["mcp", "--bogus"])),
        Some(Err("unknown flag: --bogus".to_string()))
    );
    assert_eq!(
        app::mcp_call(&words(&["mcp", "--role", "nobody"])),
        Some(Err("unknown role: nobody".to_string()))
    );
    assert!(matches!(
        app::mcp_call(&words(&["mcp", "extra"])),
        Some(Err(_))
    ));
    assert_eq!(app::mcp_call(&words(&["sky"])), None);
    assert_eq!(app::mcp_call(&words(&["--role", "mcp"])), None);
}

#[test]
fn role_before_mcp_serves() {
    let dirs = Dirs::new("mcp-role");
    let out = dirs.run(&["--role", "project-manager", "mcp"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "");
}

#[test]
fn an_unknown_flag_after_mcp_is_exit_2() {
    let dirs = Dirs::new("mcp-bogus");
    let out = dirs.run(&["mcp", "--bogus"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).starts_with("switchboard mcp: unknown flag: --bogus\n"));
    assert_eq!(stdout(&out), "");
}

#[test]
fn mcp_help_prints_help_and_does_not_serve() {
    let dirs = Dirs::new("mcp-help");
    for argv in [&["mcp", "--help"][..], &["mcp", "-h"], &["--help", "mcp"]] {
        let out = dirs.run(argv);
        assert_eq!(out.status.code(), Some(0), "{argv:?}");
        assert!(
            stdout(&out).starts_with("usage: switchboard [--role <role>] mcp\n"),
            "{argv:?}: {}",
            stdout(&out)
        );
    }
}

// #355: `--help` and `--version` anywhere.

#[test]
fn help_and_version_answer_anywhere_on_the_line() {
    let dirs = Dirs::new("help");
    for argv in [
        &["sky", "--help"][..],
        &["--json", "--help"],
        &["star", "-h"],
    ] {
        let out = dirs.run(argv);
        assert_eq!(out.status.code(), Some(0), "{argv:?}");
        assert_eq!(stdout(&out), format!("{}\n", cli::USAGE), "{argv:?}");
        assert_eq!(stderr(&out), "", "{argv:?}");
    }
    let out = dirs.run(&["sky", "--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        format!("switchboard {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(cli::USAGE.contains("--help"));
}

// #356: extra words.

#[test]
fn extra_words_after_a_command_are_exit_2() {
    let dirs = Dirs::new("extra");
    dirs.save("owner/alpha", &[(1, "One", &[])]);
    dirs.save("owner/beta", &[(1, "One", &[])]);
    for (argv, first) in [
        (
            &["sky", "extra", "words"][..],
            "switchboard: sky takes no other words, got extra words\n",
        ),
        (
            &["waiting", "x"],
            "switchboard: waiting takes no other words, got x\n",
        ),
        (
            &["reports", "x"],
            "switchboard: reports takes no other words, got x\n",
        ),
        (
            &["budget", "x"],
            "switchboard: budget takes no other words, got x\n",
        ),
        (
            &["loadout", "x"],
            "switchboard: loadout takes no other words, got x\n",
        ),
        (
            &["delete", "alpha", "beta"],
            "switchboard: delete takes one repo, got more: beta\n",
        ),
        (
            &["--role", "project-manager", "merge", "alpha#1", "beta#1"],
            "switchboard: merge takes one <repo>#<pr>, got more: beta#1\n",
        ),
    ] {
        let out = dirs.run(argv);
        assert_eq!(out.status.code(), Some(2), "{argv:?}: {}", stderr(&out));
        assert!(
            stderr(&out).starts_with(first),
            "{argv:?}: {}",
            stderr(&out)
        );
        assert_eq!(stdout(&out), "", "{argv:?}");
    }
    assert!(LocalItems::new(&dirs.data()).load().unwrap().is_empty());
}

// #346: a short repo name that matches two owners.

#[test]
fn resolve_repo_names_both_matches() {
    let known = ["aaa/alpha", "owner/alpha", "owner/beta"];
    assert_eq!(
        cli::resolve_repo("owner/alpha", known),
        Ok(Some("owner/alpha"))
    );
    assert_eq!(cli::resolve_repo("beta", known), Ok(Some("owner/beta")));
    assert_eq!(cli::resolve_repo("gamma", known), Ok(None));
    assert_eq!(cli::resolve_repo("x/alpha", known), Ok(None));
    assert_eq!(
        cli::resolve_repo("alpha", known),
        Err("alpha matches aaa/alpha and owner/alpha; name it as <owner>/alpha".to_string())
    );
}

fn two_alphas(tag: &str) -> Dirs {
    let dirs = Dirs::new(tag);
    dirs.save("aaa/alpha", &[(1, "Aaa one", &[])]);
    dirs.save(
        "owner/alpha",
        &[(1, "Owner one", &[]), (2, "Owner two", &[])],
    );
    std::fs::create_dir_all(dirs.root.join("dev").join("alpha")).unwrap();
    dirs
}

const AMBIGUOUS: &str = "alpha matches aaa/alpha and owner/alpha; name it as <owner>/alpha";

#[test]
fn star_with_an_ambiguous_name_names_both() {
    let dirs = two_alphas("amb-star");
    let out = dirs.run(&["star", "alpha#2"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stderr(&out), format!("switchboard: {AMBIGUOUS}\n"));
    let out = dirs.run(&["star", "owner/alpha#2"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(stdout(&out).starts_with("owner/alpha#2: Owner two\n"));
}

#[test]
fn delete_with_an_ambiguous_name_writes_nothing() {
    let dirs = two_alphas("amb-delete");
    let out = dirs.run(&["delete", "alpha"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stderr(&out), format!("switchboard: delete: {AMBIGUOUS}\n"));
    assert!(LocalItems::new(&dirs.data()).load().unwrap().is_empty());
    // The full name is one item, linked to the repo.
    let out = dirs.run(&["delete", "owner/alpha"]);
    assert_eq!(out.status.code(), Some(1));
    let items = LocalItems::new(&dirs.data()).load().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].link, "https://github.com/owner/alpha");
}

#[test]
fn merge_with_an_ambiguous_name_is_refused_before_any_clone_is_used() {
    let dirs = two_alphas("amb-merge");
    let out = dirs
        .command(&["--role", "project-manager", "merge", "alpha#1"])
        .env("SWITCHBOARD_PIPELINE", dirs.root.join("pipeline"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stderr(&out), format!("merge alpha#1: {AMBIGUOUS}\n"));
}

// #359: `pipeline_repo` from settings.json.

fn write_settings(config: &Path, pipeline: &Path) {
    let settings = serde_json::json!({ "pipeline_repo": pipeline });
    std::fs::write(config.join("settings.json"), settings.to_string()).unwrap();
}

#[test]
fn budget_and_reports_read_pipeline_repo_from_settings() {
    let dirs = Dirs::new("settings");
    write_settings(&dirs.config(), &fleet_stub());
    let out = dirs.run(&["budget"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).starts_with("chatgpt: 98.6% vs pace 81.4%"),
        "{}",
        stdout(&out)
    );
    let out = dirs.run(&["reports"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "no reports\n");
}

#[test]
fn the_environment_wins_over_settings() {
    let dirs = Dirs::new("settings-env");
    write_settings(&dirs.config(), &dirs.root.join("nowhere"));
    let out = dirs
        .command(&["budget"])
        .env("SWITCHBOARD_PIPELINE", fleet_stub())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
}

#[test]
fn no_pipeline_anywhere_names_both_places() {
    let dirs = Dirs::new("settings-none");
    let out = dirs.run(&["budget"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        stderr(&out),
        "switchboard: set SWITCHBOARD_PIPELINE to your pipeline checkout \
         (or pipeline_repo in settings.json)\n"
    );
}

#[test]
fn a_settings_file_that_does_not_parse_is_named() {
    let dirs = Dirs::new("settings-bad");
    std::fs::write(dirs.config().join("settings.json"), "{ not json").unwrap();
    let out = dirs.run(&["budget"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("settings.json"), "{}", stderr(&out));
}
