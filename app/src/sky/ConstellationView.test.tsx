// Locked tests for #38 (SKY-3: zoom into one constellation). Written before the build (R28);
// the builder never edits this file.
import { fireEvent, render, screen } from "@testing-library/react";
import type { Constellation, SkyModel, Star } from "./model";
import { RINGS, layoutConstellation, onRing } from "./geometry";
import { ConstellationView } from "./ConstellationView";
import { Sky } from "../views/Sky";

const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("../core", () => ({ call }));

function deepFreeze<T>(o: T): T {
  if (o && typeof o === "object" && !Object.isFrozen(o)) {
    Object.freeze(o);
    for (const v of Object.values(o as object)) deepFreeze(v);
  }
  return o;
}

function star(number: number, state: "open" | "closed" = "open"): Star {
  return {
    number,
    title: `issue ${number}`,
    state,
    url: `https://github.com/iannitram/switchboard/issues/${number}`,
    labels: [],
    size: null,
    milestone: "1.0",
    inFlight: false,
    pr: null,
    job: null,
  };
}

const C: Constellation = deepFreeze({
  repo: "iannitram/switchboard",
  url: "https://github.com/iannitram/switchboard",
  rank: 0,
  radius: 0,
  size: 1,
  angle: 0,
  brightness: 0.25,
  features: [
    { id: "core", title: "The core", release: "1.0", stars: [star(1, "closed"), star(2)] },
    { id: "sky", title: "Constellation", release: "1.0", stars: [star(3)] },
    { id: "budget", title: "The budget panel", release: "1.0", stars: [] },
    { id: "relay", title: "A relay", release: "after-1.0", stars: [] },
    { id: "gamification", title: "Gamification", release: "after-2.0", stars: [] },
  ],
  edges: [
    [1, 2],
    [2, 3],
    [9, 3],
  ],
  chain: [1, 2, 3],
  drift: [],
  reuses: [],
});

const SKY: SkyModel = deepFreeze({ ageMinutes: 1, priorityAgeDays: 0, constellations: [C] });
const num = (el: Element, attr: string) => Number(el.getAttribute(attr));

afterEach(() => call.mockReset());

test("onRing spreads points evenly from the top, clockwise in screen space", () => {
  const top = onRing(0, 4, 100);
  expect(top.x).toBeCloseTo(500);
  expect(top.y).toBeCloseTo(400);
  const right = onRing(1, 4, 100);
  expect(right.x).toBeCloseTo(600);
  expect(right.y).toBeCloseTo(500);
  const around = onRing(2, 4, 10, 50, 60);
  expect(around.x).toBeCloseTo(50);
  expect(around.y).toBeCloseTo(70);
});

test("1.0 features sit on the 250 ring and after-1.0 on the 350 ring and after-2.0 on the 450 ring", () => {
  expect(RINGS).toEqual({ "1.0": 250, "after-1.0": 350, "after-2.0": 450 });
  const at = layoutConstellation(C);
  expect(at.features.get("core")!.x).toBeCloseTo(500);
  expect(at.features.get("core")!.y).toBeCloseTo(250);
  expect(at.features.get("sky")!.x).toBeCloseTo(500 + 250 * Math.cos(Math.PI / 6));
  expect(at.features.get("sky")!.y).toBeCloseTo(625);
  expect(at.features.get("relay")!.x).toBeCloseTo(500);
  expect(at.features.get("relay")!.y).toBeCloseTo(150);
  expect(at.features.get("gamification")!.x).toBeCloseTo(500);
  expect(at.features.get("gamification")!.y).toBeCloseTo(50);
});

test("a feature's stars sit 40 from it, the first at the top", () => {
  const at = layoutConstellation(C);
  expect(at.stars.get(1)!.y).toBeCloseTo(210);
  expect(at.stars.get(2)!.x).toBeCloseTo(500);
  expect(at.stars.get(2)!.y).toBeCloseTo(290);
  expect(at.stars.get(3)!.y).toBeCloseTo(585);
  expect(at.stars.size).toBe(3);
});

test("one cluster per feature in block order, with its release", () => {
  render(<ConstellationView c={C} onBack={() => {}} />);
  const clusters = screen.getAllByTestId("feature");
  expect(clusters.map((g) => g.getAttribute("data-feature"))).toEqual(["core", "sky", "budget", "relay", "gamification"]);
  expect(clusters.map((g) => g.getAttribute("data-release"))).toEqual(["1.0", "1.0", "1.0", "after-1.0", "after-2.0"]);
  expect(screen.getByText("Constellation")).toBeTruthy();
});

test("one star per issue at its layout place, and a dark star for a feature with none", () => {
  render(<ConstellationView c={C} onBack={() => {}} />);
  const stars = screen.getAllByTestId("star");
  expect(stars.map((s) => num(s, "data-number"))).toEqual([1, 2, 3]);
  expect(num(stars[1], "cy")).toBeCloseTo(290);
  expect(screen.getAllByTestId("dark-star")).toHaveLength(3);
  expect(screen.getByRole("button", { name: "#3 issue 3" })).toBeTruthy();
});

test("a line for each blocked-by pair whose two stars are drawn", () => {
  render(<ConstellationView c={C} onBack={() => {}} />);
  const lines = screen.getAllByTestId("blocked-by");
  expect(lines.map((l) => [num(l, "data-from"), num(l, "data-to")])).toEqual([
    [1, 2],
    [2, 3],
  ]);
  expect(num(lines[0], "y1")).toBeCloseTo(210);
  expect(num(lines[0], "y2")).toBeCloseTo(290);
});

test("a ring for the 1.0, After 1.0 and After 2.0 milestones", () => {
  render(<ConstellationView c={C} onBack={() => {}} />);
  const rings = screen.getAllByTestId("ring");
  expect(rings.map((r) => r.getAttribute("data-milestone"))).toEqual(["1.0", "After 1.0", "After 2.0"]);
  expect(rings.map((r) => num(r, "r"))).toEqual([250, 350, 450]);
});

test("clicking a star calls onStar with that star", () => {
  const onStar = vi.fn();
  render(<ConstellationView c={C} onBack={() => {}} onStar={onStar} />);
  fireEvent.click(screen.getByRole("button", { name: "#2 issue 2" }));
  expect(onStar).toHaveBeenCalledWith(C.features[0].stars[1]);
});

test("the back button and Escape both go back", () => {
  const onBack = vi.fn();
  render(<ConstellationView c={C} onBack={onBack} />);
  fireEvent.click(screen.getByRole("button", { name: "Back to the sky" }));
  fireEvent.keyDown(window, { key: "Escape" });
  fireEvent.keyDown(window, { key: "Enter" });
  expect(onBack).toHaveBeenCalledTimes(2);
});

test("in the Sky view, clicking a constellation zooms in and back returns to the sky", async () => {
  call.mockResolvedValue({ ok: SKY });
  render(<Sky />);
  fireEvent.click(await screen.findByTestId("constellation"));
  expect(screen.getAllByTestId("feature")).toHaveLength(5);
  expect(screen.queryByTestId("constellation")).toBeNull();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.getByTestId("constellation")).toBeTruthy();
  expect(screen.queryByTestId("feature")).toBeNull();
});
