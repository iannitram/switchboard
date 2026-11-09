// SKY-7: the stars a focused star waits on, and the stars waiting on it. Edges are
// [blocker, blocked], as in `Constellation.edges`; each walk keeps a seen set, so a cycle ends.
export function highlightChain(edges: readonly (readonly [number, number])[], focus: number): Set<number> {
  const out = new Set<number>([focus]);
  const walk = (pick: (e: readonly [number, number], at: number) => number | null) => {
    const seen = new Set<number>([focus]);
    const queue = [focus];
    while (queue.length > 0) {
      const at = queue.pop()!;
      for (const e of edges) {
        const next = pick(e, at);
        if (next === null || seen.has(next)) continue;
        seen.add(next);
        out.add(next);
        queue.push(next);
      }
    }
  };
  walk((e, at) => (e[1] === at ? e[0] : null)); // up: to what it waits on
  walk((e, at) => (e[0] === at ? e[1] : null)); // down: to what waits on it
  return out;
}
