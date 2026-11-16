import type { Constellation, Feature, RoadmapStage, Star } from "./model";
import { orderWithinLayers } from "./layers";

export const COLUMN_WIDTH = 300;
export const ROW_HEIGHT = 66;
export const HEADER_HEIGHT = 100;

/** One Roadmap column: a dependency layer, a development stage, or Unstaged. */
export type RoadmapColumn = {
  /** #413: the layer number, for a dependency-layer column. */
  layer?: number;
  stage?: RoadmapStage;
  /** Issue numbers, in the order inside the column. */
  ids: number[];
  /** Features with no issue, drawn as dark stars (stage and Unstaged columns only). */
  dark: Feature[];
  x: number;
};

/**
 * The Roadmap's columns: one per dependency layer when the core sends `nodeLayers` (#413), then
 * one per development stage, then Unstaged when a feature not in any stage holds a star or a dark
 * star. A star joins the column of its `nodeLayer`; a star without one goes in its feature's stage
 * column (Unstaged when no stage holds it), so no star disappears. The ported barycenter pass
 * orders each column.
 */
export function layoutRoadmap(c: Constellation) {
  const stages = c.stages ?? [];
  const layers = c.nodeLayers ?? 0;
  const unstagedGroup = stages.length;
  const layerIds: number[][] = Array.from({ length: layers }, () => []);
  const groupIds: number[][] = Array.from({ length: unstagedGroup + 1 }, () => []);
  const groupDark: Feature[][] = Array.from({ length: unstagedGroup + 1 }, () => []);
  const stars = new Map<number, Star>();
  const featureOf = new Map<number, Feature>();
  // Stage order, Unstaged last, so an orphan star (on several features) counts in its first stage.
  const byGroup = c.features
    .map((feature) => {
      const index = stages.findIndex((stage) => stage.features.includes(feature.id));
      return { feature, group: index < 0 ? unstagedGroup : index };
    })
    .sort((a, b) => a.group - b.group);
  for (const { feature, group } of byGroup) {
    if (!feature.stars.length) groupDark[group].push(feature);
    for (const star of feature.stars) {
      if (stars.has(star.number)) continue;
      stars.set(star.number, star);
      featureOf.set(star.number, feature);
      const layer = star.nodeLayer;
      if (layer !== undefined && Number.isInteger(layer) && layer >= 0 && layer < layers) layerIds[layer].push(star.number);
      else groupIds[group].push(star.number);
    }
  }
  const unstaged = groupIds[unstagedGroup].length || groupDark[unstagedGroup].length;
  const columns: Omit<RoadmapColumn, "x">[] = [
    ...layerIds.map((ids, layer) => ({ layer, ids, dark: [] })),
    ...stages.map((stage, index) => ({ stage, ids: groupIds[index], dark: groupDark[index] })),
    ...(unstaged ? [{ ids: groupIds[unstagedGroup], dark: groupDark[unstagedGroup] }] : []),
  ];
  const ordered = orderWithinLayers(columns.map((column) => column.ids), c.edges);
  const positions = new Map<number, { x: number; y: number }>();
  const laid: RoadmapColumn[] = columns.map((column, index) => {
    const x = index * COLUMN_WIDTH;
    ordered[index].forEach((id, row) => positions.set(id, { x: x + 28, y: HEADER_HEIGHT + row * ROW_HEIGHT }));
    return { ...column, ids: ordered[index], x };
  });
  return {
    columns: laid, stars, featureOf, positions,
    width: Math.max(1, laid.length) * COLUMN_WIDTH,
    height: HEADER_HEIGHT + Math.max(1, ...laid.map((column) => column.ids.length + column.dark.length)) * ROW_HEIGHT,
  };
}
