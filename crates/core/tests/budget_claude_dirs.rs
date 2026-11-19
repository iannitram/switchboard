//! BUD-4: where Claude Code keeps its session logs. The environment values are passed in, so
//! no test sets a real variable. The cases follow ccusage's tests for the same rules
//! (ccusage, MIT, copyright (c) 2025 ryoppippi; see NOTICE).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use switchboard_core::budget::{claude_config_dirs, claude_projects_dirs};

fn p(text: &str) -> PathBuf {
    PathBuf::from(text)
}

#[test]
fn env_entries_are_trimmed_expanded_and_deduplicated() {
    let dirs = claude_config_dirs(
        Some(" ~/a , ~ ,, /abs, ~/a"),
        Some(OsStr::new("/ignored")),
        Some(Path::new("/home/u")),
    );
    assert_eq!(dirs, [p("/home/u/a"), p("/home/u"), p("/abs")]);
}

#[test]
fn tilde_lookalikes_stay_as_written() {
    let dirs = claude_config_dirs(Some("~user/c,foo~/c"), None, Some(Path::new("/home/u")));
    assert_eq!(dirs, [p("~user/c"), p("foo~/c")]);
}

#[test]
fn default_is_xdg_then_dot_claude() {
    let dirs = claude_config_dirs(None, Some(OsStr::new("/x")), Some(Path::new("/home/u")));
    assert_eq!(dirs, [p("/x/claude"), p("/home/u/.claude")]);
}

#[test]
fn unset_or_empty_xdg_means_dot_config() {
    for xdg in [None, Some(OsStr::new(""))] {
        let dirs = claude_config_dirs(None, xdg, Some(Path::new("/home/u")));
        assert_eq!(dirs, [p("/home/u/.config/claude"), p("/home/u/.claude")]);
    }
}

#[test]
fn no_home_and_no_variable_is_empty() {
    assert!(claude_config_dirs(None, Some(OsStr::new("/x")), None).is_empty());
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-bud4-dirs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn an_entry_naming_projects_is_read_as_its_parent_and_only_dirs_with_projects_are_kept() {
    let root = temp("projects");
    std::fs::create_dir_all(root.join("a/projects")).unwrap();
    std::fs::create_dir_all(root.join("b")).unwrap();
    let value = format!(
        "{},{}",
        root.join("a/projects").display(),
        root.join("b").display()
    );
    let dirs = claude_projects_dirs(Some(&value), None, None);
    assert_eq!(dirs, [root.join("a").join("projects")]);
    let _ = std::fs::remove_dir_all(&root);
}
