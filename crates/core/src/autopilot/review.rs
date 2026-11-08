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
    todo!("AUTO-3")
}

/// AUTO-3: the first rung for a PR built at `size` (`easy`, `medium`, `hard`).
pub fn first_rung(size: &str) -> Rung {
    todo!("AUTO-3")
}
