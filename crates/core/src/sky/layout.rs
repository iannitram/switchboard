//! SKY-1, SKY-2: where each constellation sits, how big and how bright.

use super::features::FeatureBlock;
use crate::github::Issue;

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub repo: String,
    /// 0-based rank in the priority order.
    pub rank: usize,
    /// Distance from the centre, 0.0 (centre) to 1.0 (rim).
    pub radius: f64,
    /// Drawn size, 0.0 to 1.0; the top 3 are large.
    pub size: f64,
    /// Angle in radians, so constellations don't overlap.
    pub angle: f64,
}

/// SKY-1: rank to radius and size, top 3 large and in the middle. `repos` is the priority order.
pub fn place(repos: &[String]) -> Vec<Placement> {
    todo!("SKY-1")
}

/// SKY-2: progress to 1.0, 0.0 to 1.0: closed stars in the `1.0` milestone over all stars in
/// it, each `release: 1.0` feature with no issue counting as one open star; `after-1.0` dark
/// stars are not counted. No stars and no dark stars is 0.0.
pub fn brightness(issues: &[Issue], block: &FeatureBlock) -> f64 {
    todo!("SKY-2")
}
