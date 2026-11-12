// State told by stellar physics (#297, pass 2). Each mapping here turns something the core sent
// into a look; the core decides what is true, this only decides how it shines.
import type { Constellation, Star } from "../sky/model";

/** SKY-2: how far a project has condensed, nebula (0) to bright cluster (1). Uncharted is 0. */
export function condensation(c: Pick<Constellation, "brightness" | "features">): number {
  if (c.features.length === 0) return 0;
  return Math.min(1, Math.max(0, c.brightness));
}

/** A star's magnitude from its `size:` label: easy small, medium larger, hard a giant. */
export function magnitude(size: string | null): number {
  switch (size) {
    case "easy":
      return 1;
    case "medium":
      return 1.4;
    case "hard":
      return 2;
    default:
      return 0.9;
  }
}

export type StarKind = "closed" | "open" | "pulsar";

/** SKY-4 and SKY-8 as physics: closed burns steady, open is a protostar, in flight a pulsar. */
export function starPhysics(s: Pick<Star, "state" | "inFlight" | "size" | "pr">): {
  kind: StarKind;
  magnitude: number;
  corona: "pass" | "fail" | null;
  neutral: boolean;
} {
  return {
    kind: s.inFlight ? "pulsar" : s.state === "closed" ? "closed" : "open",
    magnitude: magnitude(s.size),
    corona: s.pr?.verdict === "PASS" ? "pass" : s.pr?.verdict === "FAIL" ? "fail" : null,
    neutral: s.size === null,
  };
}

/** SKY-5 as debris: how many grains orbit a project with `n` drift items (capped). */
export const DEBRIS_MAX = 160;
export function debrisCount(n: number): number {
  if (!(n > 0)) return 0;
  return Math.min(DEBRIS_MAX, Math.round(10 + 14 * Math.sqrt(n)));
}

// ---- lenses: one question each, the same galaxy re-lit ----------------------------------------

export type Lens = "natural" | "progress" | "drift" | "flight" | "blocked" | "roadmap";
export const LENSES: { id: Lens; key: string; name: string; asks: string }[] = [
  { id: "natural", key: "n", name: "Natural", asks: "The sky as it is" },
  { id: "progress", key: "p", name: "Progress", asks: "How close is each project to 1.0?" },
  { id: "drift", key: "d", name: "Drift", asks: "Where is the roadmap out of step?" },
  { id: "flight", key: "f", name: "In flight", asks: "What is being worked on now?" },
  { id: "blocked", key: "b", name: "Blocked", asks: "What waits on what?" },
  { id: "roadmap", key: "r", name: "Roadmap", asks: "What comes next in this project's development stages?" },
];

export type SystemFacts = { condensed: number; drift: number; flying: number; blocked: number; charted: boolean };

export function systemFacts(c: Constellation): SystemFacts {
  const stars = c.features.flatMap((f) => f.stars);
  return {
    condensed: condensation(c),
    drift: c.drift.length,
    flying: stars.filter((s) => s.inFlight).length,
    blocked: c.edges.length,
    charted: c.features.length > 0,
  };
}

/** How brightly a project shows under a lens (1 = as natural). */
export function lensSystem(lens: Lens, f: SystemFacts): number {
  switch (lens) {
    case "progress":
      return f.charted ? 0.35 + 1.2 * f.condensed : 0.12;
    case "drift":
      return f.drift > 0 ? 0.5 + Math.min(1, f.drift / 60) : 0.1;
    case "flight":
      return f.flying > 0 ? 1.4 : 0.08;
    case "blocked":
      return f.blocked > 0 ? 0.5 + Math.min(1, f.blocked / 30) : 0.1;
    default:
      return 1;
  }
}

/** How many projects a lens lights up (the natural view lights them all). */
export function lensMatches(lens: Lens, facts: readonly SystemFacts[]): number {
  const hit = (f: SystemFacts) =>
    lens === "progress" ? f.charted : lens === "drift" ? f.drift > 0 : lens === "flight" ? f.flying > 0 : lens === "blocked" ? f.blocked > 0 : true;
  return facts.filter(hit).length;
}

/** How much of the drift debris shows under a lens. */
export function lensDebris(lens: Lens): number {
  return lens === "drift" ? 2.2 : lens === "natural" ? 1 : 0.15;
}

/** How bright the galaxy's own stars stay under a lens, so the answer stands out. */
export function lensDisc(lens: Lens): number {
  return lens === "natural" ? 1 : 0.3;
}

/** How brightly one issue star shows under a lens. */
export function lensStar(lens: Lens, s: { inFlight: boolean; state: "open" | "closed"; edged: boolean }): number {
  switch (lens) {
    case "flight":
      return s.inFlight ? 1.3 : 0.12;
    case "blocked":
      return s.edged ? 1.2 : 0.15;
    case "progress":
      return s.state === "closed" ? 1.15 : 0.3;
    case "drift":
      return 0.45;
    default:
      return 1;
  }
}
