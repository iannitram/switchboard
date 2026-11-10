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

/// MCP-2: the `--role <role>` flag of `switchboard mcp`; `None` without one. Any other value
/// is an error, so `serve` never starts.
fn mcp_role(words: &[String]) -> Result<Option<rules::Role>, String> {
    let mut words = words.iter();
    let mut role = None;
    while let Some(word) = words.next() {
        let value = match word.as_str() {
            "--role" => words
                .next()
                .map(String::as_str)
                .ok_or_else(|| "--role needs a value".to_string())?,
            other => match other.strip_prefix("--role=") {
                Some(value) => value,
                None => continue,
            },
        };
        role = Some(rules::Role::parse(value).ok_or_else(|| format!("unknown role: {value}"))?);
    }
    Ok(role)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match argv.first().map(String::as_str) {
        Some("mcp") => {
            let role = match mcp_role(&argv[1..]) {
                Ok(role) => role,
                Err(e) => {
                    eprintln!("switchboard mcp: {e}");
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
        _ => cli::run(&argv),
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
