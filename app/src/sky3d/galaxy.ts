// The procedural galaxy of one project: its shape (spiral, barred or elliptical, arm count,
// twist, tilt, colour) and its particles, all from a hash of the repo name. Presentation only:
// what the galaxy means (where it sits, how lit it is) comes from the core.
import { gauss, seeded } from "./random";

export type GalaxyKind = "spiral" | "barred" | "elliptical";

export type GalaxyShape = {
  kind: GalaxyKind;
  /** Spiral arms (2 to 4); 0 for an elliptical. */
  arms: number;
  /** How far the arms wind, in radians from the core to the rim. */
  twist: number;
  /** Disc tilt, radians, about x and about z. */
  tiltX: number;
  tiltZ: number;
  /** Idle spin, radians per second; the sign is the direction. */
  spin: number;
  /** Arm colour, as HSL hue 0..1, from a restrained palette. */
  hue: number;
  /** Elliptical axes (y and z relative to x = 1); 1 for a disc. */
  squashY: number;
  squashZ: number;
};

// Cool blues, teal, violet and one rose: the sky's restrained palette.
const HUES = [0.56, 0.6, 0.64, 0.5, 0.72, 0.78, 0.93];

export function galaxyShape(repo: string): GalaxyShape {
  const r = seeded(`shape:${repo}`);
  const roll = r();
  const kind: GalaxyKind = roll < 0.55 ? "spiral" : roll < 0.8 ? "barred" : "elliptical";
  return {
    kind,
    arms: kind === "elliptical" ? 0 : 2 + Math.floor(r() * 3),
    twist: 2.4 + r() * 2.6,
    tiltX: (r() - 0.5) * 1.1,
    tiltZ: (r() - 0.5) * 0.7,
    spin: (r() < 0.5 ? -1 : 1) * (0.02 + r() * 0.04),
    hue: HUES[Math.floor(r() * HUES.length)],
    squashY: kind === "elliptical" ? 0.45 + r() * 0.3 : 1,
    squashZ: kind === "elliptical" ? 0.6 + r() * 0.35 : 1,
  };
}

/** How many particles a galaxy gets: more for a bigger (higher ranked) project. */
export function particleCount(size: number): number {
  const s = Math.min(1, Math.max(0, size));
  return 320 + Math.round(1500 * s);
}

export type Particles = {
  /** xyz per particle, in a unit galaxy (the disc rim is about radius 1, in the xz plane). */
  positions: Float32Array;
  /** rgb per particle, 0..1. */
  colors: Float32Array;
  /** relative size per particle. */
  sizes: Float32Array;
};

function hsl(h: number, s: number, l: number): [number, number, number] {
  const k = (n: number) => (n + h * 12) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return [f(0), f(8), f(4)];
}

const CORE: [number, number, number] = [1, 0.86, 0.66];

/** The particles of one galaxy. Deterministic per repo and count. */
export function galaxyParticles(repo: string, shape: GalaxyShape, count: number): Particles {
  const r = seeded(`stars:${repo}`);
  const positions = new Float32Array(count * 3);
  const colors = new Float32Array(count * 3);
  const sizes = new Float32Array(count);
  const arm = hsl(shape.hue, 0.6, 0.68);
  const dust = hsl((shape.hue + 0.05) % 1, 0.45, 0.5);

  for (let i = 0; i < count; i++) {
    let x = 0;
    let y = 0;
    let z = 0;
    let c: [number, number, number];
    let size = 0.7 + r() * 0.8;
    const pick = r();

    if (shape.kind === "elliptical") {
      // A soft ellipsoid, dense at the core.
      const rad = Math.pow(r(), 1.8) * 1.05;
      const u = r() * 2 - 1;
      const t = r() * Math.PI * 2;
      const s = Math.sqrt(1 - u * u);
      x = rad * s * Math.cos(t);
      y = rad * u * shape.squashY;
      z = rad * s * Math.sin(t) * shape.squashZ;
      const warm = 1 - rad;
      c = mix(mix(arm, CORE, 0.55), CORE, warm);
    } else if (pick < 0.22) {
      // The core bulge.
      const rad = Math.abs(gauss(r)) * 0.13;
      const t = r() * Math.PI * 2;
      x = rad * Math.cos(t);
      z = rad * Math.sin(t);
      y = gauss(r) * 0.05;
      c = CORE;
      size *= 1.1;
    } else if (shape.kind === "barred" && pick < 0.34) {
      // The bar.
      x = (r() * 2 - 1) * 0.36;
      z = gauss(r) * 0.045;
      y = gauss(r) * 0.03;
      c = mix(CORE, arm, 0.3);
    } else if (pick < 0.86) {
      // The arms: a logarithmic-ish spiral with scatter that widens toward the rim.
      const k = Math.floor(r() * shape.arms);
      const t = 0.08 + Math.pow(r(), 0.75) * 0.95;
      const start = shape.kind === "barred" ? 0.36 : 0;
      const rad = start + t * (1 - start);
      const a = (k / shape.arms) * Math.PI * 2 + shape.twist * t;
      const spread = 0.04 + 0.11 * t;
      x = rad * Math.cos(a) + gauss(r) * spread;
      z = rad * Math.sin(a) + gauss(r) * spread;
      y = gauss(r) * 0.035 * (1.2 - t);
      c = mix(arm, CORE, Math.max(0, 0.5 - t));
      if (r() < 0.08) {
        c = mix(c, [1, 1, 1], 0.6); // a bright young star
        size *= 1.8;
      }
    } else {
      // Faint disc dust between the arms.
      const rad = Math.sqrt(r()) * 1.05;
      const t = r() * Math.PI * 2;
      x = rad * Math.cos(t);
      z = rad * Math.sin(t);
      y = gauss(r) * 0.03;
      c = dust;
      size *= 0.8;
    }

    positions[i * 3] = x;
    positions[i * 3 + 1] = y;
    positions[i * 3 + 2] = z;
    colors[i * 3] = c[0];
    colors[i * 3 + 1] = c[1];
    colors[i * 3 + 2] = c[2];
    sizes[i] = size;
  }
  return { positions, colors, sizes };
}

function mix(a: readonly number[], b: readonly number[], t: number): [number, number, number] {
  return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
}
