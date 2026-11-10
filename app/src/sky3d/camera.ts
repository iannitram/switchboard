// The camera for continuous, semantic zoom (#297, pass 2). The wheel, a trackpad pinch and a
// touchscreen pinch all change one goal pose; the camera eases toward it every frame. Getting
// close enough to a project enters it (the same state as a click); backing out leaves it.
import type { Pose, Vec3 } from "./layout";

/** Closest the camera may come to what it looks at, in world units: one star fills the view. */
export const MIN_DIST = 2.4;
/** Enter a project's constellation inside ENTER * R, leave it past EXIT * R (hysteresis). */
export const ENTER = 3.0;
export const EXIT = 3.8;
/** The constellation fades in between DETAIL_FAR * R and DETAIL_NEAR * R: never pops. */
export const DETAIL_NEAR = 3.3;
export const DETAIL_FAR = 5.2;
/** The click flight: about half a second, a little longer for a long way. */
export const FLIGHT_MIN_MS = 450;
export const FLIGHT_MAX_MS = 600;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * One wheel event as a zoom factor (above 1 zooms out). Lines and pages are scaled to pixels;
 * a trackpad pinch arrives as a wheel event with ctrlKey and small deltas, so it is stronger
 * per unit. A single event is capped, so a fast flick cannot jump the whole galaxy.
 */
export function wheelFactor(deltaY: number, deltaMode: number, ctrlKey: boolean): number {
  const px = deltaMode === 1 ? deltaY * 16 : deltaMode === 2 ? deltaY * 400 : deltaY;
  const k = ctrlKey ? 0.011 : 0.0016;
  return Math.exp(clamp(px * k, -0.6, 0.6));
}

/** Two fingers moving apart zoom in: the factor is the old span over the new. */
export function pinchFactor(prevSpan: number, span: number): number {
  if (!(prevSpan > 0) || !(span > 0)) return 1;
  return clamp(prevSpan / span, 0.5, 2);
}

/**
 * Zoom by `factor` toward `anchor` (the point under the cursor or between the fingers): the
 * anchor stays put on screen while everything else scales about it.
 */
export function zoomToward(p: Pose, factor: number, anchor: Vec3 | null, minDist: number, maxDist: number): Pose {
  const dist = clamp(p.dist * factor, minDist, maxDist);
  const f = dist / p.dist;
  if (!anchor || f === 1) return { ...p, dist };
  return {
    ...p,
    dist,
    target: {
      x: anchor.x + (p.target.x - anchor.x) * f,
      y: anchor.y + (p.target.y - anchor.y) * f,
      z: anchor.z + (p.target.z - anchor.z) * f,
    },
  };
}

/** Frame-rate independent exponential easing of a value toward its goal. */
export function damp(cur: number, goal: number, lambda: number, dt: number): number {
  return goal + (cur - goal) * Math.exp(-lambda * dt);
}

/** Ease a pose toward the goal: distance in log space (a zoom feels even), angles the short way. */
export function dampPose(cur: Pose, goal: Pose, dt: number, lambda = 9): Pose {
  let daz = goal.az - cur.az;
  daz = Math.atan2(Math.sin(daz), Math.cos(daz));
  return {
    target: {
      x: damp(cur.target.x, goal.target.x, lambda, dt),
      y: damp(cur.target.y, goal.target.y, lambda, dt),
      z: damp(cur.target.z, goal.target.z, lambda, dt),
    },
    dist: Math.exp(damp(Math.log(cur.dist), Math.log(goal.dist), lambda, dt)),
    az: goal.az - daz * Math.exp(-lambda * dt),
    elev: damp(cur.elev, goal.elev, lambda, dt),
  };
}

/** Still easing: the camera has not reached its goal. */
export function settling(cur: Pose, goal: Pose): boolean {
  const d = Math.hypot(cur.target.x - goal.target.x, cur.target.y - goal.target.y, cur.target.z - goal.target.z);
  return d > goal.dist * 1e-3 || Math.abs(Math.log(cur.dist / goal.dist)) > 1e-3 || Math.abs(cur.az - goal.az) > 1e-4 || Math.abs(cur.elev - goal.elev) > 1e-4;
}

export function flightMs(from: Pose, to: Pose, reducedMotion: boolean): number {
  if (reducedMotion) return 0;
  const zoom = Math.abs(Math.log(to.dist / from.dist));
  const move = Math.hypot(to.target.x - from.target.x, to.target.z - from.target.z) / Math.max(from.dist, to.dist);
  return clamp(FLIGHT_MIN_MS + 35 * zoom + 60 * move, FLIGHT_MIN_MS, FLIGHT_MAX_MS);
}

export type Zone = { repo: string; x: number; y: number; z: number; R: number };

/**
 * After the goal moved: does the camera now enter a project, or leave the open one? The project
 * in question is the one the camera looks at (its centre within 1.4 R of the target).
 */
export function zoneStep(open: string | null, goal: Pose, zones: Iterable<Zone>): { enter: string } | { leave: true } | null {
  let near: Zone | null = null;
  let best = Infinity;
  let openZone: Zone | null = null;
  for (const z of zones) {
    if (z.repo === open) openZone = z;
    const d = Math.hypot(z.x - goal.target.x, z.y - goal.target.y, z.z - goal.target.z) / z.R;
    if (d < 1.4 && d < best) {
      best = d;
      near = z;
    }
  }
  if (open !== null) {
    if (!openZone) return { leave: true };
    const off = Math.hypot(openZone.x - goal.target.x, openZone.z - goal.target.z) / openZone.R;
    if (goal.dist > EXIT * openZone.R || off > 2.2) return { leave: true };
    return null;
  }
  if (near && goal.dist < ENTER * near.R) return { enter: near.repo };
  return null;
}

/** How much of a constellation shows at a camera distance: 0 far, 1 near, smooth between. */
export function detailFade(dist: number, R: number): number {
  const t = clamp((DETAIL_FAR * R - dist) / ((DETAIL_FAR - DETAIL_NEAR) * R), 0, 1);
  return t * t * (3 - 2 * t);
}

/** How many project names the overview may try to place: more as the camera comes in. */
export function labelBudget(dist: number, overviewDist: number, total: number, base = 14): number {
  const k = overviewDist / Math.max(dist, 1e-6);
  return Math.min(total, Math.max(base, Math.round(base * k * k)));
}
