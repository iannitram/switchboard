//! The code map's two blocks, read from docs/code-map.md.
//!
//! SKY-6: `reuses:`, drawn as dashed reuse lines. unit-proof (#405): `modules:`, which the code
//! map check reads to tell whether a change's files are in the map and their status current.
//!
//! The formats:
//!
//! ```text
//! reuses:
//! - brocrew: the chat parts, Waiting on you and the fixed report (Apache-2.0)
//!
//! modules:
//! - unit-proof: crates/core/src/unit/proof.rs (present)
//! ```
//!
//! One line per entry, `- <node>: <path> (<status>)` for a module; a module path ending in `/`
//! covers every file under that directory. A block ends at the first line that does not start
//! with `- `.

/// The code map: the one file a change to a module must update.
const CODE_MAP: &str = "docs/code-map.md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reuse {
    /// The repo name it reuses, as written.
    pub repo: String,
    pub what: String,
}

/// One `modules:` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The roadmap node the module belongs to.
    pub node: String,
    /// A file, or a directory ending in `/`.
    pub path: String,
    /// `planned` or `present`; empty when the line has no status.
    pub status: String,
}

impl Module {
    fn covers(&self, file: &str) -> bool {
        if self.path.ends_with('/') {
            file.starts_with(&self.path)
        } else {
            file == self.path
        }
    }
}

/// A file the change touched; `added` when the change creates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touched {
    pub path: String,
    pub added: bool,
}

/// One way a change fails the code map check, anchored to the file it is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// `no_modules_line`, `status_planned` or `code_map_not_updated`.
    pub check: &'static str,
    pub path: String,
    pub message: String,
}

/// The `- ` lines under a block header, up to the first line that does not start with one.
fn block_lines<'a>(code_map_md: &'a str, header: &'static str) -> impl Iterator<Item = &'a str> {
    code_map_md
        .lines()
        .skip_while(move |line| line.trim_end() != header)
        .skip(1)
        .take_while(|line| line.starts_with("- "))
}

/// Parse code-map text; no `reuses:` line gives an empty list.
pub fn reuses(code_map_md: &str) -> Vec<Reuse> {
    block_lines(code_map_md, "reuses:")
        .map(|line| {
            let rest = &line[2..];
            let (repo, what) = match rest.split_once(": ") {
                Some((repo, what)) => (repo.trim(), what.trim()),
                None => (rest.trim(), ""),
            };
            Reuse {
                repo: repo.to_string(),
                what: what.to_string(),
            }
        })
        .collect()
}

/// Parse code-map text; no `modules:` line gives an empty list.
pub fn modules(code_map_md: &str) -> Vec<Module> {
    block_lines(code_map_md, "modules:")
        .map(|line| {
            let rest = &line[2..];
            let (node, tail) = rest.split_once(": ").unwrap_or((rest, ""));
            let tail = tail.trim();
            let (path, status) = match tail.strip_suffix(')').and_then(|t| t.rsplit_once(" (")) {
                Some((path, status)) => (path.trim(), status.trim()),
                None => (tail, ""),
            };
            Module {
                node: node.trim().to_string(),
                path: path.to_string(),
                status: status.to_string(),
            }
        })
        .collect()
}

/// unit-proof (#405): check a change against the `modules:` lines of its node in the head map.
/// A file the change adds needs a line that covers it. A file a line covers needs that line
/// `present` in the head, and the change must touch docs/code-map.md with its `Updated:` date not
/// older than the base's. Files no line covers are not judged. Pure: the caller reads the touched
/// files and both versions of the map.
pub fn check_modules(
    node: &str,
    touched: &[Touched],
    base_md: &str,
    head_md: &str,
) -> Vec<Finding> {
    let lines: Vec<Module> = modules(head_md)
        .into_iter()
        .filter(|module| module.node == node)
        .collect();
    let mut findings = Vec::new();
    let mut first_covered: Option<&str> = None;
    for file in touched {
        let covering: Vec<&Module> = lines.iter().filter(|m| m.covers(&file.path)).collect();
        if covering.is_empty() {
            if file.added {
                findings.push(Finding {
                    check: "no_modules_line",
                    path: file.path.clone(),
                    message: format!(
                        "{} is new and no modules: line of {node} covers it",
                        file.path
                    ),
                });
            }
            continue;
        }
        first_covered = first_covered.or(Some(file.path.as_str()));
        if covering.iter().any(|m| m.status != "present") {
            findings.push(Finding {
                check: "status_planned",
                path: file.path.clone(),
                message: format!("the {node} modules: line for {} is not present", file.path),
            });
        }
    }
    if let Some(path) = first_covered {
        let map_updated =
            touched.iter().any(|file| file.path == CODE_MAP) && date_current(base_md, head_md);
        if !map_updated {
            findings.push(Finding {
                check: "code_map_not_updated",
                path: path.to_string(),
                message: format!(
                    "{CODE_MAP} must change in this change, with its Updated: date not older than the base's"
                ),
            });
        }
    }
    findings
}

/// The map's `Updated:` date. ISO dates order as strings.
fn updated(code_map_md: &str) -> Option<&str> {
    code_map_md
        .lines()
        .find_map(|line| line.strip_prefix("Updated:").map(str::trim))
}

/// The head's date is set and not older than the base's.
fn date_current(base_md: &str, head_md: &str) -> bool {
    match (updated(head_md), updated(base_md)) {
        (Some(head), Some(base)) => head >= base,
        (Some(_), None) => true,
        _ => false,
    }
}

/// SKY-6: a reuse line is a warning when the repo it names is Parked or archived.
pub fn is_warning(archived: bool, stage: Option<&str>) -> bool {
    archived || stage.is_some_and(|stage| stage.trim().eq_ignore_ascii_case("parked"))
}
