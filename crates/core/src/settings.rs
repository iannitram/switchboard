//! CORE-5: the app's paths, kept in one settings file. A missing path is named, never guessed.

use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// The pipeline repo clone (the fleet tools live under `tools/fleet/`).
    pub pipeline_repo: Option<PathBuf>,
    /// The NextCloud backups folder (BAK-1 writes `github/<repo>/` under it).
    pub nextcloud_backups: Option<PathBuf>,
    /// The vault agents folder (trigger words, U13).
    pub vault_agents: Option<PathBuf>,
}

/// The settings names, in the order `missing` reports them.
pub const SETTING_NAMES: [&str; 3] = ["pipeline_repo", "nextcloud_backups", "vault_agents"];

impl Settings {
    /// The names of the paths not set, in `SETTING_NAMES` order.
    pub fn missing(&self) -> Vec<&'static str> {
        todo!("CORE-5")
    }

    /// The pipeline repo, or `CoreError::MissingPath("pipeline_repo")`.
    pub fn pipeline_repo(&self) -> Result<&Path, CoreError> {
        todo!("CORE-5")
    }
}

/// Read the settings file. A file that does not exist is `Settings::default()`; one that does
/// not parse is `CoreError::Parse`.
pub fn load(path: &Path) -> Result<Settings, CoreError> {
    todo!("CORE-5")
}

/// Write the settings file as pretty JSON, creating its folder.
pub fn save(path: &Path, settings: &Settings) -> Result<(), CoreError> {
    todo!("CORE-5")
}
