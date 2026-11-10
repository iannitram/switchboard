//! CORE-5: the app's paths, kept in one settings file. A missing path is named, never guessed.

use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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
    /// The names of the paths not set, in `SETTING_NAMES` order. An empty path counts as not set.
    pub fn missing(&self) -> Vec<&'static str> {
        let unset = |p: &Option<PathBuf>| p.as_deref().map_or(true, |p| p.as_os_str().is_empty());
        let values = [
            unset(&self.pipeline_repo),
            unset(&self.nextcloud_backups),
            unset(&self.vault_agents),
        ];
        SETTING_NAMES
            .iter()
            .zip(values.iter())
            .filter(|(_, &unset)| unset)
            .map(|(name, _)| *name)
            .collect()
    }

    /// The pipeline repo, or `CoreError::MissingPath("pipeline_repo")`.
    pub fn pipeline_repo(&self) -> Result<&Path, CoreError> {
        match self.pipeline_repo.as_deref() {
            Some(p) if !p.as_os_str().is_empty() => Ok(p),
            _ => Err(CoreError::MissingPath("pipeline_repo")),
        }
    }
}

/// Read the settings file. A file that does not exist is `Settings::default()`; one that does
/// not parse is `CoreError::Parse`, naming the file. Keys that are not known are ignored.
pub fn load(path: &Path) -> Result<Settings, CoreError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
    };
    serde_json::from_str(&text).map_err(|e| CoreError::Parse(format!("{}: {e}", path.display())))
}

/// Each save's own number, so two saves at once never share a temp file (#371).
static SAVE_SEQ: AtomicU64 = AtomicU64::new(0);

/// The temp file one save writes before its rename: `<name>.<pid>-<n>.tmp` beside the settings
/// file, where `n` counts this process's saves. No two saves, in one process or two, share it.
pub fn temp_path(path: &Path) -> PathBuf {
    let n = SAVE_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut file_name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    file_name.push(format!(".{}-{n}.tmp", std::process::id()));
    path.with_file_name(file_name)
}

/// Write the settings file as pretty JSON, creating its folder. The write goes to a temp file of
/// its own (`temp_path`) first and is renamed over the settings file, so a failure leaves the old
/// file in place and two saves at once both succeed (the last rename wins).
pub fn save(path: &Path, settings: &Settings) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CoreError::Io(format!("{}: {e}", parent.display())))?;
    }
    let text =
        serde_json::to_string_pretty(settings).map_err(|e| CoreError::Parse(format!("{e}")))?;
    let tmp = temp_path(path);
    std::fs::write(&tmp, text).map_err(|e| CoreError::Io(format!("{}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        CoreError::Io(format!("{}: {e}", path.display()))
    })?;
    Ok(())
}
