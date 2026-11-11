//! #384: the front door with no window: `ask "<text>" [--dry-run] [--thread <id>]`,
//! `ask_as_job <kind> "<text>" --unmet <need>[,<need>]` and `rate <thread> <turn> <up|down>`,
//! and the MCP tools of the same names, run as the built binary. The fleet is a stub `fleet.py`
//! that logs every call; the model is a local HTTP stub that counts its requests. No paid API,
//! no agent CLI and no GitHub is called.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

const MACHINE: &str = "strix-halo";
const MODEL: &str = "qwen3.8-27b";
const ASK: &str = "what is a worktree";
const ANSWER: &str = "A second checkout of one repo.";

/// Logs each call (argv, and the stdin of `attach`) to `calls.log` beside `tools/`; `actions`
/// prints the fixture's index, `budget` exits STUB_BUDGET_EXIT (75 for a lane in STUB_BRAKED),
/// `attach` STUB_ATTACH_EXIT.
const FLEET: &str = r#"import json, os, pathlib, sys
root = pathlib.Path(__file__).resolve().parent.parent.parent
argv = sys.argv[1:]
data = sys.stdin.read() if argv[:1] == ["attach"] else ""
with open(root / "calls.log", "a", encoding="utf-8", newline="\n") as fh:
    fh.write(json.dumps({"argv": argv, "stdin": data}) + "\n")
if argv[:1] == ["actions"]:
    sys.stdout.write((root / "answers" / "actions.json").read_text(encoding="utf-8"))
    sys.exit(0)
if argv[:1] == ["budget"]:
    if argv[2:3] and argv[2] in os.environ.get("STUB_BRAKED", "").split(","):
        sys.exit(75)
    sys.exit(int(os.environ.get("STUB_BUDGET_EXIT", "0")))
if argv[:1] == ["attach"]:
    sys.exit(int(os.environ.get("STUB_ATTACH_EXIT", "0")))
sys.exit(2)
"#;

/// The folders one run uses, and the model stub's request count.
struct World {
    root: PathBuf,
    data: PathBuf,
    home: PathBuf,
    cwd: PathBuf,
    worktree: PathBuf,
    asked: Arc<AtomicUsize>,
}

/// A local HTTP server that answers every request as a chat completion and counts them.
fn model_stub() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request).to_string();
                let Some(head_end) = text.find("\r\n\r\n") else {
                    continue;
                };
                let length = text[..head_end]
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                if request.len() >= head_end + 4 + length {
                    break;
                }
            }
            seen.fetch_add(1, Ordering::SeqCst);
            let body = json!({
                "choices": [{ "message": { "content": ANSWER } }],
                "usage": { "prompt_tokens": 10, "completion_tokens": 4 }
            })
            .to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (port, count)
}

fn world(tag: &str) -> World {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("sb-fd384-{tag}-{}-{nanos}", std::process::id()));
    let root = base.join("pipeline");
    for sub in ["tools/fleet", "answers"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../core/tests/fixtures/fleet-stub/answers/actions.json");
    std::fs::copy(fixture, root.join("answers/actions.json")).unwrap();
    std::fs::write(root.join("tools/fleet/fleet.py"), FLEET).unwrap();
    let (port, asked) = model_stub();
    let config = json!({ "machines": { MACHINE: {
        "qwen_capable": true,
        "qwen_model": MODEL,
        "qwen_server_url": format!("http://127.0.0.1:{port}/v1/models")
    } } });
    std::fs::write(root.join("tools/fleet/config.json"), config.to_string()).unwrap();
    let world = World {
        root,
        data: base.join("data"),
        home: base.join("home"),
        cwd: base.join("cwd"),
        // A job worktree by its name: `fleet-*` under a folder ending in `-wt`.
        worktree: base.join("repo-wt").join("fleet-7"),
        asked,
    };
    for dir in [&world.data, &world.home, &world.cwd, &world.worktree] {
        std::fs::create_dir_all(dir).unwrap();
    }
    world
}

impl World {
    /// The binary with every folder it reads set to this world; no service key is set.
    fn command(&self, argv: &[&str], cwd: &Path) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_switchboard"));
        cmd.args(argv)
            .current_dir(cwd)
            .env("SWITCHBOARD_PIPELINE", &self.root)
            .env("SWITCHBOARD_DATA", &self.data)
            .env("SWITCHBOARD_CONFIG", self.data.join("no-config"))
            .env("SWITCHBOARD_STATE", self.data.join("no-state"))
            .env("SWITCHBOARD_DEV_ROOT", self.data.join("no-dev-root"))
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("FLEET_MACHINE", MACHINE)
            .env("NO_PROXY", "127.0.0.1")
            .env_remove("FLEET_ROOT")
            .env_remove("FLEET_STATE_DIR")
            .env_remove("STUB_BUDGET_EXIT")
            .env_remove("STUB_BRAKED")
            .env_remove("STUB_ATTACH_EXIT")
            .stderr(Stdio::piped());
        for key in [
            "DEEPSEEK_API_KEY",
            "OPENAI_API_KEY",
            "GEMINI_API_KEY",
            "ANTHROPIC_API_KEY",
        ] {
            cmd.env_remove(key);
        }
        cmd
    }

    fn run(&self, argv: &[&str]) -> Output {
        self.run_with(argv, &[])
    }

    fn run_with(&self, argv: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = self.command(argv, &self.cwd);
        cmd.envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::piped());
        cmd.output().expect("run switchboard")
    }

    fn run_in(&self, argv: &[&str], cwd: &Path) -> Output {
        let mut cmd = self.command(argv, cwd);
        cmd.stdin(Stdio::null()).stdout(Stdio::piped());
        cmd.output().expect("run switchboard")
    }

    /// `switchboard [--role <role>] mcp` in `cwd`, fed `lines`; each reply.
    fn mcp(&self, role: Option<&str>, cwd: &Path, lines: &[Value]) -> Vec<Value> {
        self.mcp_with(role, cwd, &[], lines)
    }

    /// `mcp` with more environment.
    fn mcp_with(
        &self,
        role: Option<&str>,
        cwd: &Path,
        env: &[(&str, &str)],
        lines: &[Value],
    ) -> Vec<Value> {
        let mut argv = vec!["mcp"];
        if let Some(role) = role {
            argv.extend(["--role", role]);
        }
        let mut cmd = self.command(&argv, cwd);
        cmd.envs(env.iter().copied());
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("run switchboard mcp");
        let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        stdout(&out)
            .lines()
            .map(|l| serde_json::from_str(l).expect("a JSON reply"))
            .collect()
    }

    /// The fleet commands the stub was given, each as its words joined, and `attach`'s stdin.
    fn fleet_calls(&self) -> Vec<(String, String)> {
        std::fs::read_to_string(self.root.join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(|line| {
                let v: Value = serde_json::from_str(line).unwrap();
                let argv: Vec<&str> = v["argv"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap())
                    .collect();
                (argv.join(" "), v["stdin"].as_str().unwrap().to_string())
            })
            .collect()
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }

    /// The stored threads' ids.
    fn threads(&self) -> Vec<String> {
        let Ok(dir) = std::fs::read_dir(self.data.join("threads")) else {
            return Vec::new();
        };
        dir.map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter_map(|name| name.strip_suffix(".jsonl").map(str::to_string))
            .collect()
    }

    /// A thread's rows.
    fn rows(&self, thread: &str) -> Vec<Value> {
        let path = self.data.join("threads").join(format!("{thread}.jsonl"));
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// One real ask as the project manager; the thread it was stored in.
    fn ask(&self) -> String {
        let out = self.run(&pm(&["--json", "ask", ASK]));
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        json_out(&out)["handoff_id"].as_str().unwrap().to_string()
    }
}

fn pm<'a>(argv: &[&'a str]) -> Vec<&'a str> {
    let mut all = vec!["--role", "project-manager"];
    all.extend_from_slice(argv);
    all
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn json_out(out: &Output) -> Value {
    serde_json::from_str(&stdout(out)).unwrap_or_else(|e| panic!("{e}: {}", stdout(out)))
}

/// Exit 2 with the usage line and `message` on stderr, nothing on stdout.
fn assert_usage(out: &Output, message: &str) {
    assert_eq!(out.status.code(), Some(2), "{}", stderr(out));
    assert!(stderr(out).contains(message), "{}", stderr(out));
    assert!(
        stderr(out).contains("usage: switchboard"),
        "{}",
        stderr(out)
    );
    assert_eq!(stdout(out), "");
}

/// Exit 1 with exactly `message` on stderr, nothing on stdout.
fn assert_refused(out: &Output, message: &str) {
    assert_eq!(out.status.code(), Some(1), "{}", stderr(out));
    assert_eq!(stderr(out).trim_end(), message);
    assert_eq!(stdout(out), "");
}

fn call(id: u64, name: &str, arguments: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } })
}

// --- ask --dry-run ---

#[test]
fn a_dry_run_prints_the_route_and_its_reason_and_sends_stores_and_spends_nothing() {
    let w = world("dry");
    let out = w.run(&["ask", ASK, "--dry-run"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        format!(
            "route: qwen {MODEL}\nreason: labeller, kind quick-lookup\n\
             dry run: nothing sent, nothing stored, nothing spent\n"
        )
    );
    assert_eq!(w.asked(), 0);
    assert!(w.threads().is_empty());
    let calls = w.fleet_calls();
    assert!(!calls.is_empty());
    for (argv, _) in &calls {
        assert!(
            argv.starts_with("actions") || argv.starts_with("budget allow"),
            "{argv}"
        );
    }
}

#[test]
fn a_dry_run_as_json_is_the_route_with_dry_run_true() {
    let w = world("dry-json");
    let out = w.run(&["ask", "--dry-run", "--json", ASK]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        json_out(&out),
        json!({
            "step": "labeller",
            "kind": "quick-lookup",
            "pick": { "to": "service", "service": "qwen", "model": MODEL },
            "dry_run": true
        })
    );
    assert_eq!(w.asked(), 0);
    assert!(w.threads().is_empty());
}

#[test]
fn a_dry_run_needs_no_role_and_runs_in_a_job_worktree() {
    let w = world("dry-worktree");
    let out = w.run_in(&pm(&["ask", ASK, "--dry-run"]), &w.worktree);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).starts_with("route: qwen "), "{}", stdout(&out));
    assert!(w.threads().is_empty());
}

#[test]
fn a_dry_run_with_every_pool_braked_says_wait_and_queues_nothing() {
    let w = world("dry-braked");
    let out = w.run_with(&["ask", ASK, "--dry-run"], &[("STUB_BUDGET_EXIT", "75")]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).starts_with("route: wait\n"),
        "{}",
        stdout(&out)
    );
    assert!(w.threads().is_empty());
    assert_eq!(w.asked(), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn a_dry_run_whose_output_cannot_be_written_is_exit_1() {
    let w = world("dry-full");
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let mut cmd = w.command(&["ask", ASK, "--dry-run"], &w.cwd);
    let out = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::from(full))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("cannot write the output"),
        "{}",
        stderr(&out)
    );
    assert!(!stderr(&out).contains("panicked"));
}

// --- ask ---

#[test]
fn an_ask_without_the_role_is_refused_before_anything_runs() {
    let w = world("ask-role");
    let refusal = "ask: only the project-manager role may use the front door";
    assert_refused(&w.run(&["ask", ASK]), refusal);
    assert_refused(&w.run(&["--role", "satellite", "ask", ASK]), refusal);
    assert_refused(&w.run(&["--role", "budget-manager", "ask", ASK]), refusal);
    assert!(w.fleet_calls().is_empty());
    assert_eq!(w.asked(), 0);
    assert!(w.threads().is_empty());
}

#[test]
fn an_ask_in_a_job_worktree_is_refused_whatever_the_role() {
    let w = world("ask-worktree");
    let out = w.run_in(&pm(&["ask", ASK]), &w.worktree);
    assert_refused(&out, "ask: refused inside a job worktree");
    assert!(w.fleet_calls().is_empty());
    assert_eq!(w.asked(), 0);
}

#[test]
fn an_ask_is_answered_once_and_both_turns_are_stored() {
    let w = world("ask");
    let out = w.run(&pm(&["ask", ASK]));
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let threads = w.threads();
    assert_eq!(threads.len(), 1);
    assert_eq!(
        stdout(&out),
        format!(
            "route: qwen {MODEL}\nreason: labeller, kind quick-lookup\nthread: {}\n\
             answer:\n{ANSWER}\n",
            threads[0]
        )
    );
    assert_eq!(w.asked(), 1);
    let rows = w.rows(&threads[0]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["role"], "user");
    assert_eq!(rows[0]["text"], ASK);
    assert_eq!(rows[1]["role"], "assistant");
    assert_eq!(rows[1]["text"], ANSWER);
}

#[test]
fn an_ask_as_json_is_the_apps_reply_with_dry_run_false() {
    let w = world("ask-json");
    let out = w.run(&pm(&["--json", "ask", ASK]));
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let threads = w.threads();
    assert_eq!(
        json_out(&out),
        json!({
            "step": "labeller",
            "kind": "quick-lookup",
            "pick": { "to": "service", "service": "qwen", "model": MODEL },
            "handoff_id": threads[0],
            "text": ANSWER,
            "service": "qwen",
            "model": MODEL,
            "dry_run": false
        })
    );
}

#[test]
fn a_follow_up_goes_into_the_named_thread() {
    let w = world("follow");
    let thread = w.ask();
    let out = w.run(&pm(&[
        "--json",
        "ask",
        "and how do I remove one",
        "--thread",
        &thread,
    ]));
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(json_out(&out)["handoff_id"], thread.as_str());
    assert_eq!(w.threads(), vec![thread.clone()]);
    assert_eq!(w.rows(&thread).len(), 4);
    assert_eq!(w.asked(), 2);
}

#[test]
fn an_unknown_thread_is_refused_dry_or_not() {
    let w = world("no-thread");
    let refusal = "ask: refused: no thread chat-nope";
    assert_refused(&w.run(&pm(&["ask", ASK, "--thread", "chat-nope"])), refusal);
    assert_refused(
        &w.run(&["ask", ASK, "--thread", "chat-nope", "--dry-run"]),
        refusal,
    );
    assert!(w.threads().is_empty());
    assert_eq!(w.asked(), 0);
    assert!(w.fleet_calls().is_empty());
}

#[test]
fn an_ask_with_every_pool_braked_is_queued_in_its_thread() {
    let w = world("braked");
    let out = w.run_with(&pm(&["--json", "ask", ASK]), &[("STUB_BUDGET_EXIT", "75")]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let reply = json_out(&out);
    assert_eq!(reply["pick"]["to"], "wait");
    assert!(reply["queued"].is_string(), "{reply}");
    assert_eq!(reply["dry_run"], false);
    let thread = reply["handoff_id"].as_str().unwrap();
    let rows = w.rows(thread);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["outcome"], "queued");
    assert_eq!(w.asked(), 0);
}

#[test]
fn an_empty_ask_is_the_cores_refusal() {
    let w = world("empty");
    assert_refused(
        &w.run(&pm(&["ask", " "])),
        "ask: refused: chat: empty message",
    );
    assert!(w.threads().is_empty());
}

#[test]
fn an_ask_with_no_pipeline_set_says_which_variable() {
    let w = world("no-pipeline");
    let mut cmd = w.command(&["ask", ASK, "--dry-run"], &w.cwd);
    let out = cmd
        .env_remove("SWITCHBOARD_PIPELINE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("SWITCHBOARD_PIPELINE"),
        "{}",
        stderr(&out)
    );
}

// --- usage ---

#[test]
fn bad_lines_are_usage_errors() {
    let w = world("usage");
    let cases: [(&[&str], &str); 13] = [
        (&["ask"], "ask needs one \"<text>\""),
        (&["ask", "what", "is", "this"], "ask needs one \"<text>\""),
        (&["ask", ASK, "--thread"], "--thread needs a value"),
        (
            &["ask", ASK, "--thread", "--dry-run"],
            "--thread needs a value",
        ),
        (&["ask", ASK, "--unmet", "web"], "--unmet is for ask_as_job"),
        (&["ask", ASK, "--nope"], "unknown flag: --nope"),
        (&["sky", "--dry-run"], "--dry-run and --thread are for ask"),
        (
            &["rate", "t", "1", "up", "--thread", "t"],
            "--dry-run and --thread are for ask",
        ),
        (&["rate", "t", "1"], "rate needs <thread> <turn> <up|down>"),
        (
            &["rate", "t", "one", "up"],
            "rate: turn must be a whole number from 0, got one",
        ),
        (
            &["rate", "t", "-1", "up"],
            "rate: turn must be a whole number from 0, got -1",
        ),
        (
            &["rate", "t", "1", "sideways"],
            "rate: rating must be up or down, got sideways",
        ),
        (
            &["ask_as_job", "code-change", "fix it"],
            "ask_as_job needs --unmet",
        ),
    ];
    for (argv, message) in cases {
        assert_usage(&w.run(&pm(argv)), message);
    }
    assert_usage(
        &w.run(&pm(&["ask_as_job", "code-change", "--unmet", "web"])),
        "ask_as_job needs <kind> \"<text>\"",
    );
    assert_usage(
        &w.run(&pm(&[
            "ask_as_job",
            "code-change",
            "fix it",
            "--unmet",
            " , ",
        ])),
        "ask_as_job needs --unmet",
    );
    assert!(w.fleet_calls().is_empty());
    assert!(w.threads().is_empty());
}

#[test]
fn the_usage_line_names_the_front_door() {
    let w = world("help");
    let out = w.run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    for part in [
        "ask \"<text>\" [--dry-run] [--thread <id>]",
        "ask_as_job <kind> \"<text>\" --unmet <need>[,<need>]",
        "rate <thread> <turn> <up|down>",
    ] {
        assert!(stdout(&out).contains(part), "{}", stdout(&out));
    }
}

#[test]
fn a_question_about_mcp_is_an_ask_not_the_server() {
    let w = world("mcp-word");
    let out = w.run(&["ask", "mcp", "--dry-run"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).starts_with("route: "), "{}", stdout(&out));
    let out = w.run(&["--thread", "mcp", "ask", ASK, "--dry-run"]);
    assert_refused(&out, "ask: refused: no thread mcp");
}

// --- rate ---

#[test]
fn rate_logs_the_rating_row_for_the_turns_own_model() {
    let w = world("rate");
    let thread = w.ask();
    let out = w.run(&pm(&["rate", &thread, "1", "up"]));
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        format!("rate {thread}#1: up for qwen {MODEL}\n")
    );
    let rows = w.rows(&thread);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2]["id"], format!("{thread}#rating-1"));
    // A second rating replaces the first; `--json` is the row.
    let out = w.run(&pm(&["--json", "rate", &thread, "1", "down"]));
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let rows = w.rows(&thread);
    assert_eq!(rows.len(), 3);
    assert_eq!(json_out(&out), rows[2]);
    assert_eq!(rows[2]["lane"], "qwen");
    assert_eq!(rows[2]["kind"], "quick-lookup");
}

#[test]
fn rate_refuses_an_unknown_thread_or_turn_and_a_question() {
    let w = world("rate-refused");
    let thread = w.ask();
    let before = w.rows(&thread);
    assert_refused(
        &w.run(&pm(&["rate", "chat-nope", "1", "up"])),
        "rate: refused: no thread chat-nope",
    );
    assert_refused(
        &w.run(&pm(&["rate", &thread, "9", "up"])),
        &format!("rate: refused: no turn 9 in thread {thread}"),
    );
    assert_refused(
        &w.run(&pm(&["rate", &thread, "0", "up"])),
        &format!("rate: refused: turn 0 of thread {thread} is not an answer"),
    );
    assert_eq!(w.rows(&thread), before);
    assert_eq!(w.threads(), vec![thread]);
}

#[test]
fn rate_without_the_role_or_in_a_job_worktree_writes_nothing() {
    let w = world("rate-role");
    let thread = w.ask();
    assert_refused(
        &w.run(&["rate", &thread, "1", "up"]),
        "rate: only the project-manager role may use the front door",
    );
    assert_refused(
        &w.run_in(&pm(&["rate", &thread, "1", "up"]), &w.worktree),
        "rate: refused inside a job worktree",
    );
    assert_eq!(w.rows(&thread).len(), 2);
}

// --- ask_as_job ---

#[test]
fn ask_as_job_hands_fleet_the_held_job_on_stdin() {
    let w = world("job");
    let out = w.run(&pm(&[
        "--json",
        "ask_as_job",
        "code-change",
        "fix the parser",
        "--unmet",
        "tools, web",
    ]));
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let job = json!({
        "kind": "code-change",
        "request": "fix the parser",
        "needs": ["tools", "web"],
        "never_on": [MACHINE]
    });
    assert_eq!(json_out(&out), job);
    let calls = w.fleet_calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(
        calls[0].0.starts_with("attach --request -"),
        "{}",
        calls[0].0
    );
    assert_eq!(serde_json::from_str::<Value>(&calls[0].1).unwrap(), job);
    let out = w.run(&pm(&[
        "ask_as_job",
        "code-change",
        "fix it",
        "--unmet",
        "web",
    ]));
    assert_eq!(
        stdout(&out),
        "ask_as_job: queued code-change for another machine (needs web)\n"
    );
}

#[test]
fn ask_as_job_with_a_thread_sends_the_chats_packet_and_stores_the_move() {
    // #378 (FD-6): the CLI and MCP send a chat as a job through the same core call.
    let w = world("job-thread");
    let thread = w.ask();
    let out = w.run(&pm(&[
        "--json",
        "ask_as_job",
        "web-research",
        "find the paper",
        "--unmet",
        "web",
        "--thread",
        &thread,
    ]));
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    // The outgoing model (the stub) wrote the summary; the job's request is the packet, then
    // the ask.
    let request = format!(
        "Summary\n{ANSWER}\n\nLast turns\nuser: {ASK}\nassistant: {ANSWER}\n\nLinks\n\n\nAsk\nfind the paper"
    );
    assert_eq!(json_out(&out)["request"], request.as_str());
    let attached = w
        .fleet_calls()
        .into_iter()
        .find(|(argv, _)| argv.starts_with("attach"))
        .expect("the job was attached");
    let job: Value = serde_json::from_str(&attached.1).unwrap();
    assert_eq!(job["request"], request.as_str());
    assert_eq!(job["kind"], "web-research");
    let rows = w.rows(&thread);
    let moved: Vec<&Value> = rows.iter().filter(|r| r["role"] == "move").collect();
    assert_eq!(moved.len(), 1, "{rows:?}");
    assert_eq!(moved[0]["moved_from"], format!("qwen {MODEL}"));
    assert_eq!(moved[0]["lane"], "fleet-job");
    assert_eq!(moved[0]["model"], "web-research");
    assert_eq!(
        moved[0]["moved_why"],
        "I sent it as a fleet job (needs web)"
    );

    // MCP's `ask_as_job` takes the same thread.
    let replies = w.mcp(
        Some("project-manager"),
        &w.cwd,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
                "name": "ask_as_job",
                "arguments": {
                    "kind": "web-research", "request": "and its data", "unmet": ["web"],
                    "thread": thread
                }
            }}),
        ],
    );
    let text = replies[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{replies:?}"));
    assert!(
        text.contains("Ask\\nand its data") && text.contains("Summary\\n"),
        "{text}"
    );
    let moves = w
        .rows(&thread)
        .iter()
        .filter(|r| r["role"] == "move")
        .count();
    assert_eq!(moves, 2);

    // A thread that is not stored is refused before fleet is called.
    let before = w.fleet_calls().len();
    let out = w.run(&pm(&[
        "ask_as_job",
        "web-research",
        "find it",
        "--unmet",
        "web",
        "--thread",
        "chat-nope",
    ]));
    assert_refused(&out, "ask_as_job: refused: no thread chat-nope");
    assert_eq!(w.fleet_calls().len(), before);
}

// --- the Qwen fallback (#377) ---

#[test]
fn an_ask_whose_pool_is_braked_is_answered_by_qwen_and_says_so() {
    // "proofread" is a writing-editing ask: gemini, then the Claude API; no Qwen row.
    const EDIT: &str = "proofread this note";
    let w = world("fallback");
    let braked = [("STUB_BRAKED", "gemini,claudeapi")];
    let out = w.run_with(&pm(&["ask", EDIT]), &braked);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    let reason = "nothing else is in budget (waiting for budget: writing-editing), \
                  so the local Qwen lane answers";
    assert!(
        text.starts_with(&format!(
            "route: qwen {MODEL}\nreason: labeller, kind writing-editing\nfallback: {reason}\n"
        )),
        "{text}"
    );
    assert!(text.ends_with(&format!("answer:\n{ANSWER}\n")), "{text}");

    // The same reply through MCP, and a dry run says where it would go.
    let replies = w.mcp_with(
        Some("project-manager"),
        &w.cwd,
        &braked,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
                "name": "ask", "arguments": { "message": EDIT, "dry_run": true }
            }}),
        ],
    );
    let text = replies[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{replies:?}"));
    let reply: Value = serde_json::from_str(text).unwrap();
    assert_eq!(reply["fallback"], reason);
    assert_eq!(reply["pick"]["service"], "qwen");
    assert_eq!(reply["dry_run"], true);

    // Qwen braked too: the ask is queued, as before.
    let out = w.run_with(
        &pm(&["--json", "ask", EDIT]),
        &[("STUB_BRAKED", "gemini,claudeapi,qwen")],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let reply = json_out(&out);
    assert_eq!(reply["queued"], "waiting for budget: writing-editing");
    assert!(reply.get("fallback").is_none(), "{reply}");
}

#[test]
fn ask_as_job_that_fleet_does_not_queue_is_exit_1() {
    let w = world("job-75");
    let out = w.run_with(
        &pm(&["ask_as_job", "code-change", "fix it", "--unmet", "web"]),
        &[("STUB_ATTACH_EXIT", "75")],
    );
    assert_refused(
        &out,
        "ask_as_job: refused: fd-12: fleet did not queue the job (exit 75)",
    );
}

#[test]
fn ask_as_job_without_the_role_or_in_a_job_worktree_calls_no_fleet() {
    let w = world("job-role");
    let argv = ["ask_as_job", "code-change", "fix it", "--unmet", "web"];
    assert_refused(
        &w.run(&argv),
        "ask_as_job: only the project-manager role may use the front door",
    );
    assert_refused(
        &w.run_in(&pm(&argv), &w.worktree),
        "ask_as_job: refused inside a job worktree",
    );
    assert!(w.fleet_calls().is_empty());
}

// --- MCP, the built binary ---

#[test]
fn mcp_project_manager_asks_dry_then_for_real_then_rates() {
    let w = world("mcp");
    let role = Some("project-manager");
    let replies = w.mcp(
        role,
        &w.cwd,
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            call(2, "ask", json!({ "message": ASK, "dry_run": true })),
        ],
    );
    let names: Vec<&str> = replies[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    // #385: `autopilot_status` comes before them and `autopilot_pass` after.
    assert_eq!(names[7..11], ["merge", "ask", "ask_as_job", "rate"]);
    let dry = &replies[1]["result"];
    assert_eq!(dry["isError"], false, "{dry}");
    let dry: Value = serde_json::from_str(dry["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["pick"]["service"], "qwen");
    assert_eq!(w.asked(), 0);
    assert!(w.threads().is_empty());

    let replies = w.mcp(role, &w.cwd, &[call(3, "ask", json!({ "message": ASK }))]);
    let asked = &replies[0]["result"];
    assert_eq!(asked["isError"], false, "{asked}");
    let asked: Value = serde_json::from_str(asked["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(asked["text"], ANSWER);
    assert_eq!(asked["dry_run"], false);
    let thread = asked["handoff_id"].as_str().unwrap();
    assert_eq!(w.asked(), 1);

    let replies = w.mcp(
        role,
        &w.cwd,
        &[
            call(
                4,
                "rate",
                json!({ "thread": thread, "turn": 1, "rating": "up" }),
            ),
            call(
                5,
                "rate",
                json!({ "thread": "chat-nope", "turn": 1, "rating": "up" }),
            ),
            call(
                6,
                "rate",
                json!({ "thread": thread, "turn": 7, "rating": "up" }),
            ),
            call(7, "ask", json!({ "message": ASK, "thread": "chat-nope" })),
            call(
                8,
                "ask_as_job",
                json!({ "kind": "code-change", "request": "fix it", "unmet": ["web"] }),
            ),
        ],
    );
    assert_eq!(replies[0]["result"]["isError"], false, "{}", replies[0]);
    let row: Value =
        serde_json::from_str(replies[0]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(row["id"], format!("{thread}#rating-1"));
    assert_eq!(w.rows(thread).len(), 3);
    let refused = |reply: &Value, text: &str| {
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert_eq!(reply["result"]["content"][0]["text"], text);
    };
    refused(&replies[1], "rate: refused: no thread chat-nope");
    refused(
        &replies[2],
        &format!("rate: refused: no turn 7 in thread {thread}"),
    );
    refused(&replies[3], "ask: refused: no thread chat-nope");
    assert_eq!(replies[4]["result"]["isError"], false, "{}", replies[4]);
    let job: Value =
        serde_json::from_str(replies[4]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(job["never_on"], json!([MACHINE]));
    assert_eq!(w.asked(), 1);
    assert_eq!(w.threads(), vec![thread.to_string()]);
}

#[test]
fn mcp_in_a_job_worktree_holds_no_front_door() {
    let w = world("mcp-worktree");
    let replies = w.mcp(
        Some("project-manager"),
        &w.worktree,
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            call(2, "ask", json!({ "message": ASK })),
        ],
    );
    assert_eq!(replies[0]["result"]["tools"].as_array().unwrap().len(), 6);
    assert_eq!(replies[1]["error"]["code"], -32602);
    assert_eq!(replies[1]["error"]["message"], "unknown tool: ask");
    assert!(w.fleet_calls().is_empty());
    assert_eq!(w.asked(), 0);
}
