// Which labels the 3D sky shows, and where, so none overlaps another or leaves the view.
// At the overview only project names show (the top few by rank, plus the one under the mouse);
// inside a galaxy only feature names (truncated), and a star's title only on hover or selection.

export type Rect = { left: number; top: number; width: number; height: number };

export type LabelCandidate = {
  id: string;
  /** Where the label wants to sit: its centre x and its top y, in view pixels. */
  x: number;
  y: number;
  width: number;
  height: number;
  /** Lower wins a collision. */
  priority: number;
  /** Placed before everything else (the hovered or picked object). */
  forced?: boolean;
};

export type Placed = Map<string, { left: number; top: number }>;

const PAD = 4;

export function overlaps(a: Rect, b: Rect, pad = PAD): boolean {
  return a.left < b.left + b.width + pad && b.left < a.left + a.width + pad && a.top < b.top + b.height + pad && b.top < a.top + a.height + pad;
}

/**
 * Greedy placement: forced labels first, then by priority. A label is nudged inside the view
 * (never clipped) and dropped if it would overlap a placed label or a blocker (a panel, the
 * controls). A forced label ignores blockers but is still kept inside the view.
 */
export function placeLabels(cands: LabelCandidate[], view: { width: number; height: number }, blockers: Rect[] = [], margin = 6): Placed {
  const order = [...cands].sort((a, b) => Number(!!b.forced) - Number(!!a.forced) || a.priority - b.priority);
  const placed: Placed = new Map();
  const taken: Rect[] = [];
  for (const c of order) {
    if (!Number.isFinite(c.x) || !Number.isFinite(c.y)) continue;
    if (c.width > view.width - 2 * margin || c.height > view.height - 2 * margin) continue;
    const left = clamp(c.x - c.width / 2, margin, view.width - margin - c.width);
    const top = clamp(c.y, margin, view.height - margin - c.height);
    // A label far from its object (its object is off-screen) is not worth showing.
    if (Math.abs(left - (c.x - c.width / 2)) > c.width || Math.abs(top - c.y) > 2 * c.height) continue;
    const rect = { left, top, width: c.width, height: c.height };
    if (taken.some((t) => overlaps(rect, t))) continue;
    if (!c.forced && blockers.some((b) => overlaps(rect, b, 2))) continue;
    placed.set(c.id, { left, top });
    taken.push(rect);
  }
  return placed;
}

/** The overview's labelled projects: the top `n` by rank, plus the hovered and focused ones. */
export function overviewLabelRepos(cs: { repo: string; rank: number }[], n: number, extra: (string | null)[] = []): Set<string> {
  const out = new Set([...cs].sort((a, b) => a.rank - b.rank).slice(0, n).map((c) => c.repo));
  for (const e of extra) if (e) out.add(e);
  return out;
}

/**
 * A label's centre x on the outer side of its object: right of it when the object is right of
 * the hub (or level with it), left of it otherwise, `gap` pixels clear of its radius.
 */
export function outsideX(obj: { x: number; r: number }, hub: { x: number }, width: number, gap: number): number {
  return obj.x >= hub.x ? obj.x + obj.r + gap + width / 2 : obj.x - obj.r - gap - width / 2;
}

/** A long title cut at a word boundary with an ellipsis; short titles unchanged. */
export function truncate(title: string, max: number): string {
  if (title.length <= max) return title;
  const cut = title.slice(0, max - 1);
  const space = cut.lastIndexOf(" ");
  return `${(space > max * 0.6 ? cut.slice(0, space) : cut).replace(/[\s,.:;–-]+$/, "")}…`;
}

/** The project name shown on the sky: the part after the owner. */
export function shortName(repo: string): string {
  return repo.slice(repo.lastIndexOf("/") + 1);
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(Math.max(v, lo), Math.max(lo, hi));
}
