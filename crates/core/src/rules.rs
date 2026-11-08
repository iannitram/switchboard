//! CORE-3: the one guard every write passes through. It takes the face and the role and
//! answers the same way, with the same refusal text, whichever face asked.

use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Gui,
    Cli,
    Mcp,
}

/// The role flag (`--role <role>`) a CLI or MCP start was given; `None` without one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    ProjectManager,
    BudgetManager,
    Satellite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardedAction {
    /// Merge `repo#pr` (GH-5's rule decides eligibility).
    Merge {
        repo: String,
        pr: u64,
    },
    DeleteRepo {
        repo: String,
    },
    /// Restore a backup over a live repo (BAK-3); restoring to a new repo is not guarded.
    RestoreOverLive {
        repo: String,
    },
}

/// Who is asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub face: Face,
    pub role: Option<Role>,
    /// True when the process runs inside a job worktree (MCP-3, CLI-3).
    pub in_job_worktree: bool,
    /// GUI only: I confirmed this action in the app.
    pub confirmed_in_app: bool,
}

/// What the guard decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Go ahead.
    Allow,
    /// Not from here: becomes a Waiting on you item with this text (repo delete and restore
    /// over a live repo asked from the CLI or MCP).
    ToWaiting(String),
    /// Refused, with the refusal text every face shows.
    Refuse(String),
}

/// The guard. Merge eligibility (PASS on head, CI green) is GH-5's and is checked after
/// `Allow`; this decides who may ask.
pub fn guard(caller: &Caller, action: &GuardedAction) -> Verdict {
    let what = match action {
        GuardedAction::Merge { repo, pr } => format!("merge {repo}#{pr}"),
        GuardedAction::DeleteRepo { repo } => format!("delete {repo}"),
        GuardedAction::RestoreOverLive { repo } => format!("restore over {repo}"),
    };
    if caller.in_job_worktree {
        return Verdict::Refuse(format!("{what}: refused inside a job worktree"));
    }
    match action {
        GuardedAction::Merge { .. } => match caller.face {
            Face::Gui => Verdict::Allow,
            Face::Cli | Face::Mcp if matches!(caller.role, Some(Role::ProjectManager)) => {
                Verdict::Allow
            }
            Face::Cli | Face::Mcp => {
                Verdict::Refuse(format!("{what}: only the project-manager role may merge"))
            }
        },
        GuardedAction::DeleteRepo { .. } | GuardedAction::RestoreOverLive { .. } => {
            match caller.face {
                Face::Gui if caller.confirmed_in_app => Verdict::Allow,
                Face::Gui => Verdict::Refuse(format!("{what}: needs a confirm in the app")),
                Face::Cli | Face::Mcp => {
                    Verdict::ToWaiting(format!("{what}: needs a confirm in the app"))
                }
            }
        }
    }
}

impl Role {
    /// The role for a `--role` flag value; exact match only.
    pub fn parse(flag: &str) -> Option<Role> {
        match flag {
            "project-manager" => Some(Role::ProjectManager),
            "budget-manager" => Some(Role::BudgetManager),
            "satellite" => Some(Role::Satellite),
            _ => None,
        }
    }

    /// The `--role` flag value for this role; `parse` takes it back.
    pub fn flag(&self) -> &'static str {
        match self {
            Role::ProjectManager => "project-manager",
            Role::BudgetManager => "budget-manager",
            Role::Satellite => "satellite",
        }
    }
}

/// True when `dir` is inside a job worktree (the fleet's worktrees folder, or a git worktree
/// whose `.git` file points into one).
pub fn is_job_worktree(dir: &std::path::Path) -> bool {
    // (a) `<clone>-wt/fleet-<id>` and anything under it.
    for anc in dir.ancestors() {
        let is_fleet = anc
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("fleet-"));
        let parent_is_wt = anc
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with("-wt"));
        if is_fleet && parent_is_wt {
            return true;
        }
    }
    // (b) the first folder with a `.git` entry: a `.git` file pointing at a fleet job.
    for anc in dir.ancestors() {
        let git = anc.join(".git");
        if git.is_dir() {
            return false;
        }
        if git.is_file() {
            return gitdir_names_fleet_job(&git);
        }
    }
    false
}

/// True when the `gitdir:` line of a `.git` file ends in a fleet job's folder name.
fn gitdir_names_fleet_job(git_file: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(git_file) else {
        return false;
    };
    let Some(path) = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("gitdir:"))
        .map(str::trim)
    else {
        return false;
    };
    let path = path.trim_end_matches(|c: char| c == '/' || c == '\\');
    let last = path.rsplit(|c: char| c == '/' || c == '\\').next().unwrap_or("");
    last.starts_with("fleet-") || last.contains("-fleet-")
}

impl Verdict {
    /// `Allow` is `Ok`; anything else is `CoreError::Refused` with its text.
    pub fn into_result(self) -> Result<(), CoreError> {
        match self {
            Verdict::Allow => Ok(()),
            Verdict::ToWaiting(t) | Verdict::Refuse(t) => Err(CoreError::Refused(t)),
        }
    }
}
