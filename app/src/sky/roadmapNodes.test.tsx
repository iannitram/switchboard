import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Constellation, Star } from "./model";
import { layoutRoadmap } from "./roadmap";
import { RoadmapView } from "./RoadmapView";

const star = (number: number, nodeLayer?: number): Star => ({
  number, title: `Issue ${number}`, state: "open", url: `https://github.com/owner/demo/issues/${number}`,
  labels: [], size: null, milestone: null, inFlight: false, pr: null, job: null,
  ...(nodeLayer === undefined ? {} : { nodeLayer }),
});

// Layer 0: #1. Layer 1: #2 and #3. Layer 2 is empty. #4 and #5 have no node (or an unknown one),
// so they fall back to the Faces stage; #6 is in no stage, so it is Unstaged. #4 is also on a
// second feature, an orphan: it must still be drawn once.
const fixture = (): Constellation => {
  const orphan = star(4);
  return {
    repo: "owner/demo", url: "https://github.com/owner/demo", rank: 0, radius: 0, size: 1, angle: 0, brightness: .2,
    features: [
      { id: "core", title: "Core", release: "1.0", stars: [star(1, 0), star(2, 1)] },
      { id: "ui", title: "Interface", release: "1.0", stars: [star(3, 1), orphan, star(5)] },
      { id: "later", title: "Later", release: "after-1.0", stars: [star(6), orphan] },
      { id: "dark", title: "Unfiled", release: "after-2.0", stars: [] },
    ],
    stages: [
      { number: 1, name: "Core first", features: ["core"], done: 0, total: 2, complete: false },
      { number: 2, name: "Faces", features: ["ui"], done: 0, total: 3, complete: false },
    ],
    nodeLayers: 3,
    edges: [[1, 3], [2, 3]], chain: [], drift: [], reuses: [],
  };
};

const headings = () => screen.getAllByTestId("roadmap-column").map((column) => within(column).getByRole("heading").textContent);

beforeEach(() => vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} }));
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe("Roadmap columns with a nodes: block", () => {
  it("draws one column per dependency layer, headed by its number, then the stages, then Unstaged", () => {
    render(<RoadmapView c={fixture()} picked={null} onPick={vi.fn()} />);
    expect(headings()).toEqual(["Layer 0", "Layer 1", "Layer 2", "1. Core first", "2. Faces", "Unstaged"]);
  });

  it("puts a star in the column of the layer its node names", () => {
    render(<RoadmapView c={fixture()} picked={null} onPick={vi.fn()} />);
    const columns = screen.getAllByTestId("roadmap-column");
    expect(columns[0].getAttribute("data-layer")).toBe("0");
    expect(within(columns[0]).getByRole("button", { name: "#1 Issue 1" })).toBeTruthy();
    expect(within(columns[1]).getByRole("button", { name: "#2 Issue 2" })).toBeTruthy();
    expect(within(columns[1]).getByRole("button", { name: "#3 Issue 3" })).toBeTruthy();
    expect(within(columns[2]).queryAllByTestId("star")).toHaveLength(0);
  });

  it("places a star without a node, or with an unknown one, in its stage column after the layers", () => {
    render(<RoadmapView c={fixture()} picked={null} onPick={vi.fn()} />);
    const faces = screen.getAllByTestId("roadmap-column")[4];
    expect(within(faces).getByRole("button", { name: "#4 Issue 4" })).toBeTruthy();
    expect(within(faces).getByRole("button", { name: "#5 Issue 5" })).toBeTruthy();
    expect(within(screen.getAllByTestId("roadmap-column")[5]).getByRole("button", { name: "#6 Issue 6" })).toBeTruthy();
  });

  it("counts the stars drawn against the stars given, each once", () => {
    const c = fixture();
    const given = new Set(c.features.flatMap((feature) => feature.stars.map((s) => s.number)));
    render(<RoadmapView c={c} picked={null} onPick={vi.fn()} />);
    expect(screen.getAllByTestId("star")).toHaveLength(given.size);
    expect(given.size).toBe(6);
  });

  it("draws the blocked-by lines between layers", () => {
    render(<RoadmapView c={fixture()} picked={null} onPick={vi.fn()} />);
    expect(screen.getAllByTestId("blocked-by")).toHaveLength(2);
  });

  it("still draws the layers when there is no stages table, with stars no stage holds in Unstaged", () => {
    const c = fixture();
    delete c.stages;
    render(<RoadmapView c={c} picked={null} onPick={vi.fn()} />);
    expect(headings()).toEqual(["Layer 0", "Layer 1", "Layer 2", "Unstaged"]);
    expect(screen.getAllByTestId("star")).toHaveLength(6);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("lays out star positions for the links from the layer columns", () => {
    const at = layoutRoadmap(fixture());
    expect(at.columns[0].ids).toEqual([1]);
    expect([...at.columns[1].ids].sort()).toEqual([2, 3]);
    expect(at.positions.get(1)!.x).toBeLessThan(at.positions.get(3)!.x);
  });
});

describe("Roadmap columns without a usable nodes: block", () => {
  it("keeps the stages layout when nodeLayers is absent, ignoring any node layer on a star", () => {
    const c = fixture();
    delete c.nodeLayers;
    render(<RoadmapView c={c} picked={null} onPick={vi.fn()} />);
    expect(headings()).toEqual(["1. Core first", "2. Faces", "Unstaged"]);
    expect(within(screen.getAllByTestId("roadmap-column")[0]).getByRole("button", { name: "#1 Issue 1" })).toBeTruthy();
    expect(screen.getAllByTestId("star")).toHaveLength(6);
  });

  it("says plainly when there are neither a nodes: block nor a stages table", () => {
    const c = fixture();
    delete c.nodeLayers;
    delete c.stages;
    render(<RoadmapView c={c} picked={null} onPick={vi.fn()} />);
    expect(screen.getByRole("status").textContent).toBe("ROADMAP.md has no Development stages table.");
  });
});
