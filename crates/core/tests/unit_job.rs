//! unit-job (#404): the fleet job and brief for a claimed unit come from its node spec file. The
//! job carries the node's files, locked tests and verify commands; the brief carries its what,
//! its expectations and its code map lines and nothing else; a unit with no claim is refused.

use std::path::{Path, PathBuf};

use serde_json::Value;
use switchboard_core::error::CoreError;
use switchboard_core::roadmap::graph::Size;
use switchboard_core::unit::claim::Claim;
use switchboard_core::unit::job::{self, Job, NodeSpec};

const REPO: &str = "example/switchboard";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn node() -> NodeSpec {
    job::read_node(&repo_root(), "unit-job").expect("the node file parses")
}

fn code_map() -> String {
    std::fs::read_to_string(repo_root().join("docs/code-map.md")).expect("the code map reads")
}

fn claim(issue: u64) -> Claim {
    Claim {
        issue,
        worker: "builder".into(),
        token: "token-1".into(),
        claimed_at: 1_000,
    }
}

fn built() -> Job {
    job::build(&node(), Size::Medium, Some(&claim(404)), REPO, &code_map())
        .expect("a claimed unit builds")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-unit-job-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_node_file_gives_what_expectations_files_tests_verify_and_code_map_entries() {
    let n = node();
    assert_eq!(n.id, "unit-job");
    assert!(
        n.what.starts_with("`switchboard unit job` writes"),
        "{}",
        n.what
    );
    assert_eq!(n.expectations.len(), 3);
    assert_eq!(
        n.files_allowed,
        [
            "crates/core/src/unit/job.rs",
            "crates/core/tests/unit_job.rs"
        ]
    );
    assert_eq!(n.test_files, ["crates/core/tests/unit_job.rs"]);
    assert_eq!(n.verify.len(), 2);
    assert_eq!(n.code_map_entries, ["crates/core/src/unit/job.rs"]);
}

#[test]
fn the_job_carries_the_node_files_locked_tests_and_verify_commands() {
    let j = built().json;
    assert_eq!(j["id"], "unit-404-unit-job");
    assert_eq!(j["issue"], 404);
    assert_eq!(j["size"], "M");
    assert_eq!(j["repo"], REPO);
    assert_eq!(j["base"], "main");
    assert_eq!(j["kind"], "small-code-change-exact-brief");
    assert_eq!(j["brief"], "exact");
    assert!(j["lane"].is_null());
    assert!(j["title"].as_str().unwrap().contains("#404"));
    assert!(j["est_minutes"].as_f64().unwrap() > 0.0);
    assert_eq!(j["files_allowed"], serde_json::json!(node().files_allowed));
    assert_eq!(j["test_files"], serde_json::json!(node().test_files));
    assert_eq!(j["verify"], Value::Array(node().verify));
}

#[test]
fn the_brief_has_what_expectations_and_code_map_lines_and_nothing_from_outside_the_node() {
    let brief = built().brief;
    let n = node();
    assert!(brief.contains(&n.what), "{brief}");
    for expectation in &n.expectations {
        assert!(
            brief.contains(expectation.as_str()),
            "{expectation}\n{brief}"
        );
    }
    let own = code_map()
        .lines()
        .find(|l| l.starts_with("`crates/core/src/unit/job.rs`"))
        .expect("the code map has the job line")
        .to_string();
    assert!(brief.contains(&own), "{brief}");
    let other = code_map()
        .lines()
        .find(|l| l.starts_with("`crates/core/src/unit/claim.rs`"))
        .expect("the code map has the claim line")
        .to_string();
    assert!(!brief.contains(&other), "{brief}");
    assert!(!brief.contains("unit-claim"), "{brief}");
    assert!(!brief.contains("ROADMAP"), "{brief}");
}

#[test]
fn a_unit_with_no_claim_is_refused() {
    let err = job::build(&node(), Size::Medium, None, REPO, &code_map()).unwrap_err();
    assert!(matches!(err, CoreError::Refused(_)), "{err:?}");
}

#[test]
fn a_code_map_entry_with_no_line_is_refused() {
    let err = job::code_map_lines(
        "`crates/core/src/unit/claim.rs` · unit-claim (#403) · present.\n",
        &["crates/core/src/unit/job.rs".to_string()],
    )
    .unwrap_err();
    assert!(matches!(err, CoreError::Refused(_)), "{err:?}");
}

#[test]
fn write_puts_the_job_file_and_its_brief_in_the_jobs_folder_with_the_fields_the_job_check_needs() {
    let root = scratch("write");
    let jobs = root.join("jobs");
    let job = built();
    let path = job::write(&job, &jobs, &root.join("dev")).unwrap();
    assert_eq!(path, jobs.join("unit-404-unit-job.json"));

    let on_disk: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for key in [
        "id",
        "title",
        "kind",
        "size",
        "repo",
        "base",
        "clone",
        "prompt_file",
        "est_minutes",
        "issue",
        "verify",
        "files_allowed",
    ] {
        assert!(!on_disk[key].is_null(), "missing {key}");
    }
    let brief_path = jobs.join("unit-404-unit-job.md");
    assert_eq!(on_disk["prompt_file"], brief_path.to_str().unwrap());
    assert_eq!(std::fs::read_to_string(&brief_path).unwrap(), job.brief);
    assert_eq!(
        on_disk["clone"],
        root.join("dev").join("switchboard").to_str().unwrap()
    );
    let _ = std::fs::remove_dir_all(root);
}
