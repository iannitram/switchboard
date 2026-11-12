//! unit-proof (#405): the red check. A unit's test must fail on the branch before its code
//! lands. `red_check` judges one finished run of the test command. The caller runs the command
//! and passes its exit code and output; this module runs nothing.

/// What the red check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// A test ran and failed, and the command exited non-zero.
    Red { failed: u64 },
    /// Every test that ran passed; the command's failure is not a failed test.
    Passed,
    /// No test ran: none matched, all were ignored, or the build failed before any ran.
    NoTestRan,
    /// The command did not exit with a code: it could not start, or was killed.
    DidNotRun,
}

impl Verdict {
    pub fn is_red(self) -> bool {
        matches!(self, Verdict::Red { .. })
    }
}

/// Judge one run. `exit_code` is `None` when the command did not exit. `output` is its stdout
/// and stderr; the counts come from its `test result:` lines (cargo test's summary, one per
/// test binary). Output with no such line is never red.
pub fn red_check(exit_code: Option<i32>, output: &str) -> Verdict {
    let Some(code) = exit_code else {
        return Verdict::DidNotRun;
    };
    let (passed, failed) = summary(output);
    if passed + failed == 0 {
        Verdict::NoTestRan
    } else if code == 0 || failed == 0 {
        Verdict::Passed
    } else {
        Verdict::Red { failed }
    }
}

/// `(passed, failed)` summed over every `test result:` line. Ignored tests count as neither.
fn summary(output: &str) -> (u64, u64) {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("test result:"))
        .flat_map(|rest| rest.split(';'))
        .fold((0, 0), |(passed, failed), part| {
            let words: Vec<&str> = part.split_whitespace().collect();
            match words.as_slice() {
                [.., count, "passed"] => (passed + number(count), failed),
                [.., count, "failed"] => (passed, failed + number(count)),
                _ => (passed, failed),
            }
        })
}

fn number(word: &str) -> u64 {
    word.parse().unwrap_or(0)
}
