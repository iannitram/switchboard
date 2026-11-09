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

/// The Linux probe list (IT-1): the four CLIs (signed in when the auth probe exits 0),
/// llama-swap's running models, the three toolchains (present when `--version` exits 0),
/// `fleet status` (busy) and `fleet errors` (recent errors).
const LINUX_PROBES: &[Probe] = &[
    Probe {
        name: "claude",
        program: "claude",
        args: &["auth", "status"],
    },
    Probe {
        name: "codex",
        program: "codex",
        args: &["login", "status"],
    },
    Probe {
        name: "gemini",
        program: "gemini",
        args: &["auth", "status"],
    },
    Probe {
        name: "gh",
        program: "gh",
        args: &["auth", "status"],
    },
    Probe {
        name: "llama-swap",
        program: "curl",
        args: &["-fsS", "--max-time", "5", "http://127.0.0.1:1234/running"],
    },
    Probe {
        name: "cargo",
        program: "cargo",
        args: &["--version"],
    },
    Probe {
        name: "node",
        program: "node",
        args: &["--version"],
    },
    Probe {
        name: "python3",
        program: "python3",
        args: &["--version"],
    },
    Probe {
        name: "fleet status",
        program: "fleet",
        args: &["status"],
    },
    Probe {
        name: "fleet errors",
        program: "fleet",
        args: &["errors"],
    },
];

/// The fixed probe list for an OS (IT-1). Linux is filled in 1.0; Mac and Windows return
/// an empty list until the `platforms` feature.
pub fn loadout_probes(os: Os) -> &'static [Probe] {
    match os {
        Os::Linux => LINUX_PROBES,
        Os::Mac => &[],
        Os::Windows => &[],
    }
}

/// Where tokens live (GitHub, service keys): the OS keychain. On Linux in 1.0 it falls back to
/// `gh auth token` for GitHub; a stored credential needs my yes first (R34, U7).
pub trait TokenStore: Send + Sync {
    fn get(&self, service: &str) -> Option<String>;
}

/// The Linux 1.0 token store: GitHub's token from `gh auth token`, so `gh auth login` is the
/// only sign-in. Other services have no token here.
pub struct GhAuthToken {
    pub program: String,
}

impl Default for GhAuthToken {
    fn default() -> Self {
        Self {
            program: "gh".to_string(),
        }
    }
}

impl TokenStore for GhAuthToken {
    fn get(&self, service: &str) -> Option<String> {
        if service != "github" {
            return None;
        }
        let out = std::process::Command::new(&self.program)
            .args(["auth", "token"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if token.is_empty() {
            None
        } else {
            Some(token)
        }
    }
}
