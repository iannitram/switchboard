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
    let n = repos.len();
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    repos
        .iter()
        .enumerate()
        .map(|(rank, repo)| {
            let (radius, size) = if rank < 3 {
                (0.1 * rank as f64, 1.0)
            } else {
                let f = (rank - 3) as f64 / (n - 3) as f64;
                (0.35 + 0.65 * f, 0.6 - 0.4 * f)
            };
            Placement {
                repo: repo.clone(),
                rank,
                radius,
                size,
                angle: (rank as f64 * golden) % std::f64::consts::TAU,
            }
        })
        .collect()
}

/// SKY-2: progress to 1.0, 0.0 to 1.0: closed stars in the `1.0` milestone over all stars in
/// it, each `release: 1.0` feature with no issue counting as one open star; `after-1.0` dark
/// stars are not counted. No stars and no dark stars is 0.0.
pub fn brightness(issues: &[Issue], block: &FeatureBlock) -> f64 {
    todo!("SKY-2")
}
