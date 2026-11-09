//! BAK-W: back up one repo or every repo from the app (the Sky's buttons, through the
//! `backup_repo` and `backup_all` commands) and from the CLI. It runs `backup::backup_repo` /
//! `backup::backup_all`, turns a refused backup into its Waiting on you item (BAK-2), and
//! answers one `BackupRow` per repo.

use crate::backup::{self, BackupOutcome, Manifest};
use crate::clock::UnixSeconds;
use crate::github::snapshot::SnapshotStore;
use crate::settings::Settings;
use crate::waiting::{refused_backup_item, LocalItems, Source};
use crate::CoreError;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// One backup's outcome as the face reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupRow {
    pub repo: String,               // owner/name
    pub outcome: String,            // "written" | "refused"
    pub folder: String,             // folder(backups, repo).display().to_string()
    pub manifest: Option<Manifest>, // Some when written
    pub reason: Option<String>,     // Some when refused
}

/// BAK-1: `<NextCloud backups>/github/<owner>/<name>`, one `join` per part.
pub fn folder(backups: &Path, repo: &str) -> PathBuf {
    repo.split('/')
        .fold(backups.join("github"), |dir, part| dir.join(part))
}

/// "Back up all" covers every repo in the snapshot store (GH-1), by name.
pub fn repos(data_dir: &Path) -> Result<Vec<String>, CoreError> {
    let names: Vec<String> = SnapshotStore::new(data_dir)
        .load_all()?
        .into_iter()
        .map(|snap| snap.repo)
        .collect();
    if names.is_empty() {
        return Err(CoreError::GitHub("no snapshot yet".to_string()));
    }
    Ok(names)
}

/// The row for one outcome. A written backup clears that repo's refused item (WAIT-3); a
/// refused one adds or replaces it (BAK-2).
pub fn record(
    data_dir: &Path,
    backups: &Path,
    repo: &str,
    outcome: &BackupOutcome,
    now: UnixSeconds,
) -> Result<BackupRow, CoreError> {
    let dir = folder(backups, repo);
    let local = LocalItems::new(data_dir);
    let (name, manifest, reason) = match outcome {
        BackupOutcome::Written(manifest) => {
            local.remove(&Source::RefusedBackup {
                repo: repo.to_string(),
            })?;
            ("written", Some(manifest.clone()), None)
        }
        BackupOutcome::Refused(reason) => {
            local.add(refused_backup_item(repo, reason, &dir, now))?;
            ("refused", None, Some(reason.clone()))
        }
    };
    Ok(BackupRow {
        repo: repo.to_string(),
        outcome: name.to_string(),
        folder: dir.display().to_string(),
        manifest,
        reason,
    })
}

/// The NextCloud backups folder, or the setting's error when it is unset or empty.
fn backups_folder(settings: &Settings) -> Result<&Path, CoreError> {
    match settings.nextcloud_backups.as_deref() {
        Some(path) if !path.as_os_str().is_empty() => Ok(path),
        _ => Err(CoreError::MissingPath("nextcloud_backups")),
    }
}

/// BAK-1, BAK-2: back up one repo and record its row.
pub fn run_one(
    settings: &Settings,
    data_dir: &Path,
    repo: &str,
    now: UnixSeconds,
) -> Result<BackupRow, CoreError> {
    let backups = backups_folder(settings)?;
    let outcome = backup::backup_repo(settings, repo)?;
    record(data_dir, backups, repo, &outcome, now)
}

/// BAK-1, BAK-2: back up every repo in the snapshot store and record one row per repo.
pub fn run_all(
    settings: &Settings,
    data_dir: &Path,
    now: UnixSeconds,
) -> Result<Vec<BackupRow>, CoreError> {
    let backups = backups_folder(settings)?;
    let names = repos(data_dir)?;
    backup::backup_all(settings, &names)?
        .iter()
        .map(|(repo, outcome)| record(data_dir, backups, repo, outcome, now))
        .collect()
}
