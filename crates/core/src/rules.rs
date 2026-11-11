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
    /// Not from here: becomes a Waiting on you item with this text (repo archive and restore
    /// over a live repo asked from the CLI or MCP).
    ToWaiting(String),
    /// Refused, with the refusal text every face shows.
    Refuse(String),
}

/// The guard. Merge eligibility (PASS on head, CI green) is GH-5's and is checked after
/// `Allow`; this decides who may ask.
///
/// Inside a job worktree everything is refused. A merge is mine from the app and the
/// project-manager role's from the CLI or MCP. Archiving a repo (the `delete` request; nothing
/// is ever deleted) and restoring over a live one need my confirm in the app; from the CLI or MCP they become a Waiting on you item.
pub fn guard(caller: &Caller, action: &GuardedAction) -> Verdict {
    let what = match action {
        GuardedAction::Merge { repo, pr } => format!("merge {repo}#{pr}"),
        // Switchboard never deletes a repo: the request archives it (#347).
        GuardedAction::DeleteRepo { repo } => format!("archive {repo}"),
        GuardedAction::RestoreOverLive { repo } => format!("restore over {repo}"),
    };
    if caller.in_job_worktree {
        return Verdict::Refuse(format!("{what}: refused inside a job worktree"));
    }
    match action {
        GuardedAction::Merge { .. } => match (caller.face, &caller.role) {
            (Face::Gui, _) | (Face::Cli | Face::Mcp, Some(Role::ProjectManager)) => Verdict::Allow,
            _ => Verdict::Refuse(format!("{what}: only the project-manager role may merge")),
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

/// MCP-2, MCP-3, CLI-3: who may edit a budget pool (BUD-2: the cap, plan, reset or cost). The
/// app may; the CLI and MCP only with the budget-manager role (MCP-2 gives it the pool cap);
/// nothing inside a job worktree. A pool edit is not one of CORE-3's guarded actions, so it
/// never becomes a Waiting on you item here; the saved edit adds its own (U5).
pub fn guard_pool_edit(caller: &Caller, pool: &str) -> Verdict {
    let what = format!("edit pool {pool}");
    if caller.in_job_worktree {
        return Verdict::Refuse(format!("{what}: refused inside a job worktree"));
    }
    match (caller.face, &caller.role) {
        (Face::Gui, _) | (Face::Cli | Face::Mcp, Some(Role::BudgetManager)) => Verdict::Allow,
        _ => Verdict::Refuse(format!(
            "{what}: only the budget-manager role may edit a pool"
        )),
    }
}

/// True when `dir` is inside a job worktree (the fleet's worktrees folder, or a git worktree
/// whose `.git` file points into one).
///
/// True when some ancestor of `dir` (itself included) is named `fleet-*` under a folder whose
/// name ends in `-wt`, or when the first `.git` entry found walking up is a file whose
/// `gitdir:` path ends in a component that starts with `fleet-` or contains `-fleet-`. A
/// `.git` folder (a plain clone) is not a job worktree. It only reads the disk.
pub fn is_job_worktree(dir: &std::path::Path) -> bool {
    let named = dir.ancestors().any(|a| {
        let name = a.file_name().and_then(|n| n.to_str());
        let parent = a
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str());
        matches!((name, parent), (Some(n), Some(p)) if n.starts_with("fleet-") && p.ends_with("-wt"))
    });
    if named {
        return true;
    }
    let Some(git) = dir.ancestors().map(|a| a.join(".git")).find(|g| g.exists()) else {
        return false;
    };
    if !git.is_file() {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(&git) else {
        return false;
    };
    text.lines()
        .find_map(|l| l.trim().strip_prefix("gitdir:"))
        .map(|path| {
            let last = path
                .trim()
                .trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or("");
            last.starts_with("fleet-") || last.contains("-fleet-")
        })
        .unwrap_or(false)
}

impl Role {
    /// The role from its `--role` flag: `project-manager`, `budget-manager` or `satellite`,
    /// exact match only.
    pub fn parse(flag: &str) -> Option<Role> {
        match flag {
            "project-manager" => Some(Role::ProjectManager),
            "budget-manager" => Some(Role::BudgetManager),
            "satellite" => Some(Role::Satellite),
            _ => None,
        }
    }

    /// The `--role` flag for this role; `Role::parse` reads it back.
    pub fn flag(&self) -> &'static str {
        match self {
            Role::ProjectManager => "project-manager",
            Role::BudgetManager => "budget-manager",
            Role::Satellite => "satellite",
        }
    }
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
