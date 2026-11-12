// #455: dependency layers, in-column order and downstream impact (ported from obsidian-wayfinder).
import { describe, expect, it } from "vitest";
import { dependencyLayers, downstreamImpact, orderWithinLayers, type Edge } from "./layers";

function deepFreeze<T>(o: T): T {
  if (o && typeof o === "object" && !Object.isFrozen(o)) {
    Object.freeze(o);
    for (const v of Object.values(o as object)) deepFreeze(v);
  }
  return o;
}

const layerOf = (ids: number[], edges: Edge[]) =>
  Object.fromEntries(dependencyLayers(deepFreeze(ids), deepFreeze(edges)).layer);

describe("dependencyLayers", () => {
  it("puts a chain one layer per link", () => {
    expect(layerOf([3, 2, 1], [[1, 2], [2, 3]])).toEqual({ 1: 0, 2: 1, 3: 2 });
  });

  it("puts a diamond's join one past its deepest blocker", () => {
    // 1 -> 2 -> 4, 1 -> 3, 3 -> 4, and 4 also waits on 5 with no blocker.
    const { layer, columns } = dependencyLayers(
      deepFreeze([1, 2, 3, 4, 5]),
      deepFreeze<Edge[]>([[1, 2], [1, 3], [2, 4], [3, 4], [5, 4]]),
    );
    expect(Object.fromEntries(layer)).toEqual({ 1: 0, 2: 1, 3: 1, 4: 2, 5: 0 });
    expect(columns).toEqual([[1, 5], [2, 3], [4]]);
  });

  it("gives the members of a cycle one shared layer and ends", () => {
    // 1 blocks 2, 2 and 3 block each other, 3 blocks 4.
    expect(layerOf([1, 2, 3, 4], [[1, 2], [2, 3], [3, 2], [3, 4]])).toEqual({ 1: 0, 2: 1, 3: 1, 4: 2 });
  });

  it("ignores edges to ids outside the set and self edges", () => {
    expect(layerOf([1, 2], [[9, 1], [1, 2], [2, 2], [2, 9]])).toEqual({ 1: 0, 2: 1 });
  });

  it("returns empty for no ids", () => {
    expect(dependencyLayers([], [])).toEqual({ layer: new Map(), columns: [] });
  });
});

describe("orderWithinLayers", () => {
  it("keeps column 0 and sorts later columns by their blockers' mean position", () => {
    // Column 0 is [5, 1]: 5 at 0, 1 at 1. 2 waits on 1 (1.0), 3 on 5 (0.0), 4 on both (0.5).
    const out = orderWithinLayers(
      deepFreeze([[5, 1], [2, 4, 3]]),
      deepFreeze<Edge[]>([[1, 2], [5, 3], [1, 4], [5, 4]]),
    );
    expect(out).toEqual([[5, 1], [3, 4, 2]]);
  });

  it("sorts an id with no placed blocker by its number, ties by the lower number", () => {
    // 7 and 8 both wait on 1 (key 0); 2 has no blocker placed, so its key is 2.
    const out = orderWithinLayers(deepFreeze([[1], [8, 2, 7]]), deepFreeze<Edge[]>([[1, 8], [1, 7]]));
    expect(out).toEqual([[1], [7, 8, 2]]);
  });
});

describe("downstreamImpact", () => {
  it("counts distinct open dependents, direct or not", () => {
    // 1 -> 2 -> 4, 1 -> 3 -> 4: 4 counted once for 1.
    const out = downstreamImpact(deepFreeze([1, 2, 3, 4]), deepFreeze<Edge[]>([[1, 2], [1, 3], [2, 4], [3, 4]]));
    expect(Object.fromEntries(out)).toEqual({ 1: 3, 2: 1, 3: 1, 4: 0 });
  });

  it("leaves out closed ids and ends on a cycle", () => {
    // 5 is closed, so 1 -> 5 -> 6 does not reach 6; 2 and 3 block each other.
    const out = downstreamImpact(
      deepFreeze([1, 2, 3, 6]),
      deepFreeze<Edge[]>([[1, 5], [5, 6], [1, 2], [2, 3], [3, 2]]),
    );
    expect(Object.fromEntries(out)).toEqual({ 1: 2, 2: 1, 3: 1, 6: 0 });
  });
});
