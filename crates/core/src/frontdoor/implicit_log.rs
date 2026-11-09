//! FD-9: implicit chat signals logged in the thread store (FD-13). The shell's `chat_implicit` command is a thin call into `log_implicit`.

use super::sticky::{implicit_row, Implicit};
use super::thread::ThreadStore;
use crate::joblog::JobRow;
use crate::CoreError;
use std::path::Path;

/// The signal whose `Implicit::name` is exactly `name`; `None` for any other text.
pub fn implicit_from_name(name: &str) -> Option<Implicit> {
    [
        Implicit::OpenedInClaude,
        Implicit::ReAsked,
        Implicit::Abandoned,
        Implicit::CopiedAnswer,
    ]
    .into_iter()
    .find(|signal| signal.name() == name)
}

/// FD-9: build the implicit row for the signal named `signal` (`implicit_row`) and append it to
/// the thread `handoff_id` in `data_dir`'s thread store; returns the row. An unknown name or an
/// empty handoff id is `CoreError::Refused` and writes nothing.
pub fn log_implicit(
    data_dir: &Path,
    handoff_id: &str,
    signal: &str,
    service: Option<&str>,
    model: Option<&str>,
    kind: Option<&str>,
) -> Result<JobRow, CoreError> {
    let found = implicit_from_name(signal)
        .ok_or_else(|| CoreError::Refused(format!("unknown implicit signal: {signal}")))?;
    if handoff_id.is_empty() {
        return Err(CoreError::Refused(
            "implicit signal with no handoff id".to_string(),
        ));
    }
    let row = implicit_row(found, service, model, kind, handoff_id);
    ThreadStore::new(data_dir).append(handoff_id, &row)?;
    Ok(row)
}
