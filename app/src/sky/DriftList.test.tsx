// Locked tests for #40 (SKY-5: the drift list). Written before the build (R28); the builder
// never edits this file.
import { render, screen, within } from "@testing-library/react";
import type { Constellation, DriftItem, SkyModel } from "./model";
import { DriftList } from "./DriftList";
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

const ROADMAP = "https://github.com/iannitram/switchboard/blob/main/ROADMAP.md";
const ORPHAN: DriftItem = { kind: "orphan", text: "#7 has no feature label", link: "https://github.com/iannitram/switchboard/issues/7" };
const DARK: DriftItem = { kind: "dark", text: "budget has no issue", link: ROADMAP };
const STALE: DriftItem = { kind: "stale", text: "ROADMAP.md is 15 days old", link: "https://github.com/iannitram/wavcraft/blob/main/ROADMAP.md" };

function constellation(repo: string, drift: DriftItem[]): Constellation {
  return {
    repo,
    url: `https://github.com/${repo}`,
    rank: 0,
    radius: 0,
    size: 1,
    angle: 0,
    brightness: 0,
    features: [],
    edges: [],
    chain: [],
    drift,
    reuses: [],
  };
}

const SKY: SkyModel = deepFreeze({
  ageMinutes: 1,
  priorityAgeDays: 0,
  constellations: [constellation("iannitram/switchboard", [ORPHAN, DARK]), constellation("iannitram/wavcraft", [STALE])],
});

afterEach(() => call.mockReset());

test("the heading counts the items", () => {
  render(<DriftList drift={[{ ...ORPHAN, repo: "iannitram/switchboard" }]} />);
  expect(screen.getByRole("heading", { name: "Drift (1)" })).toBeTruthy();
});

test("each item names its repo after the slash, says what drifted and links to the fix", () => {
  render(<DriftList drift={[{ ...ORPHAN, repo: "iannitram/switchboard" }, { ...DARK, repo: "iannitram/switchboard" }]} />);
  const links = screen.getAllByRole("link");
  expect(links.map((a) => a.textContent)).toEqual(["switchboard: #7 has no feature label", "switchboard: budget has no issue"]);
  expect(links.map((a) => a.getAttribute("href"))).toEqual([ORPHAN.link, ROADMAP]);
});

test("each item carries its kind", () => {
  render(<DriftList drift={[{ ...STALE, repo: "iannitram/wavcraft" }, { ...DARK, repo: "iannitram/switchboard" }]} />);
  const items = screen.getAllByRole("listitem");
  expect(items.map((li) => li.getAttribute("data-kind"))).toEqual(["stale", "dark"]);
});

test("no drift says so", () => {
  render(<DriftList drift={[]} />);
  expect(screen.getByRole("heading", { name: "Drift (0)" })).toBeTruthy();
  expect(screen.getByText("No drift")).toBeTruthy();
  expect(screen.queryByRole("listitem")).toBeNull();
});

test("the Sky view lists every constellation's drift in sky order", async () => {
  call.mockResolvedValue({ ok: SKY });
  render(<Sky />);
  const list = await screen.findByRole("region", { name: "Drift" });
  expect(within(list).getByRole("heading", { name: "Drift (3)" })).toBeTruthy();
  expect(within(list).getAllByRole("link").map((a) => a.textContent)).toEqual([
    "switchboard: #7 has no feature label",
    "switchboard: budget has no issue",
    "wavcraft: ROADMAP.md is 15 days old",
  ]);
});
