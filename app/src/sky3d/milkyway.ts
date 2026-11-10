// One living galaxy (#297, pass 2). The whole sky is a single procedural spiral galaxy; each
// project is a star system embedded in its disc. Where a system sits still comes from the core
// (SKY-1: radius, angle, size); this file turns that into world space and generates the disc,
// the dust and each system's own cloud of gas and stars. Presentation only, seeded per name.
import type { Constellation } from "../sky/model";
import { gauss, seeded } from "./random";
import { shortName } from "./labels";
import { blackbody, nebulaStage, populationTemp, powerLawBrightness } from "./optics";

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
  /** Per particle: 0 a star, 1 glowing gas (soft, large), 3 a bright star with diffraction spikes. */
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

const HII: [number, number, number] = [1, 0.42, 0.55];
const star = (pop: "old" | "disc" | "young", r: () => number) => blackbody(populationTemp(pop, r()));

/** Globular clusters: old, tight balls of stars scattered through the halo. */
const GLOBULARS = 9;
function globularCentres(r: () => number): [number, number, number][] {
  return Array.from({ length: GLOBULARS }, () => {
    const d = 0.25 + r() * 0.75;
    const a = r() * Math.PI * 2;
    return [d, a, (r() < 0.5 ? -1 : 1) * (0.08 + r() * 0.3) * GAL_R];
  });
}

/**
 * The galaxy's stars and glowing gas. Deterministic for a seed and count. Each population has
 * its own temperatures (colour from the Planckian locus) and the brightness of every star is
 * drawn from a power law: a great many faint ones, a few bright, and a handful so bright they
 * show diffraction spikes. Colours are linear HDR light (above 1 blooms).
 */
export function discParticles(seed: string, count: number): DiscParticles {
  const r = seeded(`disc:${seed}`);
  const orbit = new Float32Array(count * 4);
  const colors = new Float32Array(count * 3);
  const phase = new Float32Array(count);
  const kind = new Float32Array(count);
  const globs = globularCentres(seeded(`globular:${seed}`));
  for (let i = 0; i < count; i++) {
    const roll = r();
    let d: number; // share of GAL_R
    let a: number;
    let y: number;
    let c: [number, number, number];
    let b = powerLawBrightness(r(), 0.2, 1.7, 5);
    let size = 0.22 + Math.pow(r(), 5) * 0.6;
    let k = 0;
    if (roll < 0.1) {
      // The old bulge: warm yellow-white, puffy; thin at the very middle, where the black hole
      // has cleared its surroundings.
      d = 0.05 + Math.abs(gauss(r)) * 0.1;
      a = r() * Math.PI * 2;
      y = gauss(r) * 0.05 * GAL_R * Math.max(0.2, 1 - d * 6);
      c = star("old", r);
      b *= 0.9;
    } else if (roll < 0.16) {
      // The bar.
      const along = (r() * 2 - 1) * BAR;
      const across = gauss(r) * 0.022;
      d = Math.max(0.07, Math.hypot(along, across));
      a = BAR_ANGLE + Math.atan2(across, along);
      y = gauss(r) * 0.012 * GAL_R;
      c = star("old", r);
      b *= 0.7;
    } else if (roll < 0.68) {
      // The arms: young blue-white stars and the rose glow of hydrogen where they are born.
      const arm = ARMS.indexOf(pickArm(r));
      d = BAR + Math.pow(r(), 0.85) * (1.25 - BAR);
      const spread = 0.022 + 0.05 * d;
      a = armAngle(arm, d) + (gauss(r) * spread) / Math.max(d, 0.05);
      d += gauss(r) * spread * 0.6;
      // A thin disc: young stars hug the plane, the scale height flaring a little outward.
      y = gauss(r) * 0.005 * GAL_R * (1 + d);
      c = star("young", r);
      // The arms ease out of the bar, so their roots do not pile up into two hot spots.
      b *= 1.25 * (0.4 + 0.6 * Math.min(1, (d - BAR) / 0.22));
      if (r() < 0.045) {
        k = 1;
        c = r() < 0.7 ? HII : [0.45, 0.6, 1];
        b = 0.16 + r() * 0.16;
        size = 2 + r() * 3;
      }
    } else if (roll < 0.965) {
      // The old thin disc between the arms: sun-like and cooler, with real vertical scatter.
      d = Math.min(1.35, -Math.log(1 - r() * 0.985) * 0.3);
      a = r() * Math.PI * 2;
      y = gauss(r) * 0.011 * GAL_R * (1 + d);
      c = star("disc", r);
      b *= 0.75;
    } else {
      // The faint halo above and below, some of it gathered into globular clusters.
      if (r() < 0.4) {
        const g = globs[Math.floor(r() * GLOBULARS)];
        const spread = 0.012;
        const gx = g[0] * Math.cos(g[1]) + gauss(r) * spread;
        const gz = g[0] * Math.sin(g[1]) + gauss(r) * spread;
        d = Math.hypot(gx, gz);
        a = Math.atan2(gz, gx);
        y = g[2] + gauss(r) * spread * GAL_R;
        b = powerLawBrightness(r(), 0.24, 2.2, 1.6);
      } else {
        d = Math.pow(r(), 0.6) * 1.1;
        a = r() * Math.PI * 2;
        y = gauss(r) * 0.22 * GAL_R;
        b *= 0.55;
      }
      c = star("old", r);
    }
    // Soft unresolved light that fills the disc (the galaxy reads as one body, not dots).
    if (k === 0 && r() < 0.06) {
      k = 1;
      size = 3.5 + r() * 5;
      b = Math.min(b, 0.6) * 0.3;
    } else if (k === 0 && b > 3.2 && r() < 0.035) {
      // One of the brightest handful: it gets diffraction spikes.
      k = 3;
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
    phase[i] = 0.2 + r() * 0.28; // dust opacity
  }
  return { orbit, colors, phase, kind, count };
}

// ---- a project's own cloud: from a nebula (stellar nursery) to a cluster -----------------------

/** What kind of object a project's gas is, by seed. */
export type NebulaType = "emission" | "reflection" | "bubble" | "remnant" | "globules";
export const NEBULA_TYPES: NebulaType[] = ["emission", "reflection", "bubble", "remnant", "globules"];

type RGB = [number, number, number];
/** Emission physics: hydrogen-alpha rose, sulphur deep red, doubly ionised oxygen teal. */
const H_ALPHA: RGB = [1, 0.26, 0.4];
const S_II: RGB = [0.95, 0.2, 0.18];
const O_III: RGB = [0.2, 0.82, 0.78];
/** Reflection: starlight scattered by dust near hot stars, blue. */
const REFLECT: RGB = [0.34, 0.55, 1];
const REFLECT_PALE: RGB = [0.62, 0.74, 1];

/**
 * A nebula's look, by seed: its type, the two gases that colour it (mixed per seed), the colour
 * of the bright rims where gas faces its stars, how many depth slices draw it, and its shape.
 */
export type NebulaStyle = {
  type: NebulaType;
  a: RGB;
  b: RGB;
  rim: RGB;
  /** 3 to 5 slices at different depths: volume and parallax. */
  slices: number;
  /** 0..1, the seed the baked texture grows from. */
  seed: number;
  /** Squash of the silhouette (1 round-ish, below 1 elongated). */
  aspect: number;
  tilt: number;
  /** How strongly its dust darkens what is behind. */
  dust: number;
};

export function nebulaStyle(repo: string): NebulaStyle {
  const r = seeded(`nebula:${repo}`);
  const type = NEBULA_TYPES[Math.floor(r() * NEBULA_TYPES.length)];
  const pick = r();
  let a: RGB;
  let b: RGB;
  let rim: RGB;
  switch (type) {
    case "reflection":
      a = REFLECT;
      b = pick < 0.5 ? REFLECT_PALE : H_ALPHA;
      rim = [0.75, 0.85, 1];
      break;
    case "remnant":
      a = pick < 0.6 ? H_ALPHA : S_II;
      b = O_III;
      rim = [1, 0.7, 0.62];
      break;
    case "globules":
      a = H_ALPHA;
      b = pick < 0.5 ? S_II : O_III;
      rim = [1, 0.62, 0.55];
      break;
    default:
      a = pick < 0.7 ? H_ALPHA : S_II;
      b = pick < 0.35 ? REFLECT : O_III;
      rim = [1, 0.78, 0.68];
  }
  return {
    type,
    a,
    b,
    rim,
    slices: 3 + Math.floor(r() * 3),
    seed: r(),
    aspect: 0.55 + r() * 0.45,
    tilt: r() * Math.PI,
    dust: type === "globules" ? 1 : type === "remnant" ? 0.15 : 0.45 + r() * 0.4,
  };
}

/**
 * The stars of one system, in a unit cloud (the gas itself is a baked texture, nebula.ts).
 * `progress` 0 is a dense cloud with a few embedded protostars glowing red; rising, a young
 * blue-white cluster forms at the heart; at 1 it is a clean, brilliant cluster. Colours are
 * linear HDR light from the stars' temperatures.
 */
export function systemParticles(repo: string, progress: number, count: number): DiscParticles & { local: Float32Array } {
  const style = nebulaStyle(repo);
  const st = nebulaStage(progress);
  const r = seeded(`system:${repo}`);
  const cluster = Math.round(count * 0.55 * st.cluster);
  const proto = st.protostars;
  const field = Math.max(8, Math.round(count * 0.18));
  const n = cluster + proto + field;
  const local = new Float32Array(n * 3);
  const colors = new Float32Array(n * 3);
  const phase = new Float32Array(n);
  const kind = new Float32Array(n);
  const orbit = new Float32Array(n * 4);
  const cos = Math.cos(style.tilt);
  const sin = Math.sin(style.tilt);
  for (let i = 0; i < n; i++) {
    let x: number;
    let y: number;
    let z: number;
    let c: RGB;
    let b: number;
    let size: number;
    if (i < cluster) {
      // The young cluster: hot, blue-white, packed toward the centre of the cavity it blew.
      const spread = 0.1 + 0.12 * (1 - st.cluster);
      x = gauss(r) * spread;
      y = gauss(r) * spread * 0.8;
      z = gauss(r) * spread;
      c = blackbody(populationTemp("young", r()));
      b = powerLawBrightness(r(), 0.35, 1.5, 4.5) * (0.6 + 0.6 * st.cluster);
      size = 0.05 + r() * 0.04;
    } else if (i < cluster + proto) {
      // Protostars: still wrapped in the dense gas, a deep red-orange glow.
      x = gauss(r) * 0.32;
      y = gauss(r) * 0.15;
      z = gauss(r) * 0.32;
      c = blackbody(1800 + r() * 900);
      b = 0.8 + r() * 1.2;
      size = 0.08 + r() * 0.04;
    } else {
      // Stars of the disc that happen to lie in front of or behind the cloud.
      x = gauss(r) * 0.6;
      y = gauss(r) * 0.35;
      z = gauss(r) * 0.6;
      c = blackbody(populationTemp("disc", r()));
      b = powerLawBrightness(r(), 0.18, 1.7, 2.5);
      size = 0.04 + r() * 0.03;
    }
    const rx = x * cos - z * sin;
    const rz = x * sin + z * cos;
    local.set([rx, y, rz], i * 3);
    colors.set([c[0] * b, c[1] * b, c[2] * b], i * 3);
    phase[i] = r();
    kind[i] = 0;
    orbit.set([0, 0, 0, size], i * 4);
  }
  return { orbit, colors, phase, kind, count: n, local };
}
