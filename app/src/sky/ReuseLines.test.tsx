// Locked tests for #41 (SKY-6: reuse lines between constellations). Written before the build
// (R28); the builder never edits this file.
import { render, screen } from "@testing-library/react";
import type { Constellation, Reuse, SkyModel } from "./model";
import { REUSE_STROKE, ReuseLines, WARNING_STROKE, reuseLines } from "./ReuseLines";
import { SkyMap } from "./SkyMap";

function deepFreeze<T>(o: T): T {
  if (o && typeof o === "object" && !Object.isFrozen(o)) {
    Object.freeze(o);
    for (const v of Object.values(o as object)) deepFreeze(v);
  }
  return o;
}

function constellation(repo: string, radius: number, angle: number, reuses: Reuse[] = []): Constellation {
  return {
    repo,
    url: `https://github.com/${repo}`,
    rank: 0,
    radius,
    size: 1,
    angle,
    brightness: 0,
    features: [],
    edges: [],
    chain: [],
    drift: [],
    reuses,
  };
}

// switchboard sits at the centre (500, 500); wavcraft at radius 1, angle 0 sits at (950, 500);
// nc-outliner at radius 1, angle pi/2 sits at (500, 950).
const SKY: SkyModel = deepFreeze({
  ageMinutes: 1,
  priorityAgeDays: 0,
  constellations: [
    constellation("iannitram/switchboard", 0, 0, [
      { repo: "wavcraft", what: "the audio graph", warning: false },
      { repo: "iannitram/nc-outliner", what: "the outline tree", warning: true },
      { repo: "brocrew", what: "the chat parts", warning: true },
      { repo: "switchboard", what: "itself", warning: false },
    ]),
    constellation("iannitram/wavcraft", 1, 0),
    constellation("iannitram/nc-outliner", 1, Math.PI / 2),
  ],
});

test("the two strokes", () => {
  expect(REUSE_STROKE).toBe("#94a3b8");
  expect(WARNING_STROKE).toBe("#d97706");
});

test("a line runs from the reusing constellation to the reused one, matched by name or owner/name", () => {
  const lines = reuseLines(SKY);
  expect(lines).toHaveLength(2);
  expect(lines[0].from.x).toBeCloseTo(500);
  expect(lines[0].from.y).toBeCloseTo(500);
  expect(lines[0].to.x).toBeCloseTo(950);
  expect(lines[0].to.y).toBeCloseTo(500);
  expect(lines[1].to.x).toBeCloseTo(500);
  expect(lines[1].to.y).toBeCloseTo(950);
});

test("each line is labelled and keeps its warning", () => {
  expect(reuseLines(SKY).map((l) => [l.label, l.warning])).toEqual([
    ["switchboard reuses wavcraft", false],
    ["switchboard reuses nc-outliner", true],
  ]);
});

test("a reused repo not on the sky, and a repo reusing itself, draw nothing", () => {
  expect(reuseLines(SKY).some((l) => l.label.includes("brocrew"))).toBe(false);
  expect(reuseLines(SKY).some((l) => l.label === "switchboard reuses switchboard")).toBe(false);
});

test("lines are dashed, grey or amber, and carry their label as a title", () => {
  render(
    <svg>
      <ReuseLines lines={reuseLines(SKY)} />
    </svg>,
  );
  const drawn = screen.getAllByTestId("reuse-line");
  expect(drawn.map((l) => l.getAttribute("stroke"))).toEqual(["#94a3b8", "#d97706"]);
  expect(drawn.map((l) => l.getAttribute("data-warning"))).toEqual(["false", "true"]);
  expect(drawn.every((l) => l.getAttribute("stroke-dasharray") === "6 4")).toBe(true);
  expect(drawn.map((l) => l.querySelector("title")?.textContent)).toEqual([
    "switchboard reuses wavcraft",
    "switchboard reuses nc-outliner",
  ]);
  expect(Number(drawn[0].getAttribute("x2"))).toBeCloseTo(950);
});

test("the sky map draws the reuse lines under the constellations", () => {
  const { container } = render(<SkyMap sky={SKY} />);
  expect(screen.getAllByTestId("reuse-line")).toHaveLength(2);
  const order = Array.from(container.querySelectorAll('[data-testid="reuse-line"], [data-testid="constellation"]'));
  expect(order[0].getAttribute("data-testid")).toBe("reuse-line");
  expect(order[order.length - 1].getAttribute("data-testid")).toBe("constellation");
});
