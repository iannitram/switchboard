//! bug-fix (#409): the next bug by severity, report count and age; a fix job only after the repro
//! test failed on main with the reported signature and with the test locked; three tries or a
//! repeated diff park the bug with one Waiting on you item; one pitfalls line; the search
//! patterns; the status row.

use std::path::PathBuf;
use std::process::Command;

use switchboard_core::bug::fix::{
    attempts, glob_match, job, lesson_check, next, park, put_status_row, repro_path,
    signature_check, status_row_ok, Expect, GitGrep, LessonError, Main, Refusal, Repro, Request,
    Run, SignatureError, StatusRow, Stop,
};
use switchboard_core::bug::{Bug, OwnerRow};
use switchboard_core::runner::spec;
use switchboard_core::runner::waiting::RunnerItems;
use switchboard_core::CoreError;

fn bug(id: &str, issue: u64, severity: &str, reports: usize) -> Bug {
    Bug {
        id: id.into(),
        repo: "o/app".into(),
        issue,
        reports: (0..reports).map(|i| format!("r{i}")).collect(),
        error_signature: Some("panic: index out of range".into()),
        severity: severity.into(),
        status: "open".into(),
        attempts: 0,
    }
}

fn owner() -> OwnerRow {
    OwnerRow {
        app: "app".into(),
        repo: "o/app".into(),
        test_cmd: "cargo test".into(),
        test_dir: "tests/*.rs".into(),
        lint_cmd: Some("cargo clippy".into()),
        release_style: "none".into(),
        siblings: vec![],
    }
}

struct FakeMain(Run);

impl Main for FakeMain {
    fn run(&self, _: &str) -> Result<Run, CoreError> {
        Ok(self.0.clone())
    }
}

fn red() -> FakeMain {
    FakeMain(Run {
        exit: 101,
        output: "thread main panicked: panic: index out of range".into(),
    })
}

fn repro() -> Repro {
    Repro {
        test: "tests/bug_b1.rs".into(),
        cmd: "cargo test --test bug_b1".into(),
    }
}

fn scope() -> Vec<String> {
    vec!["src/**".into()]
}

#[test]
fn next_orders_by_severity_then_reports_then_age_and_skips_claimed() {
    let bugs = vec![
        bug("a", 1, "cosmetic", 9),
        bug("b", 2, "crash", 1),
        bug("c", 3, "crash", 3),
        bug("d", 4, "crash", 3),
        bug("e", 5, "data-loss", 1),
    ];
    assert_eq!(next(&bugs, &[]).unwrap().id, "e");
    assert_eq!(next(&bugs, &[("o/app", 5)]).unwrap().id, "c");
    assert_eq!(next(&bugs, &[("o/app", 5), ("o/app", 3)]).unwrap().id, "d");
    let mut other = bug("other", 5, "data-loss", 1);
    other.repo = "o/other".into();
    let mut repos = bugs.clone();
    repos.push(other);
    assert_eq!(next(&repos, &[("o/app", 5)]).unwrap().id, "other");
    let mut closed = bugs.clone();
    closed.iter_mut().for_each(|b| b.status = "closed".into());
    assert!(next(&closed, &[]).is_none());
    assert!(next(&[], &[]).is_none());
}

#[test]
fn without_a_repro_test_the_repro_job_is_written_for_one_test_file() {
    let b = bug("b1", 7, "crash", 1);
    let o = owner();
    let req = Request {
        bug: &b,
        owner: &o,
        lane: "qwen",
        repro: None,
        scope: &[],
    };
    // Main is not run for the repro job: a passing run would refuse a fix job, not this.
    let main = FakeMain(Run {
        exit: 0,
        output: String::new(),
    });
    let j = job(&req, &main).unwrap();
    assert_eq!(j.stage, "repro");
    assert_eq!(j.files_allowed, vec![repro_path(&o, &b)]);
    assert_eq!(j.files_allowed, vec!["tests/bug_b1.rs".to_string()]);
    assert!(!j.open_pr);
    assert!(j.brief.contains("panic: index out of range"));
    let mut recursive = o.clone();
    recursive.test_dir = "tests/**/*.rs".into();
    assert_eq!(repro_path(&recursive, &b), "tests/bug_b1.rs");
    recursive.test_dir = "tests/**/test_*.py".into();
    assert_eq!(repro_path(&recursive, &b), "tests/test_bug_b1.py");
}

#[test]
fn fix_job_needs_the_repro_red_on_main_with_the_reported_signature() {
    let b = bug("b1", 7, "crash", 1);
    let o = owner();
    let r = repro();
    let s = scope();
    let req = Request {
        bug: &b,
        owner: &o,
        lane: "qwen",
        repro: Some(&r),
        scope: &s,
    };

    let passing = FakeMain(Run {
        exit: 0,
        output: "ok".into(),
    });
    assert!(matches!(
        job(&req, &passing),
        Err(Refusal::NotRed { exit: 0, .. })
    ));
    let other = FakeMain(Run {
        exit: 101,
        output: "panic: something else".into(),
    });
    assert!(matches!(
        job(&req, &other),
        Err(Refusal::NotRed { exit: 101, .. })
    ));

    let j = job(&req, &red()).unwrap();
    assert_eq!(j.stage, "fix");
    assert_eq!(j.verify[0].name, "regression");
    assert_eq!(j.verify[0].cmd, r.cmd);
    assert_eq!(j.verify[0].expect, Expect::FailBefore);
    assert!(j
        .verify
        .iter()
        .any(|v| v.cmd == "cargo test" && v.expect == Expect::Pass));
    assert!(j
        .verify
        .iter()
        .any(|v| v.cmd == "cargo clippy" && v.expect == Expect::Pass));
}

#[test]
fn a_bug_with_no_signature_gets_no_job() {
    let mut b = bug("b1", 7, "crash", 1);
    b.error_signature = None;
    let o = owner();
    let req = Request {
        bug: &b,
        owner: &o,
        lane: "qwen",
        repro: None,
        scope: &[],
    };
    assert!(matches!(job(&req, &red()), Err(Refusal::NoSignature)));
}

#[test]
fn fix_job_files_allowed_excludes_the_repro_test() {
    let b = bug("b1", 7, "crash", 1);
    let o = owner();
    let r = repro();
    for glob in ["tests/*.rs", "tests/**", "tests/bug_b1.rs", "**"] {
        let s = vec!["src/**".to_string(), glob.to_string()];
        let req = Request {
            bug: &b,
            owner: &o,
            lane: "qwen",
            repro: Some(&r),
            scope: &s,
        };
        assert!(
            matches!(job(&req, &red()), Err(Refusal::TouchesRepro(g)) if g == glob),
            "{glob}"
        );
    }
    let s = scope();
    let req = Request {
        bug: &b,
        owner: &o,
        lane: "qwen",
        repro: Some(&r),
        scope: &s,
    };
    let j = job(&req, &red()).unwrap();
    assert!(j.files_allowed.iter().all(|g| !glob_match(g, &r.test)));
    for glob in ["tests/bug_b?.rs", "tests/bug_b[1].rs", r"tests\*.rs"] {
        let scope = [glob.to_string()];
        let unsafe_req = Request {
            scope: &scope,
            ..req
        };
        assert!(matches!(
            job(&unsafe_req, &red()),
            Err(Refusal::UnsupportedGlob(_))
        ));
    }
    let none = Request {
        bug: &b,
        owner: &o,
        lane: "qwen",
        repro: Some(&r),
        scope: &[],
    };
    assert!(matches!(job(&none, &red()), Err(Refusal::NoScope)));
}

#[test]
fn glob_star_stays_in_a_part_and_double_star_crosses() {
    assert!(glob_match("src/*.rs", "src/a.rs"));
    assert!(!glob_match("src/*.rs", "src/x/a.rs"));
    assert!(glob_match("src/**", "src/x/a.rs"));
    assert!(glob_match("src/**/a.rs", "src/a.rs"));
    assert!(!glob_match("src/**", "tests/a.rs"));
}

#[test]
fn diffs_stop_at_three_tries_or_a_repeat() {
    let d = |s: &str| s.to_string();
    assert_eq!(attempts(&[]), None);
    assert_eq!(attempts(&[d("a"), d("b")]), None);
    assert_eq!(attempts(&[d("a"), d("b"), d("c")]), Some(Stop::Attempts));
    assert_eq!(attempts(&[d("a"), d("b"), d("b\n")]), Some(Stop::Attempts));
    assert_eq!(attempts(&[d("a"), d("a")]), Some(Stop::SameDiff));
    assert_eq!(attempts(&[d("a"), d("b"), d("a")]), Some(Stop::Attempts));
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-bug-fix-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn bug_loop() -> spec::Spec {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    spec::load(&root.join("../../specs/bug-loop.json")).unwrap()
}

#[test]
fn the_third_failed_try_parks_the_bug_with_one_waiting_item() {
    let dir = tmp("park");
    let items = RunnerItems::new(&dir);
    let spec = bug_loop();
    let b = bug("b1", 7, "crash", 1);
    let diffs = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    assert_eq!(attempts(&diffs), Some(Stop::Attempts));

    let parked = park(&b, true, diffs.len(), &spec, &items, 100).unwrap();
    assert_eq!(parked.bug.status, "parked");
    assert_eq!(parked.bug.attempts, 3);
    let again = park(&b, true, 3, &spec, &items, 200).unwrap();
    assert_eq!(again.item.id, parked.item.id);
    let open = items.list().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].unit, "b1");
    assert_eq!(open[0].step, "owner-rules-on-parked");

    let no_repro = park(&b, false, 1, &spec, &items, 300).unwrap();
    assert_eq!(no_repro.bug.status, "unreproduced");
}

fn added(lines: &[&str]) -> String {
    let mut d = String::from("--- a/docs/pitfalls.md\n+++ b/docs/pitfalls.md\n@@ -1 +1,9 @@\n");
    for l in lines {
        d.push_str(&format!("+{l}\n"));
    }
    d
}

#[test]
fn a_fix_with_no_pitfalls_line_or_two_fails_the_lesson_check() {
    let line = "bug: b1 class: off-by-one test: tests/bug_b1.rs";
    let body = "**The loop read one past the end.**";
    let ok = added(&[body, "Check the bound first.", line]);
    assert_eq!(
        lesson_check(&ok, "b1", "tests/bug_b1.rs"),
        Ok("off-by-one".into())
    );

    let none = added(&[body, "Check the bound first."]);
    assert_eq!(
        lesson_check(&none, "b1", "tests/bug_b1.rs"),
        Err(LessonError::NoLine)
    );
    let other_bug = added(&[body, "bug: b2 class: off-by-one test: tests/bug_b1.rs"]);
    assert_eq!(
        lesson_check(&other_bug, "b1", "tests/bug_b1.rs"),
        Err(LessonError::NoLine)
    );
    let two = added(&[body, line, "bug: b1 class: other test: tests/bug_b1.rs"]);
    assert_eq!(
        lesson_check(&two, "b1", "tests/bug_b1.rs"),
        Err(LessonError::ManyLines(2))
    );

    let long = "word ".repeat(70);
    let big = added(&[&long, line]);
    assert!(matches!(
        lesson_check(&big, "b1", "tests/bug_b1.rs"),
        Err(LessonError::TooLong(_))
    ));
}

fn git(dir: &PathBuf, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn signature_patterns_hit_at_base_and_nowhere_at_head() {
    let dir = tmp("sig");
    git(&dir, &["init", "--quiet"]);
    std::fs::write(
        dir.join("caf\u{e9}.rs"),
        "let x = v[v.len()];\nlet y = unwrap_it();\nok();\n",
    )
    .unwrap();
    git(&dir, &["add", "caf\u{e9}.rs"]);
    git(&dir, &["commit", "--quiet", "-m", "base"]);
    let base = git(&dir, &["rev-parse", "HEAD"]);
    std::fs::write(dir.join("caf\u{e9}.rs"), "let x = v.get(0);\nok();\n").unwrap();
    git(&dir, &["commit", "--quiet", "-am", "fix"]);
    let head = git(&dir, &["rev-parse", "HEAD"]);

    let grep = GitGrep { clone: dir.clone() };
    let files = grep.changed(&base, &head).unwrap();
    assert_eq!(files, vec!["caf\u{e9}.rs".to_string()]);
    let p = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();

    let good = p(&[r"v\[v\.len\(\)\]", "unwrap_it"]);
    assert_eq!(
        signature_check(&grep, &good, &files, &base, &head).unwrap(),
        Ok(())
    );
    let one = p(&["unwrap_it"]);
    assert_eq!(
        signature_check(&grep, &one, &files, &base, &head).unwrap(),
        Err(SignatureError::Count(1))
    );
    let misses = p(&["unwrap_it", "never_there"]);
    assert_eq!(
        signature_check(&grep, &misses, &files, &base, &head).unwrap(),
        Err(SignatureError::MissesBase("never_there".into()))
    );
    let stays = p(&["unwrap_it", "ok"]);
    assert_eq!(
        signature_check(&grep, &stays, &files, &base, &head).unwrap(),
        Err(SignatureError::HitsHead("ok".into()))
    );
}

#[test]
fn the_status_row_replaces_its_area_and_is_never_doubled() {
    let table = "# Status\n\n| Area | State | Verified by |\n|---|---|---|\n| Import | Verified | `t1` |\n| Export | Partial | `t2` |\n";
    let row = StatusRow::fixed("Export", "tests/bug_b1.rs", "abc123");
    let once = put_status_row(table, &row);
    assert_eq!(once.matches("| Export |").count(), 1);
    assert!(once.contains("| Import | Verified | `t1` |"));
    assert!(status_row_ok(&once, "tests/bug_b1.rs", "abc123"));
    assert_eq!(put_status_row(&once, &row), once);

    let new = StatusRow::fixed("Sync", "tests/bug_b1.rs", "abc123");
    let added = put_status_row(table, &new);
    assert!(added.trim_end().ends_with(&new.line()));
    assert!(status_row_ok(&added, "tests/bug_b1.rs", "abc123"));
    assert!(!status_row_ok(table, "tests/bug_b1.rs", "abc123"));
    assert!(!status_row_ok(&once, "tests/bug_b1.rs", "other"));
}

#[test]
fn the_code_map_covers_the_bug_fix_files() {
    use switchboard_core::sky::codemap::{check_modules, Touched};
    let map = include_str!("../../../docs/code-map.md");
    let touched: Vec<Touched> = [
        ("crates/core/src/bug/fix.rs", true),
        ("crates/core/src/bug/mod.rs", false),
        ("crates/core/tests/bug_fix.rs", true),
        ("docs/code-map.md", false),
    ]
    .into_iter()
    .map(|(path, added)| Touched {
        path: path.into(),
        added,
    })
    .collect();
    let findings = check_modules("bug-fix", &touched, map, map);
    assert!(findings.is_empty(), "{findings:?}");
}
