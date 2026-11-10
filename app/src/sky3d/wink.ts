// The sky's live search filter (#297, pass 5): what still matches the query as it is typed, and
// how a non-match winks out: a short star-like flicker as it shrinks to nothing over about 300 ms,
// each one a little after the last (a seeded stagger), so the field goes dark like stars going
// out rather than in one uniform dissolve. Clearing the query winks them back the same way.
// Pure, so it can be tested without a GPU.
import type { Constellation } from "../sky/model";
import { shortName } from "./labels";

/** What stays lit. A null set means "no filter at that level": everything shows. */
export type SkyFilter = {
  /** Projects whose name matches (galaxy level). */
  repos: Set<string> | null;
  /** Inside the open project: the issue stars that match, by number. */
  stars: Set<number> | null;
  /** Inside the open project: features whose title matches or that hold a matching star. */
  features: Set<string> | null;
  /** How many things are lit at the level being looked at. */
  count: number;
};

export const NO_FILTER: SkyFilter = { repos: null, stars: null, features: null, count: -1 };

/**
 * Case-insensitive substring match on the project name (`owner/name` or just `name`), and,
 * inside an open project, on its feature titles and its issues' titles or numbers (`#12`, `12`).
 * The open project itself always stays lit: it is the place being searched.
 */
export function matchSky(constellations: readonly Constellation[], query: string, open: Constellation | null = null): SkyFilter {
  const q = query.trim().toLowerCase();
  if (!q) return NO_FILTER;
  const has = (t: string) => t.toLowerCase().includes(q);
  const repos = new Set(constellations.filter((c) => has(c.repo) || has(shortName(c.repo))).map((c) => c.repo));
  if (!open) return { repos, stars: null, features: null, count: repos.size };
  repos.add(open.repo);
  const num = /^#?(\d+)$/.exec(q);
  const stars = new Set<number>();
  const features = new Set<string>();
  for (const f of open.features) {
    const fHit = has(f.title);
    for (const s of f.stars) {
      const sHit = num ? String(s.number).startsWith(num[1]) : has(s.title);
      if (sHit || fHit) stars.add(s.number);
      if (sHit) features.add(f.id);
    }
    if (fHit) features.add(f.id);
  }
  return { repos, stars, features, count: stars.size + features.size };
}

/** Whether `id` is lit under `set` (no filter lights everything). */
export const lit = <T>(set: Set<T> | null, id: T): boolean => set === null || set.has(id);

export const WINK_MS = 300;
export const WINK_STAGGER_MS = 140;

/** A seeded 0..1 per id (FNV-1a), the same every run: who goes out first. */
export function winkSeed(id: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0) / 0x100000000;
}

/** The wait before `id` starts to wink, in ms. */
export const winkDelay = (id: string): number => winkSeed(id) * WINK_STAGGER_MS;

/**
 * One step of a wink. `p` is how far out a thing is (0 lit, 1 gone); `out` the way it is going;
 * `heldMs` how long since that was last decided; `dtMs` the frame time. It waits out its
 * stagger, then moves at 1 / WINK_MS. Reduced motion: no flicker, no tween, just there or not.
 */
export function winkStep(p: number, out: boolean, dtMs: number, heldMs: number, delayMs: number, reduced = false): number {
  const goal = out ? 1 : 0;
  if (reduced) return goal;
  if (p === goal || heldMs < delayMs) return p;
  const step = Math.max(0, Math.min(dtMs, heldMs - delayMs)) / WINK_MS;
  return out ? Math.min(1, p + step) : Math.max(0, p - step);
}

/**
 * How a thing looks at wink progress `p`: alpha and size multipliers. It flickers while it
 * shrinks (the flicker fades in and out with the wink, so it starts and ends smooth), and is fully gone at 1. The flicker phase is seeded so
 * neighbours do not pulse together.
 */
export function winkLook(p: number, seed = 0, reduced = false): { alpha: number; scale: number } {
  const t = Math.max(0, Math.min(1, Number.isFinite(p) ? p : 0));
  if (reduced) return t >= 0.5 ? { alpha: 0, scale: 0 } : { alpha: 1, scale: 1 };
  if (t <= 0) return { alpha: 1, scale: 1 };
  if (t >= 1) return { alpha: 0, scale: 0 };
  const flick = 1 - 0.65 * Math.abs(Math.sin((t * 3 + seed) * Math.PI)) * Math.sin(t * Math.PI);
  return { alpha: (1 - t) * flick, scale: 1 - t * t };
}

/** Out of play (labels, hit targets, drift pills hidden and not focusable) past halfway. */
export const winkedOut = (p: number): boolean => p >= 0.5;
