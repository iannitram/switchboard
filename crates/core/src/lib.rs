//! Switchboard core: the one library every face (the GUI, the `switchboard` CLI and
//! `switchboard mcp`) calls. The core owns every rule; faces hold none of their own.
//!
//! This is the Roadmap skeleton: each module names the Musts it owns (SPEC.md) and carries the
//! signatures its issues fill in. A stub body is `todo!("<Must id>")`. The code map
//! (`docs/code-map.md`) lists each module's status; an issue moves it from planned to present.

// Skeleton only: stubs take arguments they don't read yet. Remove once the last stub is filled.
#![allow(unused_variables, dead_code)]

pub mod abilities;
pub mod autopilot;
pub mod backup;
pub mod backup_run;
pub mod budget;
pub mod clock;
pub mod confirm;
pub mod error;
pub mod fleet;
pub mod frontdoor;
pub mod github;
pub mod it;
pub mod joblog;
pub mod lock;
pub mod platform;
pub mod reports;
pub mod rules;
pub mod settings;
pub mod sky;
pub mod waiting;

pub use error::CoreError;
