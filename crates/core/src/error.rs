//! The one error type faces see. SHELL-3 shows it in place: the command that failed and the
//! first lines of its output.

use crate::fleet::FleetError;
use serde::Serialize;

/// Stable machine-readable names for failures returned by the faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorCode {
    Usage,
    SettingMissing,
    Refused,
    FleetFailed,
    FleetBadJson,
    FleetSpawn,
    FleetTimeout,
    #[serde(rename = "github")]
    GitHub,
    File,
    Parse,
    NotFound,
    InJobWorktree,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::SettingMissing => "setting_missing",
            Self::Refused => "refused",
            Self::FleetFailed => "fleet_failed",
            Self::FleetBadJson => "fleet_bad_json",
            Self::FleetSpawn => "fleet_spawn",
            Self::FleetTimeout => "fleet_timeout",
            Self::GitHub => "github",
            Self::File => "file",
            Self::Parse => "parse",
            Self::NotFound => "not_found",
            Self::InJobWorktree => "in_job_worktree",
        }
    }

    pub fn retryable(self) -> bool {
        matches!(self, Self::FleetSpawn | Self::FleetTimeout | Self::GitHub)
    }

    fn fix(self) -> &'static str {
        match self {
            Self::Usage => "Check the command syntax and try again.",
            Self::SettingMissing => "Set the missing value in Settings.",
            Self::Refused => "Review the requested action and its requirements.",
            Self::FleetFailed => "Check the fleet command output and try again.",
            Self::FleetBadJson => "Check the fleet tool output and try again.",
            Self::FleetSpawn => "Check that the fleet tool is installed and try again.",
            Self::FleetTimeout => "Check whether the fleet tool is still running, then retry.",
            Self::GitHub => "Check GitHub connectivity and try again.",
            Self::File => "Check that the file exists and is accessible.",
            Self::Parse => "Check the file contents and correct its format.",
            Self::NotFound => "Check the name and try again.",
            Self::InJobWorktree => "Run this command outside the job worktree.",
        }
    }
}

/// A stable error envelope shared by the CLI and MCP faces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub fix: String,
    pub reason: Option<String>,
}

impl ErrorBody {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: code.retryable(),
            fix: code.fix().to_string(),
            reason: None,
        }
    }
}

impl From<String> for ErrorBody {
    fn from(message: String) -> Self {
        Self::new(ErrorCode::Refused, message)
    }
}

impl From<&str> for ErrorBody {
    fn from(message: &str) -> Self {
        Self::new(ErrorCode::Refused, message)
    }
}

impl From<&CoreError> for ErrorBody {
    fn from(error: &CoreError) -> Self {
        match error {
            CoreError::Fleet(fleet) => {
                use crate::fleet::FleetErrorReason;
                let (code, reason) = match &fleet.reason {
                    FleetErrorReason::Exit => (
                        ErrorCode::FleetFailed,
                        fleet.exit_code.map(|n| format!("exit_code={n}")),
                    ),
                    FleetErrorReason::BadJson(why) => (ErrorCode::FleetBadJson, Some(why.clone())),
                    FleetErrorReason::Spawn(why) => (ErrorCode::FleetSpawn, Some(why.clone())),
                    FleetErrorReason::Timeout(seconds) => (
                        ErrorCode::FleetTimeout,
                        Some(format!("timeout_seconds={seconds}")),
                    ),
                };
                let mut body = Self::new(code, error.to_string());
                body.reason = reason;
                body
            }
            CoreError::MissingPath(name) => {
                let mut body = Self::new(ErrorCode::SettingMissing, error.to_string());
                body.reason = Some((*name).to_string());
                body
            }
            CoreError::Refused(_) => Self::new(ErrorCode::Refused, error.to_string()),
            CoreError::GitHub(_) => Self::new(ErrorCode::GitHub, error.to_string()),
            CoreError::Io(_) => Self::new(ErrorCode::File, error.to_string()),
            CoreError::Parse(_) => Self::new(ErrorCode::Parse, error.to_string()),
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::{CoreError, ErrorBody, ErrorCode};
    use crate::fleet::{FleetError, FleetErrorReason};

    #[test]
    fn serialized_error_code_names_are_a_contract() {
        let codes = [
            ErrorCode::Usage,
            ErrorCode::SettingMissing,
            ErrorCode::Refused,
            ErrorCode::FleetFailed,
            ErrorCode::FleetBadJson,
            ErrorCode::FleetSpawn,
            ErrorCode::FleetTimeout,
            ErrorCode::GitHub,
            ErrorCode::File,
            ErrorCode::Parse,
            ErrorCode::NotFound,
            ErrorCode::InJobWorktree,
        ];
        let names: Vec<String> = codes
            .iter()
            .map(|code| {
                serde_json::to_value(code)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        // Catches: changing or removing any serialized ErrorCode name.
        assert_eq!(
            names,
            [
                "usage",
                "setting_missing",
                "refused",
                "fleet_failed",
                "fleet_bad_json",
                "fleet_spawn",
                "fleet_timeout",
                "github",
                "file",
                "parse",
                "not_found",
                "in_job_worktree"
            ]
        );
    }

    #[test]
    fn every_core_error_and_fleet_reason_maps_to_a_useful_body() {
        let fleet = |reason| {
            CoreError::Fleet(FleetError {
                command: vec!["fleet".into()],
                exit_code: Some(7),
                stderr: String::new(),
                reason,
                undo_note: None,
            })
        };
        let cases = [
            (
                CoreError::MissingPath("data"),
                ErrorCode::SettingMissing,
                false,
            ),
            (CoreError::Refused("no".into()), ErrorCode::Refused, false),
            (CoreError::GitHub("offline".into()), ErrorCode::GitHub, true),
            (CoreError::Io("read failed".into()), ErrorCode::File, false),
            (CoreError::Parse("bad file".into()), ErrorCode::Parse, false),
            (fleet(FleetErrorReason::Exit), ErrorCode::FleetFailed, false),
            (
                fleet(FleetErrorReason::BadJson("bad json".into())),
                ErrorCode::FleetBadJson,
                false,
            ),
            (
                fleet(FleetErrorReason::Spawn("missing".into())),
                ErrorCode::FleetSpawn,
                true,
            ),
            (
                fleet(FleetErrorReason::Timeout(30)),
                ErrorCode::FleetTimeout,
                true,
            ),
        ];
        for (error, code, retryable) in cases {
            let body = ErrorBody::from(&error);
            // Catches: changing a CoreError or FleetErrorReason classification or fix text.
            assert_eq!(body.code, code);
            assert_eq!(body.retryable, retryable);
            assert!(!body.fix.is_empty());
        }
    }
}
