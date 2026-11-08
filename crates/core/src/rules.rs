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
    todo!("CORE-3")
}

/// True when `dir` is inside a job worktree (the fleet's worktrees folder, or a git worktree
/// whose `.git` file points into one).
pub fn is_job_worktree(dir: &std::path::Path) -> bool {
    todo!("CORE-3, MCP-3")
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
