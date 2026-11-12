import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Constellation, SkyModel, Star } from "./model";
import { layoutRoadmap } from "./roadmap";
import { RoadmapView } from "./RoadmapView";
import Sky3D from "../sky3d/Sky3D";

const { setLens } = vi.hoisted(() => ({ setLens: vi.fn() }));
vi.mock("../sky3d/SkyScene", () => ({
  SkyScene: class {
    resize() {} start() {} dispose() {} setSky() {} setOpen() {} setFilter() {}
    setHovered() {} setFocusStar() {} setPicked() {} invalidate() {}
    setLens = setLens;
  },
}));

const star = (number: number, state: "open" | "closed" = "open"): Star => ({
  number, title: `Issue ${number}`, state, url: `https://github.com/owner/demo/issues/${number}`,
  labels: [], size: null, milestone: null, inFlight: number === 3, pr: null, job: null,
});
const fixture = (): Constellation => ({
  repo: "owner/demo", url: "https://github.com/owner/demo", rank: 0, radius: 0, size: 1, angle: 0, brightness: .2,
  features: [
    { id: "core", title: "Core", release: "1.0", stars: [star(5, "closed"), star(1, "closed")] },
    { id: "ui", title: "Interface", release: "1.0", stars: [star(2), star(3)] },
    { id: "later", title: "Later", release: "after-1.0", stars: [star(9)] },
    { id: "dark", title: "Unfiled", release: "after-2.0", stars: [] },
  ],
  stages: [
    { number: 1, name: "Core first", features: ["core"], done: 2, total: 2, complete: true },
    { number: 2, name: "Faces", features: ["ui"], done: 0, total: 2, complete: false },
    { number: 3, name: "Design pass", features: [], done: 0, total: 0, complete: false },
  ],
  edges: [[1, 2], [5, 3], [3, 9]], chain: [1, 2], drift: [], reuses: [],
});

beforeEach(() => {
  setLens.mockClear();
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe("Roadmap columns", () => {
  it("uses the ported in-column order, keeps stage order, and leaves the input alone", () => {
    const c = fixture();
    const before = structuredClone(c);
    const at = layoutRoadmap(c);
    expect(at.columns.map((column) => column.ids)).toEqual([[5, 1], [3, 2], [], [9]]);
    expect(at.positions.get(1)!.x).toBeLessThan(at.positions.get(2)!.x);
    expect(c).toEqual(before);
  });

  it("heads each stage with its number, name, counts and completion, with Unstaged last", () => {
    render(<RoadmapView c={fixture()} picked={null} onPick={vi.fn()} />);
    const columns = screen.getAllByTestId("roadmap-column");
    expect(columns.map((c) => within(c).getByRole("heading").textContent)).toEqual(["1. Core first", "2. Faces", "3. Design pass", "Unstaged"]);
    expect(within(columns[0]).getByText("2 / 2 issues · Complete")).toBeTruthy();
    expect(columns[1].hasAttribute("data-complete")).toBe(false);
    expect(within(columns[2]).getByText("0 / 0 issues")).toBeTruthy();
    expect(within(columns[3]).getByRole("button", { name: "#9 Issue 9" })).toBeTruthy();
    expect(within(columns[3]).getByText("Unfiled · no issues")).toBeTruthy();
  });

  it("draws links across stages and into Unstaged at the star coordinates", () => {
    render(<RoadmapView c={fixture()} picked={null} onPick={vi.fn()} />);
    const edges = screen.getAllByTestId("blocked-by");
    expect(edges).toHaveLength(3);
    expect(edges.every((e) => Number(e.getAttribute("x1")) < Number(e.getAttribute("x2")))).toBe(true);
  });

  it("keeps stars selectable, with their state and in-flight marker", () => {
    const onPick = vi.fn();
    render(<RoadmapView c={fixture()} picked={null} onPick={onPick} />);
    const button = screen.getByRole("button", { name: "#3 Issue 3" });
    expect(button.getAttribute("data-pulse")).toBe("true");
    fireEvent.click(button);
    expect(onPick.mock.calls[0][0].number).toBe(3);
  });

  it("does not duplicate a star on two features or add Unstaged when everything is staged", () => {
    const c = fixture();
    c.features = c.features.slice(0, 2);
    c.features[1].stars.push(c.features[0].stars[0]);
    render(<RoadmapView c={c} picked={null} onPick={vi.fn()} />);
    expect(screen.getAllByTestId("star")).toHaveLength(4);
    expect(screen.queryByText("Unstaged")).toBeNull();
  });

  it("says plainly when stages are missing, including older snapshots", () => {
    const c = fixture();
    delete c.stages;
    render(<RoadmapView c={c} picked={null} onPick={vi.fn()} />);
    expect(screen.getByRole("status").textContent).toBe("ROADMAP.md has no Development stages table.");
    expect(screen.queryAllByTestId("star")).toHaveLength(0);
  });
});

describe("project lens switcher", () => {
  const props = () => {
    const c = fixture();
    const sky: SkyModel = { ageMinutes: 0, priorityAgeDays: null, constellations: [c] };
    return { sky, open: c.repo, onOpen: vi.fn(), onBack: vi.fn(), picked: null, onPick: vi.fn(), toolbar: null, onFail: vi.fn() };
  };

  it("selects Roadmap beside existing lenses and restores another lens's sky", () => {
    render(<Sky3D {...props()} />);
    const lenses = screen.getByRole("group", { name: "Lens" });
    expect(within(lenses).getAllByRole("button").map((b) => b.textContent)).toEqual(["Natural", "Progress", "Drift", "In flight", "Blocked", "Roadmap"]);
    fireEvent.click(within(lenses).getByRole("button", { name: "Roadmap" }));
    expect(screen.getAllByTestId("roadmap-column")).toHaveLength(4);
    expect(within(lenses).getByRole("button", { name: "Roadmap" }).getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(within(lenses).getByRole("button", { name: "Blocked" }));
    expect(screen.queryAllByTestId("roadmap-column")).toHaveLength(0);
    expect(screen.getAllByTestId("ring")).toHaveLength(3);
    expect(setLens).toHaveBeenLastCalledWith("blocked");
  });

  it("supports r to toggle Roadmap and keeps it project-only", () => {
    const p = props();
    const { rerender } = render(<Sky3D {...p} />);
    fireEvent.keyDown(window, { key: "r" });
    expect(screen.getAllByTestId("roadmap-column")).toHaveLength(4);
    fireEvent.keyDown(window, { key: "r" });
    expect(screen.queryAllByTestId("roadmap-column")).toHaveLength(0);
    rerender(<Sky3D {...p} open={null} />);
    expect(screen.queryByRole("button", { name: "Roadmap" })).toBeNull();
  });
});
