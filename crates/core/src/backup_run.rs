//! BAK-1, BAK-2: the back-up button's core. The Sky's "back up" and "back up all" call
//! `run_one` / `run_all` through the `backup_repo` and `backup_all` commands; a refused backup
//! becomes its Waiting on you item, and each outcome is one `BackupRow` for the face.

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

/// BAK-1: the folder one repo's backup lands in: `<backups>/github/<owner>/<name>`.
pub fn folder(backups: &Path, repo: &str) -> PathBuf {
    let mut dir = backups.join("github");
    for part in repo.split('/') {
        dir = dir.join(part);
    }
    dir
}

/// "Back up all" covers every repo in the snapshot store, by name, in the store's order.
pub fn repos(data_dir: &Path) -> Result<Vec<String>, CoreError> {
    let snaps = SnapshotStore::new(data_dir).load_all()?;
    if snaps.is_empty() {
        return Err(CoreError::GitHub("no snapshot yet".to_string()));
    }
    Ok(snaps.into_iter().map(|s| s.repo).collect())
}

/// Records one outcome: a written backup clears that repo's refused item; a refused one
/// replaces it. Answers the row for the face.
pub fn record(
    data_dir: &Path,
    backups: &Path,
    repo: &str,
    outcome: &BackupOutcome,
    now: UnixSeconds,
) -> Result<BackupRow, CoreError> {
    let dir = folder(backups, repo);
    let local = LocalItems::new(data_dir);
    let (outcome, manifest, reason) = match outcome {
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
        outcome: outcome.to_string(),
        folder: dir.display().to_string(),
        manifest,
        reason,
    })
}

/// The backups folder, or the missing-setting error when it is unset or empty.
fn backups_folder(settings: &Settings) -> Result<&Path, CoreError> {
    match settings.nextcloud_backups.as_deref() {
        Some(p) if !p.as_os_str().is_empty() => Ok(p),
        _ => Err(CoreError::MissingPath("nextcloud_backups")),
    }
}

/// BAK-1, BAK-2: back up one repo (`owner/name`) and record its outcome.
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

/// BAK-1, BAK-2: back up every repo in the snapshot store and record each outcome, in the
/// order `backup_all` returned them.
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
