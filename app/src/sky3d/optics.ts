// Light as a telescope records it (#297, pass 3): star colour from temperature, brightness from
// a power law, exposure that follows the zoom, and how a nebula turns its gas into stars.
// Pure functions only; the scene and its shaders read them.

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const smooth = (a: number, b: number, x: number) => {
  const t = clamp((x - a) / (b - a), 0, 1);
  return t * t * (3 - 2 * t);
};

/**
 * The colour of a black body at `kelvin`, along the Planckian locus: red-orange when cool, white
 * near 6500 K, blue-white when hot. Display RGB, scaled so the strongest channel is 1 (brightness
 * is a separate matter). A fit to the CIE 1964 curve, good from 1000 K to 40000 K.
 */
export function blackbody(kelvin: number): [number, number, number] {
  const t = clamp(kelvin, 1000, 40000) / 100;
  let r: number;
  let g: number;
  let b: number;
  if (t <= 66) {
    r = 255;
    g = 99.4708025861 * Math.log(t) - 161.1195681661;
    b = t <= 19 ? 0 : 138.5177312231 * Math.log(t - 10) - 305.0447927307;
  } else {
    r = 329.698727446 * Math.pow(t - 60, -0.1332047592);
    g = 288.1221695283 * Math.pow(t - 60, -0.0755148492);
    b = 255;
  }
  const c = [clamp(r, 0, 255), clamp(g, 0, 255), clamp(b, 0, 255)];
  const m = Math.max(c[0], c[1], c[2]);
  return [c[0] / m, c[1] / m, c[2] / m];
}

/** Temperatures by stellar population, in kelvin: [coolest, hottest]. */
export const POPULATIONS = {
  /** The old bulge and halo: warm yellow-white and orange giants. */
  old: [3300, 5600],
  /** The thin disc between the arms: mostly sun-like and cooler. */
  disc: [3800, 7500],
  /** The arms: young, blue-white. */
  young: [7000, 26000],
} as const;

/** A temperature for a population from a uniform draw `u`, cool stars far more common. */
export function populationTemp(pop: keyof typeof POPULATIONS, u: number): number {
  const [lo, hi] = POPULATIONS[pop];
  // Cool stars dominate any population: the draw is bent toward the low end.
  return lo * Math.pow(hi / lo, Math.pow(clamp(u, 0, 1), 2.2));
}

/**
 * Apparent brightness from a uniform draw: a power law, so most stars are faint and a very few
 * are bright. `u` near 0 gives the floor, near 1 the rare bright star; capped at `max`.
 */
export function powerLawBrightness(u: number, floor = 0.18, slope = 1.6, max = 6): number {
  return Math.min(max, floor * Math.pow(1 - clamp(u, 0, 0.9999), -1 / slope));
}

/**
 * Exposure for a camera distance as a share of the whole-galaxy distance (1 = overview). Close
 * in, bright things fill the frame, so the exposure comes down: cores never clip and every star
 * in a crowded figure stays a separate point. Log-linear in the zoom, flat outside the range.
 */
export const EXPOSURE_FAR = 1;
export const EXPOSURE_NEAR = 0.42;
export function exposureForZoom(zoom: number): number {
  if (!(zoom > 0)) return EXPOSURE_NEAR;
  const near = 0.012;
  const t = clamp(Math.log(zoom / near) / Math.log(1 / near), 0, 1);
  return EXPOSURE_NEAR + (EXPOSURE_FAR - EXPOSURE_NEAR) * t;
}

/**
 * How far a project's nebula has turned its gas into stars, from its progress (0..1):
 * - `gas`: how much glowing gas is left (1 a dense cloud, a few wisps at the end);
 * - `cavity`: the radius (share of the cloud) a young cluster has blown clear;
 * - `cluster`: how bright the young cluster at the heart is;
 * - `protostars`: how many embedded, still-forming stars glow inside the dense gas.
 */
export type NebulaStage = { gas: number; cavity: number; cluster: number; protostars: number };
export function nebulaStage(progress: number): NebulaStage {
  const p = clamp(Number.isFinite(progress) ? progress : 0, 0, 1);
  return {
    gas: 1 - 0.78 * smooth(0.15, 1, p),
    cavity: 0.72 * smooth(0.25, 0.95, p),
    cluster: smooth(0.2, 1, p),
    protostars: Math.round(7 * (1 - smooth(0.15, 0.7, p))),
  };
}
