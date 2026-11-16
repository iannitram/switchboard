//! `unit`: the steps of one roadmap unit. `frontier` chooses it, `claim` locks it, `proof` judges
//! the red check of its test command, `job` writes the fleet job and brief for a claimed unit,
//! `land` opens its pull request and records its resolution.

pub mod claim;
pub mod frontier;
pub mod job;
pub mod land;
pub mod proof;
