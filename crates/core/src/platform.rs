//! The Mac, Windows and Linux seams, each in one place (SPEC.md, Platform plan; R04).
//! 1.0 ships Linux only; the `platforms` feature (After 1.0) fills in Mac and Windows here and
//! nowhere else.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Mac,
    Windows,
}

/// The OS this build runs on.
pub fn current_os() -> Os {
    if cfg!(target_os = "macos") {
        Os::Mac
    } else if cfg!(target_os = "windows") {
        Os::Windows
    } else {
        Os::Linux
    }
}

/// The Python launcher and its leading arguments: `python3 -I` on Linux and Mac,
/// `py -3 -I` on Windows.
pub fn python_launcher(os: Os) -> (&'static str, &'static [&'static str]) {
    match os {
        Os::Linux | Os::Mac => ("python3", &["-I"]),
        Os::Windows => ("py", &["-3", "-I"]),
    }
}

/// Where the code clones live by default: `<home>/dev` on Linux and Mac; on Windows,
/// `D:\dev` (the code drive). Settings (CORE-5) may override it.
pub fn default_dev_root(os: Os, home: &Path) -> PathBuf {
    match os {
        Os::Linux | Os::Mac => home.join("dev"),
        Os::Windows => PathBuf::from("D:\\dev"),
    }
}

/// The NextCloud folder by default: `<home>/Nextcloud` on every OS until U7 is answered.
pub fn default_nextcloud_dir(os: Os, home: &Path) -> PathBuf {
    match os {
        Os::Linux | Os::Mac | Os::Windows => home.join("Nextcloud"),
    }
}

/// One loadout probe (IT-1): a name and the command that answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub name: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
}

/// The fixed probe list for an OS (IT-1). Linux is filled in 1.0; Mac and Windows return
/// an empty list until the `platforms` feature.
pub fn loadout_probes(os: Os) -> &'static [Probe] {
    match os {
        Os::Linux => todo!("IT-1: probe list"),
        Os::Mac => &[],
        Os::Windows => &[],
    }
}

/// Where tokens live (GitHub, service keys): the OS keychain. On Linux in 1.0 it falls back to
/// `gh auth token` for GitHub; a stored credential needs my yes first (R34, U7).
pub trait TokenStore: Send + Sync {
    fn get(&self, service: &str) -> Option<String>;
}
