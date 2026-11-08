// Locked tests for #37 (SKY-1: the Sky view draws the constellations). Written before the
// build (R28); the builder never edits this file.
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { Constellation, SkyModel } from "./model";
import { glow, project, VIEW } from "./geometry";
import { SkyMap } from "./SkyMap";
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

function constellation(repo: string, over: Partial<Constellation> = {}): Constellation {
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
    drift: [],
    reuses: [],
    ...over,
  };
}

const SKY: SkyModel = deepFreeze({
  ageMinutes: 4,
  priorityAgeDays: 2,
  constellations: [
    constellation("iannitram/switchboard", { brightness: 1 }),
    constellation("iannitram/wavcraft", {
      rank: 3,
      radius: 0.5,
      size: 0.5,
      angle: Math.PI / 2,
      brightness: 0.5,
      drift: [{ kind: "dark", text: "sky has no issue", link: "https://github.com/iannitram/wavcraft/blob/main/ROADMAP.md" }],
    }),
  ],
});

const num = (el: Element, attr: string) => Number(el.getAttribute(attr));

afterEach(() => call.mockReset());

test("project puts radius 0 at the centre and scales by 450 along the angle", () => {
  expect(VIEW).toBe(1000);
  expect(project({ radius: 0, angle: 1, size: 1 })).toEqual({ x: 500, y: 500, r: 48 });
  const east = project({ radius: 0.5, angle: 0, size: 0.5 });
  expect(east.x).toBeCloseTo(725);
  expect(east.y).toBeCloseTo(500);
  expect(east.r).toBeCloseTo(30);
  const south = project({ radius: 1, angle: Math.PI / 2, size: 0 });
  expect(south.x).toBeCloseTo(500);
  expect(south.y).toBeCloseTo(950);
  expect(south.r).toBeCloseTo(12);
});

test("glow maps brightness 0..1 to opacity 0.25..1", () => {
  expect(glow(0)).toBeCloseTo(0.25);
  expect(glow(0.5)).toBeCloseTo(0.625);
  expect(glow(1)).toBeCloseTo(1);
});

test("one constellation per repo, in priority order, at its projected place", () => {
  render(<SkyMap sky={SKY} />);
  const groups = screen.getAllByTestId("constellation");
  expect(groups.map((g) => g.getAttribute("data-repo"))).toEqual(["iannitram/switchboard", "iannitram/wavcraft"]);
  const second = groups[1].querySelector("circle")!;
  expect(num(second, "cx")).toBeCloseTo(500);
  expect(num(second, "cy")).toBeCloseTo(725);
  expect(num(second, "r")).toBeCloseTo(30);
});

test("each constellation's fill opacity is its glow", () => {
  render(<SkyMap sky={SKY} />);
  const [a, b] = screen.getAllByTestId("constellation").map((g) => g.querySelector("circle")!);
  expect(num(a, "fill-opacity")).toBeCloseTo(1);
  expect(num(b, "fill-opacity")).toBeCloseTo(0.625);
});

test("each constellation is labelled with the repo name after the slash", () => {
  render(<SkyMap sky={SKY} />);
  const [a, b] = screen.getAllByTestId("constellation");
  expect(within(a).getByText("switchboard")).toBeTruthy();
  expect(within(b).getByText("wavcraft")).toBeTruthy();
});

test("a drift count shows only on a constellation with drift", () => {
  render(<SkyMap sky={SKY} />);
  const [a, b] = screen.getAllByTestId("constellation");
  expect(within(a).queryByTestId("drift-count")).toBeNull();
  expect(within(b).getByTestId("drift-count").textContent).toBe("1");
});

test("the snapshot age and the priority copy age are on the sky", () => {
  const { unmount } = render(<SkyMap sky={SKY} />);
  expect(screen.getByTestId("ages").textContent).toBe("snapshot 4 min old · priority copy 2 days old");
  unmount();
  render(<SkyMap sky={deepFreeze({ ...SKY, priorityAgeDays: null })} />);
  expect(screen.getByTestId("ages").textContent).toBe("snapshot 4 min old · no priority copy");
});

test("clicking a constellation calls onOpen with its repo", () => {
  const onOpen = vi.fn();
  render(<SkyMap sky={SKY} onOpen={onOpen} />);
  fireEvent.click(screen.getAllByTestId("constellation")[1]);
  expect(onOpen).toHaveBeenCalledWith("iannitram/wavcraft");
});

test("the Sky view asks the core for `sky` and draws what it answers", async () => {
  call.mockResolvedValue({ ok: SKY });
  render(<Sky />);
  expect(await screen.findAllByTestId("constellation")).toHaveLength(2);
  expect(call).toHaveBeenCalledWith("sky");
});

test("an empty sky says no data yet", async () => {
  call.mockResolvedValue({ ok: deepFreeze({ ...SKY, constellations: [] }) });
  render(<Sky />);
  expect(await screen.findByText("no data yet")).toBeTruthy();
  expect(screen.queryAllByTestId("constellation")).toHaveLength(0);
});

test("a core error says no data yet and shows the command and its lines", async () => {
  call.mockResolvedValue({ error: { command: "sky", lines: ["snapshot missing", "run refresh"] } });
  render(<Sky />);
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toBe("sky failed: snapshot missing / run refresh");
  expect(screen.getByText("no data yet")).toBeTruthy();
});
