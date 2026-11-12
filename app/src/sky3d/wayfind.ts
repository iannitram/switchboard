// Finding the way (#297, pass 2): constellation figures, the breadcrumb, and jump search.
import type { Constellation, SkyModel, Star } from "../sky/model";
import type { Vec3 } from "./layout";
import { shortName } from "./labels";

/**
 * A feature's figure: the minimum spanning tree of its stars (Prim), so each feature gets its
 * own recognisable shape with the fewest, shortest lines. Returns index pairs.
 */
export function spanningTree(points: readonly Vec3[]): [number, number][] {
  const n = points.length;
  if (n < 2) return [];
  const inTree = new Array<boolean>(n).fill(false);
  const best = new Array<number>(n).fill(Infinity);
  const from = new Array<number>(n).fill(-1);
  const d2 = (a: Vec3, b: Vec3) => (a.x - b.x) ** 2 + (a.y - b.y) ** 2 + (a.z - b.z) ** 2;
  best[0] = 0;
  const out: [number, number][] = [];
  for (let k = 0; k < n; k++) {
    let u = -1;
    for (let i = 0; i < n; i++) if (!inTree[i] && (u === -1 || best[i] < best[u])) u = i;
    inTree[u] = true;
    if (from[u] >= 0) out.push([from[u], u]);
    for (let v = 0; v < n; v++) {
      if (inTree[v]) continue;
      const d = d2(points[u], points[v]);
      if (d < best[v]) {
        best[v] = d;
        from[v] = u;
      }
    }
  }
  return out;
}

export type Crumb = { level: "galaxy" | "project" | "feature" | "issue"; label: string; title: string };

/** Galaxy › project › feature › #issue: as deep as the view is. */
export function breadcrumbs(sky: SkyModel, open: string | null, feature: string | null, picked: Star | null): Crumb[] {
  const out: Crumb[] = [{ level: "galaxy", label: "Galaxy", title: "The whole sky" }];
  const c = open === null ? undefined : sky.constellations.find((x) => x.repo === open);
  if (!c) return out;
  out.push({ level: "project", label: shortName(c.repo), title: c.repo });
  const f = (picked && c.features.find((x) => x.stars.some((s) => s.number === picked.number))) || c.features.find((x) => x.id === feature);
  if (f) out.push({ level: "feature", label: f.title, title: f.title });
  if (picked) out.push({ level: "issue", label: `#${picked.number}`, title: picked.title });
  return out;
}

export type Hit =
  | { kind: "project"; repo: string; label: string; sub: string }
  | { kind: "feature"; repo: string; feature: string; label: string; sub: string }
  | { kind: "issue"; repo: string; feature: string; star: Star; label: string; sub: string };

/**
 * Jump search: projects by name, features by title, issues by number (`#12` or `12`) or title.
 * Prefix matches first, then projects before features before issues; at most `limit`.
 */
export function searchSky(sky: SkyModel, query: string, limit = 8): Hit[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const num = /^#?(\d+)$/.exec(q);
  const scored: { hit: Hit; score: number }[] = [];
  const score = (text: string, kind: number) => {
    const t = text.toLowerCase();
    const i = t.indexOf(q);
    if (i < 0) return -1;
    return (i === 0 ? 0 : /\W/.test(t[i - 1] ?? "") ? 1 : 2) * 10 + kind;
  };
  for (const c of sky.constellations as Constellation[]) {
    const name = shortName(c.repo);
    const s = Math.min(...[score(name, 0), score(c.repo, 0)].map((v) => (v < 0 ? Infinity : v)));
    if (Number.isFinite(s)) scored.push({ hit: { kind: "project", repo: c.repo, label: name, sub: c.repo }, score: s });
    for (const f of c.features) {
      const fs = score(f.title, 1);
      if (fs >= 0) scored.push({ hit: { kind: "feature", repo: c.repo, feature: f.id, label: f.title, sub: name }, score: fs });
      for (const st of f.stars) {
        let ss = -1;
        if (num) ss = st.number === Number(num[1]) ? 0 : -1;
        else ss = score(st.title, 2);
        if (ss >= 0) scored.push({ hit: { kind: "issue", repo: c.repo, feature: f.id, star: st, label: `#${st.number} ${st.title}`, sub: `${name} · ${f.title}` }, score: ss });
      }
    }
  }
  return scored
    .sort((a, b) => a.score - b.score || a.hit.label.localeCompare(b.hit.label))
    .slice(0, limit)
    .map((s) => s.hit);
}
