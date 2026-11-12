//! The one error type faces see. SHELL-3 shows it in place: the command that failed and the
//! first lines of its output.

use crate::fleet::FleetError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// CORE-2: a fleet command failed or printed JSON the core can't parse. Never a guessed value.
    Fleet(FleetError),
    /// CORE-5: a settings path the action needs is not set. Holds the setting's name.
    MissingPath(&'static str),
    /// CORE-3: a guarded action was refused. Holds the refusal text, the same for every face.
    Refused(String),
    /// A GitHub read or write failed (`github`).
    GitHub(String),
    /// A file read or write failed. Holds the path and the OS error text.
    Io(String),
    /// A file the core reads (settings, snapshot, thread, ROADMAP block) did not parse.
    Parse(String),
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreError::Fleet(e) => write!(f, "fleet error: {e}"),
            CoreError::MissingPath(name) => write!(f, "setting not set: {name}"),
            CoreError::Refused(why) => write!(f, "refused: {why}"),
            CoreError::GitHub(why) => write!(f, "GitHub: {why}"),
            CoreError::Io(why) => write!(f, "file: {why}"),
            CoreError::Parse(why) => write!(f, "parse: {why}"),
        }
    }
}

impl std::error::Error for CoreError {}
