//! Locked tests for #30 (SKY-1: place one constellation per repo). Written before the build
//! (R28); the builder never edits this file.

use std::f64::consts::{PI, TAU};
use switchboard_core::sky::layout::{place, Placement};

const EPS: f64 = 1e-9;

fn repos(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("owner/repo{i}")).collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

fn golden() -> f64 {
    PI * (3.0 - 5f64.sqrt())
}

#[test]
fn no_repos_gives_no_placements() {
    assert!(place(&[]).is_empty());
}

#[test]
fn one_placement_per_repo_in_priority_order_with_its_rank() {
    let order = repos(5);
    let placed = place(&order);
    assert_eq!(placed.len(), 5);
    for (i, p) in placed.iter().enumerate() {
        assert_eq!(p.repo, order[i]);
        assert_eq!(p.rank, i);
    }
}

#[test]
fn the_top_three_are_large_and_in_the_middle() {
    let placed = place(&repos(5));
    let radii: Vec<f64> = placed.iter().map(|p| p.radius).collect();
    let sizes: Vec<f64> = placed.iter().map(|p| p.size).collect();
    for (got, want) in radii.iter().zip([0.0, 0.1, 0.2, 0.35, 0.675]) {
        assert!(close(*got, want), "radii {radii:?}");
    }
    for (got, want) in sizes.iter().zip([1.0, 1.0, 1.0, 0.6, 0.4]) {
        assert!(close(*got, want), "sizes {sizes:?}");
    }
}

#[test]
fn ranks_past_three_spread_towards_the_rim() {
    let placed = place(&repos(13));
    // rank 12 of 13: 0.35 + 0.65 * 9/10, size 0.6 - 0.4 * 9/10
    assert!(close(placed[12].radius, 0.935));
    assert!(close(placed[12].size, 0.24));
    for pair in placed.windows(2) {
        assert!(pair[1].radius > pair[0].radius);
        assert!(pair[1].size <= pair[0].size);
    }
    assert!(placed.iter().all(|p| p.radius < 1.0 && p.size > 0.2));
}

#[test]
fn four_repos_put_the_fourth_at_the_inner_edge_of_the_rest() {
    let placed = place(&repos(4));
    assert!(close(placed[3].radius, 0.35));
    assert!(close(placed[3].size, 0.6));
}

#[test]
fn angles_step_by_the_golden_angle_wrapped_into_one_turn() {
    let placed = place(&repos(6));
    for p in &placed {
        let want = (p.rank as f64 * golden()) % TAU;
        assert!(close(p.angle, want), "rank {} angle {}", p.rank, p.angle);
        assert!((0.0..TAU).contains(&p.angle));
    }
    assert!(close(placed[0].angle, 0.0));
}

#[test]
fn moving_a_repo_up_the_order_moves_it_inward_and_makes_it_larger() {
    let before = vec!["a", "b", "c", "d", "e", "f"];
    let after = vec!["a", "b", "c", "f", "d", "e"];
    let find = |placed: &[Placement], repo: &str| -> Placement {
        placed.iter().find(|p| p.repo == repo).unwrap().clone()
    };
    let owned = |v: Vec<&str>| -> Vec<String> { v.into_iter().map(str::to_string).collect() };
    let p1 = place(&owned(before));
    let p2 = place(&owned(after));
    let (was, now) = (find(&p1, "f"), find(&p2, "f"));
    assert!(now.radius < was.radius);
    assert!(now.size > was.size);
}
