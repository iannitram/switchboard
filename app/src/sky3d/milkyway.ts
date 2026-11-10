// One living galaxy (#297, pass 2). The whole sky is a single procedural spiral galaxy; each
// project is a star system embedded in its disc. Where a system sits still comes from the core
// (SKY-1: radius, angle, size); this file turns that into world space and generates the disc,
// the dust and each system's own cloud of gas and stars. Presentation only, seeded per name.
import type { Constellation } from "../sky/model";
import { gauss, seeded } from "./random";
import { shortName } from "./labels";

/** World units from the galactic centre to the rim of the project disc (core radius 1). */
export const GAL_R = 110;
/** The core's radius 0 maps to this share of GAL_R, so the very centre stays free. */
export const R0 = 0.25;
/** Most a system sits above or below the disc plane, at the rim. */
export const DISC_H = 2.2;

/** The black hole at the centre: horizon radius, accretion disc rim, its system's radius. */
export const HOLE = { horizon: 2.4, discOuter: 10.5, systemR: 22 } as const;

/** SKY-1 radius (0 centre .. 1 rim) to a share of GAL_R, leaving the centre free. */
export function remapRadius(radius: number): number {
  return R0 + (1 - R0) * Math.min(1, Math.max(0, radius));
}

/** The project that is the black hole: the repo named `switchboard` (after the owner). */
export function isCentre(repo: string): boolean {
  return shortName(repo).toLowerCase() === "switchboard";
}

export function centreRepo(cs: readonly { repo: string }[]): string | null {
  return cs.find((c) => isCentre(c.repo))?.repo ?? null;
}

export type System = {
  repo: string;
  x: number;
  y: number;
  z: number;
  /** The visual radius of its cloud. */
  scale: number;
  /** The radius its constellation opens to (milestone rings), and the zoom scale near it. */
  R: number;
  centre: boolean;
};

/**
 * Every project's place. SKY-1 sets distance from the centre (rank), direction (angle) and
 * size; radius is remapped so rank 0 is not on top of the centre. One deliberate departure from
 * SKY-1, at the owner's request: the `switchboard` repo is the black hole at the very centre,
 * whatever its rank (its panel still shows the rank).
 */
export function placeSystems(cs: readonly Pick<Constellation, "repo" | "radius" | "angle" | "size">[]): Map<string, System> {
  const out = new Map<string, System>();
  const centre = centreRepo(cs);
  for (const c of cs) {
    if (c.repo === centre) {
      out.set(c.repo, { repo: c.repo, x: 0, y: 0, z: 0, scale: HOLE.discOuter, R: HOLE.systemR, centre: true });
      continue;
    }
    const d = GAL_R * remapRadius(c.radius);
    const r = seeded(`depth:${c.repo}`);
    const size = Math.min(1, Math.max(0, c.size));
    out.set(c.repo, {
      repo: c.repo,
      x: d * Math.cos(c.angle),
      y: (r() * 2 - 1) * DISC_H * (0.35 + 0.65 * c.radius),
      z: d * Math.sin(c.angle),
      scale: 2.4 + 4.2 * size,
      R: 0,
      centre: false,
    });
  }
  // Each system's opened radius: a share of the gap to its nearest neighbour, so none overlap.
  const all = [...out.values()];
  for (const s of all) {
    if (s.centre) continue;
    let nn = Infinity;
    for (const o of all) {
      if (o === s) continue;
      const gap = Math.hypot(o.x - s.x, o.y - s.y, o.z - s.z) - (o.centre ? o.R : 0);
      nn = Math.min(nn, o.centre ? gap * 2 : gap);
    }
    s.R = Math.min(10, Math.max(s.scale * 1.5, Number.isFinite(nn) ? nn * 0.45 : 10));
  }
  return out;
}

// ---- the disc --------------------------------------------------------------------------------

export type DiscParticles = {
  /** Per particle: orbit radius, start angle, height, world size. */
  orbit: Float32Array;
  colors: Float32Array;
  /** Per particle: 0..1, for twinkle. */
  phase: Float32Array;
  /** Per particle: 0 a star, 1 glowing gas (soft, large). */
  kind: Float32Array;
  count: number;
};

const ARMS = [
  { at: 0, weight: 0.32 },
  { at: Math.PI, weight: 0.32 },
  { at: Math.PI * 0.5, weight: 0.18 },
  { at: Math.PI * 1.5, weight: 0.18 },
];
/** Spiral pitch: how fast an arm winds out (radians of angle per unit of ln r). */
const WIND = 1 / Math.tan((21 * Math.PI) / 180);
const BAR = 0.17;
const BAR_ANGLE = 0.45;

function pickArm(r: () => number) {
  let u = r();
  for (const a of ARMS) {
    if (u < a.weight) return a;
    u -= a.weight;
  }
  return ARMS[0];
}

/** The angle of an arm's spine at distance `d` (a share of GAL_R) from the centre. */
export function armAngle(arm: number, d: number): number {
  return ARMS[arm % ARMS.length].at + BAR_ANGLE + Math.log(Math.max(d, BAR) / BAR) * WIND;
}

const WARM: [number, number, number] = [1, 0.84, 0.6];
const OLD: [number, number, number] = [0.98, 0.88, 0.74];
const YOUNG: [number, number, number] = [0.66, 0.8, 1];
const HII: [number, number, number] = [1, 0.55, 0.66];

/** The galaxy's stars and glowing gas. Deterministic for a seed and count. */
export function discParticles(seed: string, count: number): DiscParticles {
  const r = seeded(`disc:${seed}`);
  const orbit = new Float32Array(count * 4);
  const colors = new Float32Array(count * 3);
  const phase = new Float32Array(count);
  const kind = new Float32Array(count);
  for (let i = 0; i < count; i++) {
    const roll = r();
    let d: number; // share of GAL_R
    let a: number;
    let y: number;
    let c: [number, number, number];
    let b: number;
    let size = 0.22 + Math.pow(r(), 5) * 0.6;
    let k = 0;
    if (roll < 0.09) {
      // The bulge: warm, puffy.
      // Thin at the very middle, where the black hole has cleared its surroundings.
      d = 0.05 + Math.abs(gauss(r)) * 0.1;
      a = r() * Math.PI * 2;
      y = gauss(r) * 0.05 * GAL_R * Math.max(0.2, 1 - d * 6);
      c = WARM;
      b = 0.2 + r() * 0.3;
    } else if (roll < 0.15) {
      // The bar.
      const along = (r() * 2 - 1) * BAR;
      const across = gauss(r) * 0.022;
      d = Math.max(0.07, Math.hypot(along, across));
      a = BAR_ANGLE + Math.atan2(across, along);
      y = gauss(r) * 0.012 * GAL_R;
      c = WARM;
      b = 0.14 + r() * 0.16;
    } else if (roll < 0.7) {
      // The arms: young blue-white stars, a few hot ones, and pink star-forming gas.
      const arm = ARMS.indexOf(pickArm(r));
      d = BAR + Math.pow(r(), 0.85) * (1.25 - BAR);
      const spread = 0.022 + 0.05 * d;
      a = armAngle(arm, d) + (gauss(r) * spread) / Math.max(d, 0.05);
      d += gauss(r) * spread * 0.6;
      y = gauss(r) * 0.006 * GAL_R * (1 + d);
      c = YOUNG;
      // The arms ease out of the bar, so their roots do not pile up into two hot spots.
      b = (0.35 + r() * 0.45) * (0.4 + 0.6 * Math.min(1, (d - BAR) / 0.22));
      const hot = r();
      if (hot < 0.04) {
        c = [0.85, 0.92, 1];
        b = 1;
        size *= 2.2;
      } else if (hot < 0.09) {
        k = 1;
        c = r() < 0.55 ? HII : [0.5, 0.62, 1];
        b = 0.2 + r() * 0.2;
        size = 2.2 + r() * 3.2;
      }
    } else if (roll < 0.97) {
      // The old disc between the arms: faint, warm.
      d = Math.min(1.35, -Math.log(1 - r() * 0.985) * 0.3);
      a = r() * Math.PI * 2;
      y = gauss(r) * 0.01 * GAL_R * (1 + d);
      c = OLD;
      b = 0.18 + r() * 0.3;
    } else {
      // A sparse halo above and below.
      d = Math.pow(r(), 0.6) * 1.1;
      a = r() * Math.PI * 2;
      y = gauss(r) * 0.22 * GAL_R;
      c = OLD;
      b = 0.15 + r() * 0.25;
    }
    // Soft glow that fills the disc with light (the galaxy reads as one body, not dots).
    if (k === 0 && r() < 0.06) {
      k = 1;
      size = 3.5 + r() * 5;
      b *= 0.3;
    }
    orbit.set([Math.max(0.2, d * GAL_R), a, y, size], i * 4);
    colors.set([c[0] * b, c[1] * b, c[2] * b], i * 3);
    phase[i] = r();
    kind[i] = k;
  }
  return { orbit, colors, phase, kind, count };
}

/** Dark dust lanes along the inner edge of each arm. Same layout as the disc. */
export function dustParticles(seed: string, count: number): DiscParticles {
  const r = seeded(`dust:${seed}`);
  const orbit = new Float32Array(count * 4);
  const colors = new Float32Array(count * 3);
  const phase = new Float32Array(count);
  const kind = new Float32Array(count).fill(2);
  for (let i = 0; i < count; i++) {
    const arm = ARMS.indexOf(pickArm(r));
    const d = BAR * 1.15 + Math.pow(r(), 0.9) * (1.1 - BAR * 1.15);
    const a = armAngle(arm, d) - 0.2 - Math.abs(gauss(r)) * 0.1;
    const dd = d + gauss(r) * 0.016;
    orbit.set([dd * GAL_R, a, gauss(r) * 0.25, 2.2 + r() * 3.6], i * 4);
    const t = r();
    colors.set([0.035 + t * 0.03, 0.025 + t * 0.02, 0.02 + t * 0.015], i * 3);
    phase[i] = 0.1 + r() * 0.22; // dust opacity
  }
  return { orbit, colors, phase, kind, count };
}

// ---- a project's own cloud: from a nebula (stellar nursery) to a cluster -----------------------

export type NebulaForm = "diffuse" | "filament" | "shell" | "bipolar";

export type NebulaStyle = { form: NebulaForm; a: [number, number, number]; b: [number, number, number]; twist: number; tilt: number };

// Emission colours: hydrogen rose, oxygen teal, reflection blue, violet, amber, sulphur red.
const EMISSION: [number, number, number][] = [
  [1, 0.38, 0.55],
  [0.3, 0.9, 0.82],
  [0.42, 0.6, 1],
  [0.72, 0.45, 1],
  [1, 0.68, 0.32],
  [1, 0.3, 0.3],
];
const FORMS: NebulaForm[] = ["diffuse", "filament", "shell", "bipolar"];

export function nebulaStyle(repo: string): NebulaStyle {
  const r = seeded(`nebula:${repo}`);
  const form = FORMS[Math.floor(r() * FORMS.length)];
  const ia = Math.floor(r() * EMISSION.length);
  const ib = (ia + 1 + Math.floor(r() * (EMISSION.length - 1))) % EMISSION.length;
  return { form, a: EMISSION[ia], b: EMISSION[ib], twist: (r() * 2 - 1) * 2.4, tilt: r() * Math.PI };
}

/** Particles for one system, in a unit cloud. `condensed` 0 = all gas, 1 = a bright cluster. */
export function systemParticles(repo: string, condensed: number, count: number): DiscParticles & { local: Float32Array } {
  const style = nebulaStyle(repo);
  const r = seeded(`system:${repo}`);
  const c = Math.min(1, Math.max(0, condensed));
  const local = new Float32Array(count * 3);
  const colors = new Float32Array(count * 3);
  const phase = new Float32Array(count);
  const kind = new Float32Array(count);
  const orbit = new Float32Array(count * 4);
  const starShare = 0.18 + 0.7 * c;
  const cos = Math.cos(style.tilt);
  const sin = Math.sin(style.tilt);
  for (let i = 0; i < count; i++) {
    let x: number;
    let y: number;
    let z: number;
    let col: [number, number, number];
    let size: number;
    let k: number;
    const t = r();
    if (r() < starShare) {
      // Stars: spread wide in a young nebula, a tight bright ball when condensed.
      const spread = 0.55 - 0.32 * c;
      x = gauss(r) * spread;
      y = gauss(r) * spread * 0.7;
      z = gauss(r) * spread;
      const hot = r() < 0.12 + 0.2 * c;
      col = hot ? [1, 0.95, 0.85] : mix([0.7, 0.82, 1], [1, 0.86, 0.66], r());
      const b = (0.45 + 0.55 * c) * (hot ? 1 : 0.55 + 0.4 * r());
      col = [col[0] * b, col[1] * b, col[2] * b];
      size = (hot ? 0.09 : 0.05) * (0.7 + r() * 0.6);
      k = 0;
    } else if (r() < 0.4) {
      // Wisps: thin bright threads that wind through the gas, so a nebula has structure up close.
      const j = Math.floor(r() * WISPS);
      const wr = seeded(`wisp:${repo}:${j}`);
      const [cx, cy, cz] = gasPoint(style, wr, repo);
      const dir = wr() * Math.PI * 2;
      const bend = 2 + wr() * 3;
      const ph = wr() * 6.3;
      const len = 0.35 + wr() * 0.35;
      const u = r() * 2 - 1;
      const along = u * len;
      const off = Math.sin(u * bend + ph) * 0.09;
      x = cx + Math.cos(dir) * along - Math.sin(dir) * off + gauss(r) * 0.02;
      z = cz + Math.sin(dir) * along + Math.cos(dir) * off + gauss(r) * 0.02;
      y = cy * 0.6 + gauss(r) * 0.03;
      const shrink = 1 - 0.35 * c;
      x *= shrink;
      y *= shrink;
      z *= shrink;
      col = mix(style.a, style.b, Math.min(1, Math.max(0, 0.5 + x + 0.3 * u)));
      const b = (0.5 - 0.3 * c) * (1 - Math.abs(u) * 0.7);
      col = [col[0] * b, col[1] * b, col[2] * b];
      size = 0.08 + r() * 0.1;
      k = 2;
    } else {
      // Gas, shaped by the form.
      [x, y, z] = gasPoint(style, r, repo);
      size = 0.55 + r() * 0.6;
      k = 1;
      // Gas thins and pulls in as the system condenses.
      const shrink = 1 - 0.35 * c;
      x *= shrink;
      y *= shrink;
      z *= shrink;
      col = mix(style.a, style.b, Math.min(1, Math.max(0, 0.5 + x)));
      const b = (0.55 - 0.3 * c) * (0.6 + 0.4 * r());
      col = [col[0] * b, col[1] * b, col[2] * b];
    }
    const rx = x * cos - z * sin;
    const rz = x * sin + z * cos;
    local.set([rx, y, rz], i * 3);
    colors.set(col, i * 3);
    phase[i] = t;
    kind[i] = k;
    orbit.set([0, 0, 0, size], i * 4);
  }
  return { orbit, colors, phase, kind, count, local };
}

const WISPS = 6;

/** A point in a nebula's gas, by its form (system-local units, before the tilt). */
function gasPoint(style: { form: string; twist: number }, r: () => number, repo: string): [number, number, number] {
  let x = 0;
  let y = 0;
  let z = 0;
  switch (style.form) {
    case "filament": {
      const s = r() * 2 - 1;
      const ang = s * style.twist;
      x = s * 0.75 + gauss(r) * 0.18;
      z = Math.sin(ang * 1.5) * 0.35 + gauss(r) * 0.18;
      y = gauss(r) * 0.14;
      break;
    }
    case "shell": {
      const u = r() * 2 - 1;
      const th = r() * Math.PI * 2;
      const rad = 0.62 + gauss(r) * 0.07;
      const s = Math.sqrt(1 - u * u);
      x = rad * s * Math.cos(th);
      y = rad * u * 0.8;
      z = rad * s * Math.sin(th);
      break;
    }
    case "bipolar": {
      const side = r() < 0.5 ? -1 : 1;
      const along = Math.pow(r(), 0.7) * 0.7;
      const w = 0.14 + along * 0.4;
      x = side * along;
      y = gauss(r) * w;
      z = gauss(r) * w;
      break;
    }
    default: {
      const lobe = Math.floor(r() * 4);
      const lr = seeded(`lobe:${repo}:${lobe}`);
      const cx = (lr() * 2 - 1) * 0.4;
      const cz = (lr() * 2 - 1) * 0.4;
      x = cx + gauss(r) * 0.28;
      z = cz + gauss(r) * 0.28;
      y = gauss(r) * 0.16;
    }
  }
  return [x, y, z];
}

function mix(a: readonly number[], b: readonly number[], t: number): [number, number, number] {
  return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
}
