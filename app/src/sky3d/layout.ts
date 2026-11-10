// Where things sit in the 3D sky. The core gives each constellation its radius, angle and size
// (SKY-1); this turns them into world coordinates and adds depth, which is presentation only.
import type { Constellation, Release } from "../sky/model";
import { seeded } from "./random";

/** World units from the centre of the sky to the rim (radius 1). */
export const SKY_REACH = 100;
/** Radius 0 sits this far out (as a share of the reach), so the top 3 never overlap. */
export const INNER = 0.12;
/** Most the sky is lifted above or sunk below its plane, at the rim. */
export const DEPTH = 16;

export type Vec3 = { x: number; y: number; z: number };

/** A galaxy's place in the world and its visual radius (world units). */
export function galaxyPlacement(c: Pick<Constellation, "repo" | "radius" | "angle" | "size">): Vec3 & { scale: number } {
  const dist = SKY_REACH * (INNER + (1 - INNER) * c.radius);
  const r = seeded(`depth:${c.repo}`);
  return {
    x: dist * Math.cos(c.angle),
    y: (r() * 2 - 1) * DEPTH * (0.25 + 0.75 * c.radius),
    z: dist * Math.sin(c.angle),
    scale: 4.5 + 8 * Math.min(1, Math.max(0, c.size)),
  };
}

// ---- One constellation, opened (SKY-3) ------------------------------------------------------
// Local units: the outermost milestone ring has radius 1, in the xz plane. Each feature is a
// cluster on its milestone's ring; each issue a star in the cluster; a feature with no issue is
// one dark star.

export const RELEASES: readonly Release[] = ["1.0", "after-1.0", "after-2.0"];
export const RING_NAMES: Record<Release, string> = { "1.0": "1.0", "after-1.0": "After 1.0", "after-2.0": "After 2.0" };

const STAR_GAP = 0.042;
const DARK_R = 0.03;
const CLUSTER_GAP = 0.05;

/** A cluster's radius for n stars (sunflower packing). */
export function clusterRadius(n: number): number {
  return n === 0 ? DARK_R : STAR_GAP * 0.9 * Math.sqrt(n) + STAR_GAP * 0.6;
}

export type InteriorLayout = {
  rings: { release: Release; radius: number }[];
  features: Map<string, Vec3 & { r: number }>;
  stars: Map<number, Vec3>;
};

export function layoutInterior(c: { repo: string; features: { id: string; release: Release; stars: { number: number }[] }[] }): InteriorLayout {
  const groups = RELEASES.map((release) => c.features.filter((f) => f.release === release));
  // The ring each group needs: its clusters side by side round the circumference.
  const need = groups.map((g) => g.reduce((s, f) => s + 2 * clusterRadius(f.stars.length) + CLUSTER_GAP, 0) / (2 * Math.PI));
  const widest = groups.map((g) => g.reduce((m, f) => Math.max(m, clusterRadius(f.stars.length)), 0));
  const radii: number[] = [];
  radii[0] = Math.max(0.42, need[0]);
  radii[1] = Math.max(radii[0] + 0.16 + widest[0] + widest[1], need[1]);
  radii[2] = Math.max(radii[1] + 0.12 + widest[1] + widest[2], need[2]);
  const k = 1 / radii[2];

  const features = new Map<string, Vec3 & { r: number }>();
  const stars = new Map<number, Vec3>();
  groups.forEach((g, gi) => {
    const ring = radii[gi];
    const total = g.reduce((s, f) => s + 2 * clusterRadius(f.stars.length) + CLUSTER_GAP, 0);
    // Spread the slack evenly, so a sparse ring is not bunched at the top.
    const slack = Math.max(0, 2 * Math.PI * ring - total) / Math.max(1, g.length);
    let arc = 0;
    g.forEach((f) => {
      const cr = clusterRadius(f.stars.length);
      const span = 2 * cr + CLUSTER_GAP + slack;
      const a = -Math.PI / 2 + (arc + span / 2) / ring;
      arc += span;
      const fx = ring * Math.cos(a);
      const fz = ring * Math.sin(a);
      features.set(f.id, { x: fx * k, y: 0, z: fz * k, r: cr * k });
      const rand = seeded(`cluster:${c.repo}:${f.id}`);
      const turn = rand() * Math.PI * 2;
      f.stars.forEach((s, j) => {
        const rr = STAR_GAP * 0.9 * Math.sqrt(j + 0.5);
        const t = turn + j * 2.39996;
        stars.set(s.number, { x: (fx + rr * Math.cos(t)) * k, y: (rand() - 0.5) * 0.04 * k, z: (fz + rr * Math.sin(t)) * k });
      });
    });
  });
  return { rings: RELEASES.map((release, i) => ({ release, radius: radii[i] * k })), features, stars };
}

// ---- The camera ---------------------------------------------------------------------------

/** How far a camera must stand to see a sphere of `radius`, given the vertical fov and aspect. */
export function fitDistance(radius: number, vfovDeg: number, aspect: number): number {
  const v = (vfovDeg * Math.PI) / 180;
  const h = 2 * Math.atan(Math.tan(v / 2) * aspect);
  return radius / Math.sin(Math.min(v, h) / 2);
}

/** A camera pose: look at `target` from `dist` away, at azimuth and elevation (radians). */
export type Pose = { target: Vec3; dist: number; az: number; elev: number };

export function poseEye(p: Pose): Vec3 {
  const ce = Math.cos(p.elev);
  return {
    x: p.target.x + p.dist * ce * Math.sin(p.az),
    y: p.target.y + p.dist * Math.sin(p.elev),
    z: p.target.z + p.dist * ce * Math.cos(p.az),
  };
}

/** Blend two poses: targets and angles linearly, distance geometrically (a zoom feels even). */
export function blendPose(a: Pose, b: Pose, t: number): Pose {
  if (t <= 0) return a;
  if (t >= 1) return b;
  const lerp = (x: number, y: number) => x + (y - x) * t;
  return {
    target: { x: lerp(a.target.x, b.target.x), y: lerp(a.target.y, b.target.y), z: lerp(a.target.z, b.target.z) },
    dist: Math.exp(lerp(Math.log(a.dist), Math.log(b.dist))),
    az: lerp(a.az, b.az),
    elev: lerp(a.elev, b.elev),
  };
}
