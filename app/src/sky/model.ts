// The sky as the core works it out (SPEC.md, feature `sky`). The face paints this and holds no
// rules: placement, brightness, drift, reuse warnings and the chain all come from
// `switchboard-core` (crates/core/src/sky/) through the `sky` command.

export type Release = "1.0" | "after-1.0" | "after-2.0";

/** One GitHub issue. */
export type Star = {
  number: number;
  title: string;
  state: "open" | "closed";
  url: string;
  labels: string[];
  /** From the `size:` label: "easy", "medium" or "hard"; null when it has none. */
  size: string | null;
  milestone: string | null;
  /** SKY-4: a fleet job for this issue is in flight. */
  inFlight: boolean;
  /** SKY-8: the PR that closes it, with its latest R25 verdict. */
  pr: { number: number; url: string; state: string; verdict: "PASS" | "FAIL" | null } | null;
  /** SKY-8: the fleet job for it, as the job log names it; null when none. */
  job: string | null;
};

/** One roadmap feature: a cluster of stars. No stars is a dark star. */
export type Feature = { id: string; title: string; release: Release; stars: Star[] };

/** SKY-5: one drift item; `link` goes to the issue or to ROADMAP.md. */
export type DriftItem = { kind: "orphan" | "dark" | "stale"; text: string; link: string };

/** SKY-6: one `reuses:` line of the code map; `warning` when that repo is Parked or archived. */
export type Reuse = { repo: string; what: string; warning: boolean };

/** One project on the sky. */
export type Constellation = {
  /** owner/name */
  repo: string;
  url: string;
  /** SKY-1: 0-based rank, radius 0 (centre) to 1 (rim), size 0 to 1, angle in radians. */
  rank: number;
  radius: number;
  size: number;
  angle: number;
  /** SKY-2: progress to 1.0, 0 to 1. */
  brightness: number;
  features: Feature[];
  /** SKY-3: [blocker, blocked] issue numbers, one per `Blocked by #N`. */
  edges: [number, number][];
  /** SKY-7: the longest chain, first blocker to last blocked; empty when none. */
  chain: number[];
  drift: DriftItem[];
  reuses: Reuse[];
};

export type SkyModel = {
  /** The snapshot's age in whole minutes (GH-1). */
  ageMinutes: number;
  /** SKY-9: the priority copy's age in whole days; null when there is no copy. */
  priorityAgeDays: number | null;
  /** In priority order. */
  constellations: Constellation[];
};
