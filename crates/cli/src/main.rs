//! `switchboard`: the CLI face (CLI-1 to CLI-3) and `switchboard mcp` (MCP-1 to MCP-3). Both
//! are thin calls into the core; neither holds a rule of its own.

// The test crates compile this file in as a module and leave `main` and others unused.
#![allow(dead_code)]

mod cli;
mod mcp;

use std::process::ExitCode;

use switchboard_core::clock::SystemClock;
use switchboard_core::fleet::FleetBridge;
use switchboard_core::rules;

/// The parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// `--role <role>`: unlocks that role's write actions (CLI-3, MCP-2).
    pub role: Option<String>,
    /// `--json`: print JSON instead of text (CLI-1).
    pub json: bool,
    /// The command and its words, e.g. `["star", "switchboard#12"]`.
    pub command: Vec<String>,
}

/// Parse `std::env::args` (without the program name). Flags may come anywhere.
pub fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        role: None,
        json: false,
        command: Vec::new(),
    };
    let mut words = argv.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--json" => args.json = true,
            "--role" => match words.next() {
                Some(value) if !value.starts_with("--") => args.role = Some(value.clone()),
                _ => return Err("--role needs a value".to_string()),
            },
            flag if flag.starts_with("--") => return Err(format!("unknown flag: {flag}")),
            _ => args.command.push(word.clone()),
        }
    }
    Ok(args)
}

/// The real core behind `switchboard mcp`: each read tool answers with what the CLI command of
/// the same name prints with `--json`. `data` is the app data folder (`None` when unknown).
struct Core {
    data: Option<std::path::PathBuf>,
}

impl Core {
    fn data(&self) -> Result<&std::path::Path, String> {
        self.data
            .as_deref()
            .ok_or_else(|| format!("set {} to the app data folder", cli::DATA_ENV))
    }
}

impl mcp::ReadCore for Core {
    fn call(&self, tool: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let text = |key: &str| args.get(key).and_then(|v| v.as_str());
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map_or(usize::MAX, |n| n as usize);
        let answer = match tool {
            "sky" => cli::sky_answer(self.data()?),
            "constellation" => {
                let repo = text("repo").ok_or("constellation: repo is required")?;
                cli::constellation_answer(self.data()?, repo)
            }
            "star" => {
                let repo = text("repo").ok_or("star: repo is required")?;
                let issue = args
                    .get("issue")
                    .and_then(|v| v.as_u64())
                    .ok_or("star: issue is required")?;
                cli::star_answer(self.data()?, &format!("{repo}#{issue}"))
            }
            "waiting" => cli::waiting_items(self.data()?).and_then(|mut items| {
                items.truncate(limit);
                serde_json::to_value(items).map_err(cli::Fail::from)
            }),
            "reports" => cli::reports_answer().and_then(|mut reports| {
                reports.truncate(limit);
                serde_json::to_value(reports).map_err(cli::Fail::from)
            }),
            "budget" => cli::budget_rows()
                .and_then(|rows| serde_json::to_value(rows).map_err(cli::Fail::from)),
            other => return Err(format!("unknown tool: {other}")),
        };
        answer.map_err(|fail| fail.message)
    }
}

/// The usage text `switchboard mcp --help` prints.
pub const MCP_USAGE: &str = "usage: switchboard [--role <role>] mcp\n\
                             Serves the MCP tools over stdin and stdout (MCP-1 to MCP-3); \
                             --role <role> unlocks that role's write tools (MCP-2).";

/// What a command line that names `mcp` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpCall {
    /// Serve, with the `--role` flag's role (`None` without one).
    Serve(Option<rules::Role>),
    /// `--help` or `-h` anywhere on the line: print `MCP_USAGE`.
    Help,
    /// `--version` or `-V` anywhere on the line: print the version.
    Version,
}

/// MCP-2 (#354): the command line read as `switchboard mcp`, or `None` when its command is not
/// `mcp`. `--role <role>` (or `--role=<role>`) and `--json` may come before or after `mcp`; any
/// other flag, an extra word, or a role that is not one is an error, so `serve` never starts.
pub fn mcp_call(argv: &[String]) -> Option<Result<McpCall, String>> {
    let mut words = argv.iter();
    let mut role: Option<&str> = None;
    let mut command: Vec<&str> = Vec::new();
    let mut help = false;
    let mut version = false;
    let mut error: Option<String> = None;
    while let Some(word) = words.next() {
        match word.as_str() {
            "--role" => match words.next() {
                Some(value) if !value.starts_with("--") => role = Some(value),
                _ => {
                    error.get_or_insert_with(|| "--role needs a value".to_string());
                }
            },
            "--json" => {}
            "--help" | "-h" => help = true,
            "--version" | "-V" => version = true,
            flag if flag.starts_with("--role=") => role = Some(&flag["--role=".len()..]),
            flag if flag.starts_with("--") => {
                error.get_or_insert_with(|| format!("unknown flag: {flag}"));
            }
            other => command.push(other),
        }
    }
    if command.first() != Some(&"mcp") {
        return None;
    }
    if help {
        return Some(Ok(McpCall::Help));
    }
    if version {
        return Some(Ok(McpCall::Version));
    }
    if let Some(e) = error {
        return Some(Err(e));
    }
    if let Some(extra) = command.get(1) {
        return Some(Err(format!("mcp takes no other words, got {extra}")));
    }
    Some(match role {
        None => Ok(McpCall::Serve(None)),
        Some(value) => rules::Role::parse(value)
            .map(|role| McpCall::Serve(Some(role)))
            .ok_or_else(|| format!("unknown role: {value}")),
    })
}

/// The arguments as text (#335): a word that is not valid UTF-8 is an error naming it, so the
/// CLI never panics on one.
pub fn utf8_args(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Vec<String>, String> {
    args.into_iter()
        .map(|word| {
            word.into_string().map_err(|word| {
                format!(
                    "an argument is not valid UTF-8: {}",
                    word.to_string_lossy().escape_debug()
                )
            })
        })
        .collect()
}

fn main() -> ExitCode {
    let argv = match utf8_args(std::env::args_os().skip(1)) {
        Ok(argv) => argv,
        Err(e) => {
            eprintln!("switchboard: {e}");
            eprintln!("{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    let role = match mcp_call(&argv) {
        None => return cli::run(&argv),
        Some(Ok(McpCall::Help)) => return cli::emit(&format!("{MCP_USAGE}\n")),
        Some(Ok(McpCall::Version)) => return cli::emit(&format!("{}\n", cli::version_text())),
        Some(Ok(McpCall::Serve(role))) => role,
        Some(Err(e)) => {
            eprintln!("switchboard mcp: {e}");
            eprintln!("{MCP_USAGE}");
            return ExitCode::from(2);
        }
    };
    // MCP-2: the real write core: the CLI's own merge, the pipeline checkout and the
    // app data folder from the environment, the real clock.
    let merge = cli::real_merge;
    let writes = mcp::CoreWrites {
        merge: &merge,
        fleet: cli::pipeline_dir().map(|root| FleetBridge::new(&root)),
        data: cli::data_dir(),
        clock: &SystemClock,
    };
    let stdin = std::io::stdin();
    let core = Core {
        data: cli::data_dir(),
    };
    match mcp::serve(stdin.lock(), std::io::stdout().lock(), &core, role, &writes) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("switchboard mcp: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp::ReadCore;
    use serde_json::json;
    use switchboard_core::github::snapshot::SnapshotStore;
    use switchboard_core::github::{Issue, PullRequest, RepoSnapshot};

    /// A stub app data folder holding one repo snapshot with an open issue and a PR closing it.
    fn stub_data(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("switchboard-core-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snap = RepoSnapshot {
            repo: "owner/alpha".to_string(),
            issues: vec![Issue {
                number: 2,
                title: "Core two".to_string(),
                state: "open".to_string(),
                labels: vec!["size:medium".to_string()],
                milestone: Some("1.0".to_string()),
                body: String::new(),
                closed_by_pr: None,
            }],
            pulls: vec![PullRequest {
                number: 10,
                title: "PR 10".to_string(),
                state: "open".to_string(),
                closes: vec![2],
                ci_on_head: "success".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        SnapshotStore::new(&dir).save(&snap).expect("save snapshot");
        dir
    }

    #[test]
    fn read_tools_answer_data_from_the_stub_folder() {
        let core = Core {
            data: Some(stub_data("answers")),
        };
        let sky = core.call("sky", &json!({})).expect("sky");
        assert_eq!(sky[0]["repo"], "owner/alpha");
        assert_eq!(sky[0]["open"], 1);
        let star = core
            .call("star", &json!({ "repo": "alpha", "issue": 2 }))
            .expect("star");
        assert_eq!(star["title"], "Core two");
        assert_eq!(star["pr"]["number"], 10);
        let constellation = core
            .call("constellation", &json!({ "repo": "alpha" }))
            .expect("constellation");
        assert_eq!(constellation["repo"], "owner/alpha");
        let waiting = core
            .call("waiting", &json!({ "limit": 5 }))
            .expect("waiting");
        assert!(waiting.is_array());
    }

    #[test]
    fn unknown_tool_repo_and_star_are_errors() {
        let core = Core {
            data: Some(stub_data("errors")),
        };
        assert!(core
            .call("nope", &json!({}))
            .unwrap_err()
            .contains("unknown tool"));
        assert!(core
            .call("constellation", &json!({ "repo": "zzz" }))
            .is_err());
        assert!(core
            .call("star", &json!({ "repo": "alpha", "issue": 99 }))
            .is_err());
        assert!(Core { data: None }.call("sky", &json!({})).is_err());
    }
}
