//! GH-1, GH-3: the app's poll. Which repos to read (the priority copy's order, then the rest of
//! the snapshot store by name), and the poll over the snapshot store in the app data folder.

use super::poll::Poller;
use super::snapshot::SnapshotStore;
use super::transport::GitHubTransport;
use crate::clock::Clock;
use crate::sky::model::PRIORITY_FILE;
use crate::sky::priority;
use crate::CoreError;
use std::path::Path;

/// GH-1: the Tauri event the poll thread sends after each tick, with the repos it read.
pub const SNAPSHOT_EVENT: &str = "snapshot";

/// GH-1: the poll thread wakes this often; a tick reads only the repos whose snapshot is due.
pub const TICK_EVERY_SECS: u64 = 60;

/// GH-1: the repos to read: the priority copy's order (each once), then every other repo in
/// the snapshot store, by name. No copy and no snapshot is no repos.
pub fn repos(data_dir: &Path) -> Result<Vec<String>, CoreError> {
    let mut out: Vec<String> = Vec::new();
    let path = data_dir.join(PRIORITY_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            for repo in priority::parse(&text)?.order {
                if !out.contains(&repo) {
                    out.push(repo);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
    }
    for snapshot in SnapshotStore::new(data_dir).load_all()? {
        if !out.contains(&snapshot.repo) {
            out.push(snapshot.repo);
        }
    }
    Ok(out)
}

/// GH-1: one tick of the poll: read the repos whose snapshot is 10 minutes old or missing.
/// Answers the repos read.
pub fn tick(
    transport: &dyn GitHubTransport,
    data_dir: &Path,
    clock: &dyn Clock,
) -> Result<Vec<String>, CoreError> {
    let store = SnapshotStore::new(data_dir);
    Poller {
        transport,
        store: &store,
        clock,
        repos: repos(data_dir)?,
    }
    .tick()
}

/// GH-1: the Refresh button: read every repo now, whatever its age. Answers the repos read.
pub fn refresh_all(
    transport: &dyn GitHubTransport,
    data_dir: &Path,
    clock: &dyn Clock,
) -> Result<Vec<String>, CoreError> {
    let store = SnapshotStore::new(data_dir);
    Poller {
        transport,
        store: &store,
        clock,
        repos: repos(data_dir)?,
    }
    .refresh_all()
}

/// GH-3: refresh-on-action: a write to `repo` reads that repo once, listed or not.
pub fn after_write(
    transport: &dyn GitHubTransport,
    data_dir: &Path,
    clock: &dyn Clock,
    repo: &str,
) -> Result<(), CoreError> {
    let store = SnapshotStore::new(data_dir);
    Poller {
        transport,
        store: &store,
        clock,
        repos: vec![repo.to_string()],
    }
    .after_write(repo)
}
