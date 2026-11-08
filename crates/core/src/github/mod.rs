//! `github`: GitHub reads and writes. 1.0 reads GitHub in code (GH-1); no agent scans.
//! Reads are GraphQL, with a REST conditional request and the stored ETag to skip an
//! unchanged repo (GH-2). Creates go through the pipeline's create queue (GH-4); merges only
//! through `merge_pr.py` (GH-5).

pub mod merge;
pub mod poll;
pub mod queue;
pub mod snapshot;
pub mod transport;

pub use snapshot::{Issue, Milestone, PullRequest, RepoSnapshot, ReviewVerdict};
