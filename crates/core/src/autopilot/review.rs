//! AUTO-3: the one R25 review of a PR this machine's satellite opened, as a state machine over
//! R25's ladder. Sizes climb Easy -> Medium -> Hard -> Opus -> Fable; each fixer gets up to 3
//! tries; Fable only after Opus has failed its 3.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rung {
    Easy,
    Medium,
    Hard,
    Opus,
    Fable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewState {
    /// Review job started at this rung, try n (1-based).
    Reviewing { rung: Rung, try_n: u8 },
    /// A fix bigger than the small-fix limit, waiting for the next size up to check it.
    CheckingFix { rung: Rung, try_n: u8 },
    /// `VERDICT: PASS` posted.
    Passed,
    /// FAIL without a fix (a design question or a decision of mine): Waiting on you.
    NeedsMe,
    /// The ladder reached Fable: Waiting on you.
    AtFable,
}

/// What one review attempt came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No gap found.
    Pass,
    /// The reviewer fixed a gap on the PR branch: changed lines, and whether the fix touched
    /// only docs, the code map, comments or tests.
    Fixed { loc: u32, docs_or_tests_only: bool },
    /// A gap it could not fix in scope.
    Failed,
    /// A FAIL that needs my decision.
    NeedsMe,
}

/// A fix of this many changed lines or fewer passes without a check.
pub const SMALL_FIX_LOC: u32 = 30;

/// AUTO-3: the next state.
pub fn step(state: &ReviewState, outcome: &Outcome) -> ReviewState {
    let (rung, try_n, reviewing) = match state {
        ReviewState::Reviewing { rung, try_n } => (*rung, *try_n, true),
        ReviewState::CheckingFix { rung, try_n } => (*rung, *try_n, false),
        ReviewState::Passed | ReviewState::NeedsMe | ReviewState::AtFable => {
            return state.clone();
        }
    };
    match outcome {
        Outcome::Pass => ReviewState::Passed,
        Outcome::NeedsMe => ReviewState::NeedsMe,
        Outcome::Fixed {
            loc,
            docs_or_tests_only,
        } if reviewing => {
            if *loc <= SMALL_FIX_LOC || *docs_or_tests_only {
                ReviewState::Passed
            } else {
                ReviewState::CheckingFix { rung, try_n }
            }
        }
        // A fix that is checked by the next size up fails there, as does a gap left open.
        Outcome::Fixed { .. } | Outcome::Failed => failed_try(rung, try_n),
    }
}

/// A failed try: retry at the same rung, or climb to the next rung after try 3.
fn failed_try(rung: Rung, try_n: u8) -> ReviewState {
    match (rung, try_n) {
        (Rung::Fable, _) => ReviewState::AtFable,
        (_, n) if n < 3 => ReviewState::Reviewing { rung, try_n: n + 1 },
        (Rung::Opus, _) => ReviewState::AtFable,
        (Rung::Easy, _) => ReviewState::Reviewing {
            rung: Rung::Medium,
            try_n: 1,
        },
        (Rung::Medium, _) => ReviewState::Reviewing {
            rung: Rung::Hard,
            try_n: 1,
        },
        (Rung::Hard, _) => ReviewState::Reviewing {
            rung: Rung::Opus,
            try_n: 1,
        },
    }
}

/// AUTO-3: the first rung for a PR built at `size` (`easy`, `medium`, `hard`).
pub fn first_rung(size: &str) -> Rung {
    match size.to_lowercase().as_str() {
        "easy" => Rung::Easy,
        "medium" => Rung::Medium,
        _ => Rung::Hard,
    }
}

/// The lane/size word a rung runs at: easy, medium, hard, opus, fable.
pub fn rung_label(rung: Rung) -> &'static str {
    match rung {
        Rung::Easy => "easy",
        Rung::Medium => "medium",
        Rung::Hard => "hard",
        Rung::Opus => "opus",
        Rung::Fable => "fable",
    }
}

/// AUTO-3: the fleet job that runs one review try of `pr` at `rung`. The reviewer runs on a
/// service other than the builder's (R25), and its brief is the PR diff and the issue. The
/// caller starts it with `step`.
pub fn review_job(
    repo: &str,
    pr: u64,
    issue: u64,
    rung: Rung,
    try_n: u8,
    builder_service: &str,
) -> serde_json::Value {
    serde_json::json!({
        "kind": "review",
        "repo": repo,
        "pr": pr,
        "issue": issue,
        "rung": rung_label(rung),
        "try": try_n,
        "not_service": builder_service,
        "brief": ["diff", "issue"],
    })
}
