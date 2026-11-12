import type { Constellation, Feature } from "./model";
import { orderWithinLayers } from "./layers";

export const COLUMN_WIDTH = 300;
export const ROW_HEIGHT = 66;
export const HEADER_HEIGHT = 100;

/** Stage membership comes from the core; the ported barycenter pass orders each column. */
export function layoutRoadmap(c: Constellation) {
  const stages = c.stages ?? [];
  const groups: Feature[][] = stages.map(() => []);
  const unstaged: Feature[] = [];
  for (const feature of c.features) {
    const index = stages.findIndex((stage) => stage.features.includes(feature.id));
    if (index < 0) unstaged.push(feature);
    else groups[index].push(feature);
  }
  if (unstaged.length) groups.push(unstaged);
  // An orphan can be present in several features. Draw its issue only once.
  const seen = new Set<number>();
  const issues = groups.map((features) => features.flatMap((f) => f.stars).filter((s) => {
    if (seen.has(s.number)) return false;
    seen.add(s.number);
    return true;
  }));
  const ordered = orderWithinLayers(issues.map((stars) => stars.map((s) => s.number)), c.edges);
  const stars = new Map(issues.flat().map((star) => [star.number, star]));
  const positions = new Map<number, { x: number; y: number }>();
  const columns = groups.map((features, index) => {
    const stage = stages[index];
    const ids = ordered[index];
    ids.forEach((id, row) => positions.set(id, { x: index * COLUMN_WIDTH + 28, y: HEADER_HEIGHT + row * ROW_HEIGHT }));
    return { stage, features, ids, x: index * COLUMN_WIDTH };
  });
  return {
    columns, stars, positions,
    width: Math.max(1, columns.length) * COLUMN_WIDTH,
    height: HEADER_HEIGHT + Math.max(1, ...groups.map((fs, i) => issues[i].length + fs.filter((f) => !f.stars.length).length)) * ROW_HEIGHT,
  };
}
