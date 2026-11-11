//! What `autopilot_cli.rs` and `autopilot_mcp.rs` share (#385): a pipeline with a stub
//! `fleet.py` and `merge_pr.py`, a data folder holding one repo's snapshot, and a headless
//! core over them with a fixed clock, a fake GitHub and a fake weekly backup.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use switchboard_core::autopilot::passes::AutopilotState;
use switchboard_core::backup::{BackupOutcome, RepoBackup};
use switchboard_core::clock::FixedClock;
use switchboard_core::github::snapshot::SnapshotStore;
use switchboard_core::github::transport::{GitHubTransport, RestAnswer};
use switchboard_core::github::{Issue, PullRequest, RepoSnapshot, ReviewVerdict};
use switchboard_core::CoreError;

use crate::app::autopilot::{BackupFn, Headless};

pub const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
/// 2026-11-08, a Sunday, at 6:05am Pacific.
pub const SUNDAY_AFTER_SIX: i64 = 1_794_146_700;
pub const PACIFIC: i64 = -8 * 3_600;
/// The weekly run last ran in 2020: due under the fixed clock and under the real one.
pub const LONG_AGO: i64 = 1_600_000_000;

const MERGE_STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(sys.argv[1:]) + "\n")
"#;

/// `fleet budget allow <lane>` says yes (exit 3 once a file `broken` sits beside the script);
/// `fleet run --next <lane>` prints one card; anything else exits 0. Every call is logged.
const FLEET_STUB: &str = r#"import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.log"), "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:2] == ["budget", "allow"] and os.path.exists(os.path.join(here, "broken")):
    sys.exit(3)
if args[:2] == ["run", "--next"]:
    print(json.dumps({"id": "sb-1", "outcome": "done"}))
"#;

/// The folders of one test: a pipeline (the stubs), a dev root with a clone of `demo`, the app
/// data and config folders, and a working directory that is not a job worktree.
pub struct World {
    pub base: PathBuf,
    pub pipeline: PathBuf,
    pub dev: PathBuf,
    pub data: PathBuf,
    pub config: PathBuf,
    pub cwd: PathBuf,
}

/// A world whose data folder holds `iannitram/demo` (issue 12; PR 7 from this machine with no
/// review; PR 8 with a PASS on its head and green CI; a ROADMAP.md) and a state that saw an
/// older ROADMAP and last ran the weekly run long ago. The satellite is off.
pub fn world(tag: &str) -> World {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("sb-ap-{tag}-{}-{nanos}", std::process::id()));
    let w = World {
        pipeline: base.join("pipeline"),
        dev: base.join("dev"),
        data: base.join("data"),
        config: base.join("config"),
        cwd: base.join("cwd"),
        base,
    };
    for (dir, name, text) in [
        ("merge-pr", "merge_pr.py", MERGE_STUB),
        ("fleet", "fleet.py", FLEET_STUB),
    ] {
        let dir = w.pipeline.join("tools").join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), text).unwrap();
    }
    for dir in [&w.dev.join("demo"), &w.data, &w.config, &w.cwd] {
        std::fs::create_dir_all(dir).unwrap();
    }
    SnapshotStore::new(&w.data).save(&demo()).unwrap();
    let state = AutopilotState {
        roadmaps: vec![("iannitram/demo".into(), Some("# the old plan".into()))],
        weekly_last_run: Some(LONG_AGO),
        ..Default::default()
    };
    state.save(&w.data).unwrap();
    w
}

fn demo() -> RepoSnapshot {
    let pr = |number: u64, passed: bool| PullRequest {
        draft: false,
        number,
        title: format!("PR {number}"),
        state: "open".into(),
        head_sha: HEAD.into(),
        head_ref: format!("sat/{number}"),
        labels: vec!["by:test-box".into(), "model:qwen3-coder".into()],
        closes: vec![12],
        verdicts: if passed {
            vec![ReviewVerdict {
                verdict: "PASS".into(),
                sha: HEAD.into(),
                needs_me: false,
            }]
        } else {
            vec![]
        },
        ci_on_head: "success".into(),
    };
    RepoSnapshot {
        repo: "iannitram/demo".into(),
        read_at: 1_000,
        issues: vec![Issue {
            number: 12,
            title: "Issue 12".into(),
            state: "open".into(),
            labels: vec!["size:easy".into(), "kind:wiring".into()],
            ..Default::default()
        }],
        pulls: vec![pr(7, false), pr(8, true)],
        roadmap_md: Some("# the new plan".into()),
        ..Default::default()
    }
}

impl World {
    /// Turn the satellite on for the lane `gemini`.
    pub fn satellite_on(&self) {
        std::fs::write(
            self.config.join("satellite.json"),
            r#"{ "satellite_on": true, "lanes": ["gemini"] }"#,
        )
        .unwrap();
    }

    /// Make `fleet budget allow` fail from now on.
    pub fn break_the_fleet(&self) {
        std::fs::write(self.pipeline.join("tools/fleet/broken"), "").unwrap();
    }

    /// The arguments of every call of a stub (`fleet` or `merge-pr`), in order.
    pub fn calls(&self, tool: &str) -> Vec<Vec<String>> {
        std::fs::read_to_string(self.pipeline.join("tools").join(tool).join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// The fleet calls that start work: `run <job file>` and `run --next <lane>`.
    pub fn runs(&self) -> Vec<Vec<String>> {
        let mut runs = self.calls("fleet");
        runs.retain(|args| args[0] == "run");
        runs
    }

    pub fn state(&self) -> AutopilotState {
        AutopilotState::load(&self.data).unwrap()
    }

    /// Every file of the data and config folders with its bytes.
    pub fn files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = tree(&self.data);
        files.extend(tree(&self.config));
        files
    }
}

/// Every file under `dir` with its bytes.
pub fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else {
                files.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    files
}

/// Logs each repo read after a merge.
#[derive(Default)]
pub struct FakeGitHub {
    pub reads: std::sync::Arc<Mutex<Vec<String>>>,
}

impl GitHubTransport for FakeGitHub {
    fn graphql(&self, _query: &str, variables: &Value) -> Result<Value, CoreError> {
        self.reads.lock().unwrap().push(format!(
            "{}/{}",
            variables["owner"].as_str().unwrap_or("?"),
            variables["name"].as_str().unwrap_or("?")
        ));
        Ok(json!({ "repository": {} }))
    }

    fn rest_get(&self, _path: &str, _etag: Option<&str>) -> Result<RestAnswer, CoreError> {
        Err(CoreError::GitHub("no REST here".to_string()))
    }
}

/// The seams a test's headless core borrows: the clock, the repos GitHub was read for, and
/// the repos the weekly backup was asked for.
pub struct Seams {
    pub clock: FixedClock,
    pub reads: std::sync::Arc<Mutex<Vec<String>>>,
    pub backed_up: RefCell<Vec<String>>,
}

impl Default for Seams {
    fn default() -> Self {
        Seams {
            clock: FixedClock::at(SUNDAY_AFTER_SIX),
            reads: Default::default(),
            backed_up: Default::default(),
        }
    }
}

/// A fake weekly backup: logs each repo and answers that it was refused by the stub.
pub fn fake_backup<'a>(
    seams: &'a Seams,
) -> impl Fn(&[String]) -> Result<Vec<RepoBackup>, CoreError> + 'a {
    move |repos| {
        seams.backed_up.borrow_mut().extend(repos.iter().cloned());
        Ok(repos
            .iter()
            .map(|repo| (repo.clone(), Ok(BackupOutcome::Refused("stub".into()))))
            .collect())
    }
}

/// A fake GitHub that logs into `seams.reads`.
pub fn fake_github<'a>(seams: &'a Seams) -> impl Fn() -> Box<dyn GitHubTransport> + 'a {
    move || {
        Box::new(FakeGitHub {
            reads: seams.reads.clone(),
        })
    }
}

/// The headless core over `w` and the seams given, on the machine `test-box`.
pub fn headless<'a>(
    w: &World,
    seams: &'a Seams,
    transport: &'a dyn Fn() -> Box<dyn GitHubTransport>,
    backup: &'a BackupFn<'a>,
) -> Headless<'a> {
    Headless {
        data: Some(w.data.clone()),
        config: Some(w.config.clone()),
        pipeline: Some(w.pipeline.clone()),
        dev_root: Some(w.dev.clone()),
        machine: Some("test-box".into()),
        clock: &seams.clock,
        utc_offset: Some(PACIFIC),
        transport,
        backup,
        lock_wait: Duration::ZERO,
        satellite: RefCell::new(None),
    }
}
