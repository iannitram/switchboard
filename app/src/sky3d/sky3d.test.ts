// The 3D sky's pure parts: seeded galaxies, 3D layout, labels, panel anchoring, flight timing.
import { gauss, hashString, mulberry32, seeded } from "./random";
import { galaxyParticles, galaxyShape, particleCount } from "./galaxy";
import { blendPose, clusterRadius, fitDistance, galaxyPlacement, INNER, layoutInterior, poseEye, SKY_REACH } from "./layout";
import { overlaps, overviewLabelRepos, placeLabels, shortName, truncate } from "./labels";
import { anchorPanel } from "./anchor";
import { complete, easeInOutCubic, flightValue, flyDuration, flyTo, FLY_MS, inFlight, rest } from "./motion";

describe("seeded randomness", () => {
  test("the hash is stable and differs between names", () => {
    expect(hashString("owner/one")).toBe(hashString("owner/one"));
    expect(hashString("owner/one")).not.toBe(hashString("owner/two"));
    expect(hashString("")).toBe(0x811c9dc5);
  });

  test("the same seed gives the same sequence, in [0, 1)", () => {
    const a = mulberry32(42);
    const b = mulberry32(42);
    const xs = Array.from({ length: 50 }, () => a());
    expect(Array.from({ length: 50 }, () => b())).toEqual(xs);
    expect(xs.every((x) => x >= 0 && x < 1)).toBe(true);
    expect(new Set(xs).size).toBe(50);
  });

  test("a normal deviate is finite and centred", () => {
    const r = seeded("gauss");
    const xs = Array.from({ length: 2000 }, () => gauss(r));
    const mean = xs.reduce((s, x) => s + x, 0) / xs.length;
    expect(xs.every(Number.isFinite)).toBe(true);
    expect(Math.abs(mean)).toBeLessThan(0.1);
  });
});

describe("procedural galaxies", () => {
  test("a repo always gets the same shape and particles", () => {
    const s1 = galaxyShape("owner/switchboard");
    expect(galaxyShape("owner/switchboard")).toEqual(s1);
    const p1 = galaxyParticles("owner/switchboard", s1, 500);
    const p2 = galaxyParticles("owner/switchboard", s1, 500);
    expect(Array.from(p2.positions)).toEqual(Array.from(p1.positions));
    expect(Array.from(p2.colors)).toEqual(Array.from(p1.colors));
    expect(Array.from(p2.sizes)).toEqual(Array.from(p1.sizes));
  });

  test("different repos get different galaxies", () => {
    const a = galaxyParticles("owner/a", galaxyShape("owner/a"), 200);
    const b = galaxyParticles("owner/b", galaxyShape("owner/b"), 200);
    expect(Array.from(a.positions)).not.toEqual(Array.from(b.positions));
  });

  test("the shapes vary across many repos and stay in range", () => {
    const shapes = Array.from({ length: 60 }, (_, i) => galaxyShape(`owner/repo-${i}`));
    expect(new Set(shapes.map((s) => s.kind)).size).toBe(3);
    for (const s of shapes) {
      if (s.kind === "elliptical") expect(s.arms).toBe(0);
      else expect(s.arms).toBeGreaterThanOrEqual(2);
      expect(s.arms).toBeLessThanOrEqual(4);
      expect(Math.abs(s.spin)).toBeGreaterThan(0);
      expect(s.hue).toBeGreaterThanOrEqual(0);
      expect(s.hue).toBeLessThan(1);
    }
  });

  test("particles fill the arrays, stay near the unit disc, and colours stay in 0..1", () => {
    for (const repo of ["owner/x", "owner/y", "owner/z", "owner/w"]) {
      const p = galaxyParticles(repo, galaxyShape(repo), 400);
      expect(p.positions).toHaveLength(1200);
      expect(p.colors).toHaveLength(1200);
      expect(p.sizes).toHaveLength(400);
      for (let i = 0; i < 400; i++) {
        const [x, y, z] = [p.positions[i * 3], p.positions[i * 3 + 1], p.positions[i * 3 + 2]];
        expect(Math.hypot(x, y, z)).toBeLessThan(2);
      }
      expect(Array.from(p.colors).every((c) => c >= 0 && c <= 1)).toBe(true);
    }
  });

  test("a bigger project gets more particles", () => {
    expect(particleCount(1)).toBeGreaterThan(particleCount(0.5));
    expect(particleCount(0.5)).toBeGreaterThan(particleCount(0));
    expect(particleCount(5)).toBe(particleCount(1));
  });
});

describe("3D layout", () => {
  const c = (repo: string, radius: number, angle: number, size: number) => ({ repo, radius, angle, size });

  test("the core's radius sets the distance from the centre, and its angle the direction", () => {
    const near = galaxyPlacement(c("owner/a", 0, 0, 1));
    const far = galaxyPlacement(c("owner/a", 1, 0, 1));
    expect(Math.hypot(near.x, near.z)).toBeCloseTo(SKY_REACH * INNER);
    expect(Math.hypot(far.x, far.z)).toBeCloseTo(SKY_REACH);
    const up = galaxyPlacement(c("owner/a", 0.5, Math.PI / 2, 1));
    expect(up.x).toBeCloseTo(0);
    expect(up.z).toBeGreaterThan(0);
  });

  test("moving up the order moves a galaxy inward and makes it larger", () => {
    const ranks = [0, 0.2, 0.5, 0.9].map((r, i) => galaxyPlacement(c("owner/a", r, 1, 1 - i * 0.2)));
    for (let i = 1; i < ranks.length; i++) {
      expect(Math.hypot(ranks[i].x, ranks[i].z)).toBeGreaterThan(Math.hypot(ranks[i - 1].x, ranks[i - 1].z));
      expect(ranks[i].scale).toBeLessThan(ranks[i - 1].scale);
    }
  });

  test("depth is deterministic per repo and small near the centre", () => {
    expect(galaxyPlacement(c("owner/a", 0.7, 1, 0.5))).toEqual(galaxyPlacement(c("owner/a", 0.7, 1, 0.5)));
    expect(Math.abs(galaxyPlacement(c("owner/a", 0, 1, 1)).y)).toBeLessThanOrEqual(4);
  });

  const star = (n: number) => ({ number: n });
  const feature = (id: string, release: "1.0" | "after-1.0" | "after-2.0", n: number, first = 1) => ({
    id,
    release,
    stars: Array.from({ length: n }, (_, i) => star(first + i)),
  });

  test("an opened constellation: three rings, outermost at 1, inner to outer", () => {
    const l = layoutInterior({ repo: "owner/a", features: [] });
    expect(l.rings.map((r) => r.release)).toEqual(["1.0", "after-1.0", "after-2.0"]);
    expect(l.rings[2].radius).toBeCloseTo(1);
    expect(l.rings[0].radius).toBeLessThan(l.rings[1].radius);
    expect(l.rings[1].radius).toBeLessThan(l.rings[2].radius);
  });

  test("each feature sits on its milestone's ring and each star in its cluster", () => {
    const features = [feature("core", "1.0", 4, 1), feature("ui", "1.0", 12, 10), feature("later", "after-1.0", 0), feature("far", "after-2.0", 2, 40)];
    const l = layoutInterior({ repo: "owner/a", features });
    const ring = (rel: string) => l.rings.find((r) => r.release === rel)!.radius;
    for (const f of features) {
      const at = l.features.get(f.id)!;
      expect(Math.hypot(at.x, at.z)).toBeCloseTo(ring(f.release), 5);
      for (const s of f.stars) {
        const p = l.stars.get(s.number)!;
        expect(Math.hypot(p.x - at.x, p.z - at.z)).toBeLessThanOrEqual(at.r + 1e-9);
      }
    }
    expect(l.stars.size).toBe(18);
  });

  test("clusters on a crowded ring never overlap", () => {
    const features = Array.from({ length: 14 }, (_, i) => feature(`f${i}`, "1.0", i === 3 ? 33 : 1 + (i % 6), i * 100));
    const l = layoutInterior({ repo: "owner/a", features });
    const at = features.map((f) => l.features.get(f.id)!);
    for (let i = 0; i < at.length; i++)
      for (let j = i + 1; j < at.length; j++) expect(Math.hypot(at[i].x - at[j].x, at[i].z - at[j].z)).toBeGreaterThan(at[i].r + at[j].r);
    expect(clusterRadius(33)).toBeGreaterThan(clusterRadius(4));
  });

  test("the camera fits a sphere and poses blend", () => {
    const d = fitDistance(10, 45, 1);
    expect(d).toBeCloseTo(10 / Math.sin((22.5 * Math.PI) / 180));
    expect(fitDistance(10, 45, 0.5)).toBeGreaterThan(d); // a narrow window stands further back
    const a = { target: { x: 0, y: 0, z: 0 }, dist: 100, az: 0, elev: 0.5 };
    const b = { target: { x: 10, y: 0, z: 0 }, dist: 10, az: 0, elev: 1 };
    expect(blendPose(a, b, 0)).toEqual(a);
    const mid = blendPose(a, b, 0.5);
    expect(mid.dist).toBeCloseTo(Math.sqrt(1000));
    expect(mid.target.x).toBeCloseTo(5);
    const eye = poseEye({ target: { x: 0, y: 0, z: 0 }, dist: 10, az: 0, elev: Math.PI / 2 });
    expect(eye.y).toBeCloseTo(10);
  });
});

describe("labels", () => {
  const view = { width: 400, height: 300 };
  const cand = (id: string, x: number, y: number, priority: number, forced = false) => ({ id, x, y, width: 80, height: 16, priority, forced });

  test("overlapping labels: the higher priority one wins", () => {
    const placed = placeLabels([cand("low", 105, 100, 5), cand("high", 100, 100, 1)], view);
    expect([...placed.keys()]).toEqual(["high"]);
  });

  test("a forced (hovered) label beats any priority", () => {
    const placed = placeLabels([cand("top", 100, 100, 0), cand("hovered", 110, 104, 40, true)], view);
    expect([...placed.keys()]).toEqual(["hovered"]);
  });

  test("labels that do not touch all show", () => {
    expect(placeLabels([cand("a", 60, 20, 0), cand("b", 300, 20, 1), cand("c", 60, 200, 2)], view).size).toBe(3);
  });

  test("a label near the edge is nudged inside the view, never clipped", () => {
    const placed = placeLabels([cand("edge", 395, 295, 0)], view);
    const p = placed.get("edge")!;
    expect(p.left + 80).toBeLessThanOrEqual(view.width);
    expect(p.top + 16).toBeLessThanOrEqual(view.height);
    expect(p.left).toBeGreaterThanOrEqual(0);
  });

  test("a label whose object is far off-screen is dropped", () => {
    expect(placeLabels([cand("gone", -500, 100, 0)], view).size).toBe(0);
  });

  test("blockers (panels, controls) keep labels out, except a forced one", () => {
    const blocker = { left: 50, top: 90, width: 120, height: 40 };
    expect(placeLabels([cand("a", 100, 100, 0)], view, [blocker]).size).toBe(0);
    expect(placeLabels([cand("a", 100, 100, 0, true)], view, [blocker]).size).toBe(1);
  });

  test("no two placed labels overlap, over a crowded field", () => {
    const r = seeded("crowd");
    const cands = Array.from({ length: 60 }, (_, i) => ({ id: `l${i}`, x: r() * 400, y: r() * 300, width: 40 + r() * 60, height: 14, priority: i }));
    const placed = placeLabels(cands, view);
    const rects = [...placed.entries()].map(([id, p]) => ({ ...p, width: cands.find((c) => c.id === id)!.width, height: 14 }));
    for (let i = 0; i < rects.length; i++) for (let j = i + 1; j < rects.length; j++) expect(overlaps(rects[i], rects[j], 0)).toBe(false);
  });

  test("the overview labels the top n by rank plus the hovered one", () => {
    const cs = [3, 0, 2, 1, 4].map((rank) => ({ repo: `o/r${rank}`, rank }));
    expect([...overviewLabelRepos(cs, 2)].sort()).toEqual(["o/r0", "o/r1"]);
    expect([...overviewLabelRepos(cs, 2, ["o/r4", null])].sort()).toEqual(["o/r0", "o/r1", "o/r4"]);
  });

  test("long titles are cut at a word with an ellipsis", () => {
    expect(truncate("Short one", 20)).toBe("Short one");
    const t = truncate("Plugins: add views, commands and MCP tools to Switchboard", 26);
    expect(t.length).toBeLessThanOrEqual(26);
    expect(t.endsWith("…")).toBe(true);
    expect(t).toBe("Plugins: add views…");
    expect(shortName("owner/name")).toBe("name");
  });
});

describe("panel anchoring", () => {
  const view = { width: 1000, height: 600 };
  const panel = { width: 300, height: 200 };

  test("beside the object, to the right when it fits", () => {
    const a = anchorPanel({ x: 200, y: 300, r: 20 }, panel, view);
    expect(a.side).toBe("right");
    expect(a.left).toBe(234);
    expect(a.top).toBe(200);
  });

  test("flips to the left near the right edge", () => {
    const a = anchorPanel({ x: 900, y: 300, r: 20 }, panel, view);
    expect(a.side).toBe("left");
    expect(a.left + panel.width).toBeLessThanOrEqual(900 - 20);
  });

  test("clamped inside the view near the top and bottom, never below the fold", () => {
    const top = anchorPanel({ x: 200, y: 10, r: 10 }, panel, view);
    expect(top.top).toBe(12);
    const bottom = anchorPanel({ x: 200, y: 595, r: 10 }, panel, view);
    expect(bottom.top + panel.height).toBeLessThanOrEqual(view.height - 12);
  });

  test("in a narrow view it goes above or below, still inside", () => {
    const narrow = { width: 360, height: 700 };
    const a = anchorPanel({ x: 180, y: 200, r: 30 }, panel, narrow);
    expect(a.side).toBe("below");
    expect(a.left).toBeGreaterThanOrEqual(12);
    expect(a.left + panel.width).toBeLessThanOrEqual(narrow.width - 12);
    const b = anchorPanel({ x: 180, y: 600, r: 30 }, panel, narrow);
    expect(b.side).toBe("above");
    expect(b.top).toBeGreaterThanOrEqual(12);
  });
});

describe("flights", () => {
  test("quick, and instant with reduced motion", () => {
    expect(flyDuration(false)).toBe(FLY_MS);
    expect(FLY_MS).toBeGreaterThanOrEqual(450);
    expect(FLY_MS).toBeLessThanOrEqual(600);
    expect(flyDuration(true)).toBe(0);
    const f = flyTo(rest(0), 1000, 1, true);
    expect(flightValue(f, 1000)).toBe(1);
    expect(inFlight(f, 1000)).toBe(false);
  });

  test("a fly-in moves from 0 to 1 over its duration", () => {
    const f = flyTo(rest(0), 1000, 1, false);
    expect(flightValue(f, 1000)).toBe(0);
    expect(flightValue(f, 1000 + FLY_MS / 2)).toBeCloseTo(0.5);
    expect(flightValue(f, 1000 + FLY_MS)).toBe(1);
    expect(inFlight(f, 1100)).toBe(true);
  });

  test("Escape mid-flight reverses from where it is, taking only the time it needs", () => {
    const f = flyTo(rest(0), 0, 1, false);
    const back = flyTo(f, FLY_MS * 0.4, 0, false);
    expect(back.from).toBeCloseTo(0.4);
    expect(back.duration).toBeCloseTo(FLY_MS * 0.4);
    expect(flightValue(back, FLY_MS * 0.4 + back.duration)).toBe(0);
  });

  test("a click mid-flight completes it", () => {
    const f = flyTo(rest(0), 0, 1, false);
    expect(flightValue(complete(f), 1)).toBe(1);
  });

  test("the easing starts slow, ends slow and is clamped", () => {
    expect(easeInOutCubic(0)).toBe(0);
    expect(easeInOutCubic(1)).toBe(1);
    expect(easeInOutCubic(0.5)).toBeCloseTo(0.5);
    expect(easeInOutCubic(0.1)).toBeLessThan(0.1);
    expect(easeInOutCubic(2)).toBe(1);
  });
});
