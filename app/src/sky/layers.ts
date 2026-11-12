// SPDX-License-Identifier: MIT
// Ported from obsidian-wayfinder, https://github.com/ocuclaw/obsidian-wayfinder
// (src/model.ts at 57b59d02722b5ba32a57eca649659b17f6156e05: assignLayers, orderWithinLayers,
// assignDownstreamImpact).
// Changed: taken out of their Ticket type onto issue numbers and `Constellation.edges`
// ([blocker, blocked]); each function returns a new value instead of writing onto tickets;
// `dependencyLayers` also returns the layers as columns and reads the component layers in
// Tarjan's emission order instead of a memoised recursion.
//
// MIT License
//
// Copyright (c) 2026 ocuclaw
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

// #455: the graph pieces the Roadmap lens lays out with (#392's stage columns, #413's layer
// columns). Pure functions over issue numbers; edges are [blocker, blocked].

export type Edge = readonly [number, number];

/** The blockers of each id, among `ids` only. */
function blockersOf(ids: readonly number[], edges: readonly Edge[]): Map<number, number[]> {
  const inSet = new Set(ids);
  const out = new Map<number, number[]>(ids.map((id) => [id, []]));
  for (const [blocker, blocked] of edges) {
    if (blocker === blocked || !inSet.has(blocker) || !inSet.has(blocked)) continue;
    const list = out.get(blocked)!;
    if (!list.includes(blocker)) list.push(blocker);
  }
  return out;
}

/**
 * Longest-path layering: an id with no blocker among `ids` is layer 0, any other is one more
 * than its deepest blocker. The members of a cycle (a strongly connected component, found with
 * Tarjan's algorithm) share one layer. Edges to ids outside `ids` are ignored. `columns[n]` lists
 * layer n's ids in the order they appear in `ids`.
 */
export function dependencyLayers(
  ids: readonly number[],
  edges: readonly Edge[],
): { layer: Map<number, number>; columns: number[][] } {
  const blockers = blockersOf(ids, edges);
  const index = new Map<number, number>();
  const low = new Map<number, number>();
  const stack: number[] = [];
  const onStack = new Set<number>();
  const componentOf = new Map<number, number>();
  const components: number[][] = [];
  let next = 0;

  const visit = (id: number): void => {
    index.set(id, next);
    low.set(id, next);
    next++;
    stack.push(id);
    onStack.add(id);
    for (const b of blockers.get(id)!) {
      if (!index.has(b)) {
        visit(b);
        low.set(id, Math.min(low.get(id)!, low.get(b)!));
      } else if (onStack.has(b)) {
        low.set(id, Math.min(low.get(id)!, index.get(b)!));
      }
    }
    if (low.get(id) !== index.get(id)) return;
    const component: number[] = [];
    for (let member = stack.pop(); member !== undefined; member = stack.pop()) {
      onStack.delete(member);
      componentOf.set(member, components.length);
      component.push(member);
      if (member === id) break;
    }
    components.push(component);
  };
  for (const id of blockers.keys()) if (!index.has(id)) visit(id);

  // Tarjan emits a component only after every component it depends on, so one pass in
  // emission order sees each blocker's layer before it is needed.
  const componentLayer: number[] = [];
  components.forEach((members, c) => {
    let layer = 0;
    for (const id of members) {
      for (const b of blockers.get(id)!) {
        const bc = componentOf.get(b)!;
        if (bc !== c) layer = Math.max(layer, componentLayer[bc] + 1);
      }
    }
    componentLayer[c] = layer;
  });

  const layer = new Map<number, number>();
  const columns: number[][] = [];
  for (const id of blockers.keys()) {
    const l = componentLayer[componentOf.get(id)!];
    layer.set(id, l);
    (columns[l] ??= []).push(id);
  }
  return { layer, columns };
}

/**
 * The order inside each column: column 0 keeps its given order; each later column is sorted by
 * the mean position of each id's blockers in the columns already ordered (a barycenter pass, so
 * blocked-by lines cross less). An id with no blocker placed so far sorts by its own number, and
 * ties go to the lower number. Returns new arrays.
 */
export function orderWithinLayers(columns: readonly (readonly number[])[], edges: readonly Edge[]): number[][] {
  const blockers = blockersOf(columns.flat(), edges);
  const pos = new Map<number, number>();
  return columns.map((column, c) => {
    let ordered = [...column];
    if (c > 0) {
      const key = (id: number): number => {
        const ps = blockers
          .get(id)!
          .map((b) => pos.get(b))
          .filter((p): p is number => p !== undefined);
        return ps.length ? ps.reduce((a, b) => a + b, 0) / ps.length : id;
      };
      const keys = new Map(ordered.map((id) => [id, key(id)]));
      ordered = ordered.sort((a, b) => keys.get(a)! - keys.get(b)! || a - b);
    }
    ordered.forEach((id, i) => pos.set(id, i));
    return ordered;
  });
}

/**
 * For each open id, the number of distinct open ids that wait on it, directly or through
 * others. Only edges between open ids count; a cycle ends because each walk keeps a seen set.
 */
export function downstreamImpact(openIds: readonly number[], edges: readonly Edge[]): Map<number, number> {
  const open = new Set(openIds);
  const dependents = new Map<number, number[]>();
  for (const [blocker, blocked] of edges) {
    if (blocker === blocked || !open.has(blocker) || !open.has(blocked)) continue;
    const list = dependents.get(blocker) ?? [];
    if (!list.includes(blocked)) list.push(blocked);
    dependents.set(blocker, list);
  }
  const out = new Map<number, number>();
  for (const id of open) {
    const seen = new Set<number>([id]);
    const queue = [...(dependents.get(id) ?? [])];
    for (let at = queue.shift(); at !== undefined; at = queue.shift()) {
      if (seen.has(at)) continue;
      seen.add(at);
      queue.push(...(dependents.get(at) ?? []));
    }
    out.set(id, seen.size - 1);
  }
  return out;
}
