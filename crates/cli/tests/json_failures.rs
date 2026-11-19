//! #491: every failure under `--json` prints the `{"error": ..}` envelope on stdout, with the
//! same exit code and the same stderr text as without it. Run as the built binary over temp
//! folders.

use std::path::PathBuf;
use std::process::{Command, Output};

struct Dirs {
    root: PathBuf,
}

impl Dirs {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("sb-jsonfail-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for sub in ["data", "config", "state", "pipeline"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        Self { root }
    }

    fn run(&self, argv: &[&str], pipeline: bool) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_switchboard"));
        cmd.args(argv)
            .current_dir(&self.root)
            .env("SWITCHBOARD_DATA", self.root.join("data"))
            .env("SWITCHBOARD_CONFIG", self.root.join("config"))
            .env("FLEET_STATE_DIR", self.root.join("state"))
            .env_remove("FLEET_ROOT");
        if pipeline {
            cmd.env("SWITCHBOARD_PIPELINE", self.root.join("pipeline"));
        } else {
            cmd.env_remove("SWITCHBOARD_PIPELINE");
        }
        cmd.output().expect("run switchboard")
    }
}

fn envelope(out: &Output) -> serde_json::Value {
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json on stdout");
    answer["error"].clone()
}

#[test]
fn a_bad_settings_file_is_an_envelope_under_json_for_settings_and_backup() {
    let dirs = Dirs::new("settings");
    std::fs::write(dirs.root.join("config").join("settings.json"), "{ not json").unwrap();
    for argv in [
        &["--json", "settings"][..],
        &["--json", "backup", "o/r"][..],
    ] {
        let out = dirs.run(argv, false);
        // Catches: a settings or backup load failure printing only stderr under --json.
        assert_eq!(out.status.code(), Some(1), "{argv:?}");
        assert_eq!(envelope(&out)["code"], "parse", "{argv:?}");
        // Catches: the envelope replacing the stderr line instead of joining it.
        assert!(!out.stderr.is_empty(), "{argv:?}");
    }
}

#[test]
fn the_reports_load_failure_is_an_envelope_under_json() {
    let dirs = Dirs::new("reports");
    let jobs = dirs.root.join("state").join("jobs");
    std::fs::create_dir_all(&jobs).unwrap();
    // `jobs/done` as a file: reading it as a folder fails with something other than not found.
    std::fs::write(jobs.join("done"), "x").unwrap();
    let plain = dirs.run(&["reports"], true);
    let json = dirs.run(&["--json", "reports"], true);
    // Catches: the reports load failure printing no envelope under --json.
    assert_eq!(envelope(&json)["code"], "file");
    // Catches: --json changing the exit code or the stderr text of the failure.
    assert_eq!(plain.status.code(), Some(1));
    assert_eq!(json.status.code(), plain.status.code());
    assert_eq!(json.stderr, plain.stderr);
}

#[test]
fn an_unset_pipeline_exits_2_with_and_without_json() {
    let dirs = Dirs::new("pipeline");
    let plain = dirs.run(&["reports"], false);
    let json = dirs.run(&["--json", "reports"], false);
    // Catches: --json recomputing the exit code of a missing setting as 1.
    assert_eq!(plain.status.code(), Some(2));
    assert_eq!(json.status.code(), Some(2));
    // Catches: a missing setting without the setting_missing code.
    assert_eq!(envelope(&json)["code"], "setting_missing");
}

#[test]
fn no_command_prints_the_usage_once_and_exactly_as_before() {
    let dirs = Dirs::new("nocommand");
    let plain = dirs.run(&[], false);
    let stderr = String::from_utf8_lossy(&plain.stderr).into_owned();
    // Catches: the usage text gaining a "switchboard: " prefix or being printed twice.
    assert!(stderr.starts_with("usage: switchboard"), "{stderr}");
    assert_eq!(stderr.matches("usage: switchboard").count(), 1, "{stderr}");
    assert_eq!(plain.status.code(), Some(2));
    let json = dirs.run(&["--json"], false);
    // Catches: the no-command usage error missing its envelope under --json.
    assert_eq!(envelope(&json)["code"], "usage");
    assert_eq!(json.stderr, plain.stderr);
}

#[test]
fn the_mcp_read_tool_returns_the_same_code_as_the_cli_for_the_same_failure() {
    use std::io::Write;
    use std::process::Stdio;
    let dirs = Dirs::new("mcp-same");
    fn bare<'a>(cmd: &'a mut Command, dirs: &Dirs) -> &'a mut Command {
        cmd.current_dir(&dirs.root)
            .env_remove("SWITCHBOARD_DATA")
            .env_remove("HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("APPDATA")
    }
    let mut cli = Command::new(env!("CARGO_BIN_EXE_switchboard"));
    let cli = bare(cli.args(["--json", "sky"]), &dirs)
        .output()
        .expect("run cli");
    let mut mcp = Command::new(env!("CARGO_BIN_EXE_switchboard"));
    let mut child = bare(mcp.arg("mcp"), &dirs)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("run mcp");
    let call =
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"sky","arguments":{}}}"#;
    writeln!(child.stdin.take().unwrap(), "{call}").unwrap();
    let out = child.wait_with_output().unwrap();
    let reply: serde_json::Value =
        serde_json::from_slice(out.stdout.split(|b| *b == b'\n').next().unwrap()).unwrap();
    let body: serde_json::Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    // Catches: the MCP read tool and the CLI naming the same failure with different codes.
    assert_eq!(reply["result"]["isError"], true);
    assert_eq!(body["code"], envelope(&cli)["code"]);
    assert_eq!(body["code"], "setting_missing");
}
