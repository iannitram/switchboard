// Pass 2 of the 3D sky: one living galaxy. The pure parts: placement, zoom, physics, wayfinding.
import type { Constellation, SkyModel, Star } from "../sky/model";
import { centreRepo, discParticles, GAL_R, HOLE, isCentre, nebulaStyle, placeSystems, R0, remapRadius, systemParticles } from "./milkyway";
import { damp, dampPose, detailFade, ENTER, EXIT, labelBudget, MIN_DIST, pinchFactor, wheelFactor, zoneStep, zoomOutToward, zoomToward, flightMs, FLIGHT_MAX_MS, FLIGHT_MIN_MS } from "./camera";
import { condensation, DEBRIS_MAX, debrisCount, lensMatches, lensStar, lensSystem, magnitude, starPhysics, type SystemFacts } from "./physics";
import { outsideX } from "./labels";
import { breadcrumbs, searchSky, spanningTree } from "./wayfind";
import type { Pose } from "./layout";

const star = (number: number, extra: Partial<Star> = {}): Star => ({
  number,
  title: `Issue ${number}`,
  state: "open",
  url: "",
  labels: [],
  size: null,
  milestone: null,
  inFlight: false,
  pr: null,
  job: null,
  ...extra,
});

const con = (repo: string, extra: Partial<Constellation> = {}): Constellation => ({
  repo,
  url: "",
  rank: 0,
  radius: 0.5,
  size: 0.5,
  angle: 0,
  brightness: 0,
  features: [],
  edges: [],
  chain: [],
  drift: [],
  reuses: [],
  ...extra,
});

describe("one galaxy: where systems sit", () => {
  test("the core's radius is remapped so the centre stays free", () => {
    expect(remapRadius(0)).toBe(R0);
    expect(remapRadius(1)).toBe(1);
    expect(remapRadius(0.5)).toBeCloseTo(R0 + (1 - R0) * 0.5);
    expect(remapRadius(-3)).toBe(R0);
    expect(remapRadius(9)).toBe(1);
  });

  test("the switchboard repo is the centre, matched by name after the owner", () => {
    expect(isCentre("someone/switchboard")).toBe(true);
    expect(isCentre("someone/Switchboard")).toBe(true);
    expect(isCentre("switchboard/other")).toBe(false);
    expect(centreRepo([{ repo: "a/x" }, { repo: "b/switchboard" }])).toBe("b/switchboard");
    expect(centreRepo([{ repo: "a/x" }])).toBeNull();
  });

  test("the centre project sits at the origin; the rest keep rank distance and angle", () => {
    const cs = [con("o/switchboard", { rank: 5, radius: 0.6 }), con("o/a", { radius: 0, angle: 0 }), con("o/b", { radius: 1, angle: Math.PI / 2 })];
    const s = placeSystems(cs);
    const sw = s.get("o/switchboard")!;
    expect([sw.x, sw.y, sw.z, sw.centre]).toEqual([0, 0, 0, true]);
    expect(sw.R).toBe(HOLE.systemR);
    const a = s.get("o/a")!;
    expect(Math.hypot(a.x, a.z)).toBeCloseTo(GAL_R * R0);
    const b = s.get("o/b")!;
    expect(Math.hypot(b.x, b.z)).toBeCloseTo(GAL_R);
    expect(b.z).toBeGreaterThan(0);
    expect(Math.abs(b.x)).toBeLessThan(1e-6);
  });

  test("without a switchboard repo nothing sits at the centre", () => {
    const s = placeSystems([con("o/a", { radius: 0 }), con("o/b", { radius: 0.4, angle: 2 })]);
    for (const sys of s.values()) {
      expect(sys.centre).toBe(false);
      expect(Math.hypot(sys.x, sys.z)).toBeGreaterThanOrEqual(GAL_R * R0 - 1e-9);
    }
  });

  test("opened radii never overlap their neighbours", () => {
    const cs = Array.from({ length: 50 }, (_, i) => con(`o/p${i}`, { radius: (i % 10) / 10, angle: i * 2.4, size: 1 - i / 50 }));
    cs.push(con("o/switchboard"));
    const s = [...placeSystems(cs).values()];
    for (const a of s)
      for (const b of s) {
        if (a === b) continue;
        const d = Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
        expect(a.R + b.R).toBeLessThanOrEqual(d + 1e-6 + Math.max(a.scale, b.scale) * 3);
      }
  });

  test("the disc is deterministic and stays inside the galaxy", () => {
    const a = discParticles("t", 3000);
    const b = discParticles("t", 3000);
    expect(Array.from(a.orbit)).toEqual(Array.from(b.orbit));
    for (let i = 0; i < a.count; i++) {
      expect(a.orbit[i * 4]).toBeLessThan(GAL_R * 1.6);
      expect(a.orbit[i * 4 + 3]).toBeGreaterThan(0);
    }
    expect(Array.from(a.colors).every((c) => c >= 0 && c <= 1)).toBe(true);
  });
});

describe("one galaxy: nebulae to clusters", () => {
  test("roadmap-less projects no longer look alike: forms and colours vary", () => {
    const styles = Array.from({ length: 52 }, (_, i) => nebulaStyle(`o/repo-${i}`));
    expect(new Set(styles.map((s) => s.form)).size).toBe(4);
    expect(new Set(styles.map((s) => s.a.join() + s.b.join())).size).toBeGreaterThan(10);
    for (const s of styles) expect(s.a).not.toEqual(s.b);
  });

  test("condensation: uncharted is a nebula, progress condenses it", () => {
    expect(condensation({ brightness: 0.9, features: [] })).toBe(0);
    expect(condensation({ brightness: 0.4, features: [{ id: "a", title: "", release: "1.0", stars: [] }] })).toBe(0.4);
    expect(condensation({ brightness: 3, features: [{ id: "a", title: "", release: "1.0", stars: [] }] })).toBe(1);
    const gas = (c: number) => Array.from(systemParticles("o/x", c, 2000).kind).filter((k) => k === 1).length;
    expect(gas(0)).toBeGreaterThan(gas(1));
  });
});

describe("one galaxy: zoom", () => {
  const pose: Pose = { target: { x: 0, y: 0, z: 0 }, dist: 100, az: 0, elev: 1 };

  test("wheel down zooms out, up zooms in; lines and pinches scale; one event is capped", () => {
    expect(wheelFactor(100, 0, false)).toBeGreaterThan(1);
    expect(wheelFactor(-100, 0, false)).toBeLessThan(1);
    expect(wheelFactor(3, 1, false)).toBeCloseTo(wheelFactor(48, 0, false));
    expect(wheelFactor(-5, 0, true)).toBeLessThan(wheelFactor(-5, 0, false));
    expect(wheelFactor(100000, 0, false)).toBeCloseTo(Math.exp(0.6));
    expect(wheelFactor(0, 0, false)).toBe(1);
  });

  test("fingers apart zoom in, together zoom out", () => {
    expect(pinchFactor(100, 200)).toBe(0.5);
    expect(pinchFactor(200, 100)).toBe(2);
    expect(pinchFactor(0, 100)).toBe(1);
  });

  test("zooming toward the cursor keeps that point fixed, within the limits", () => {
    const anchor = { x: 40, y: 0, z: -20 };
    const p = zoomToward(pose, 0.5, anchor, MIN_DIST, 500);
    expect(p.dist).toBe(50);
    expect(p.target.x).toBeCloseTo(20);
    expect(p.target.z).toBeCloseTo(-10);
    expect(zoomToward(pose, 1e-9, anchor, MIN_DIST, 500).dist).toBe(MIN_DIST);
    expect(zoomToward(pose, 1e9, null, MIN_DIST, 500).dist).toBe(500);
    expect(zoomToward(pose, 0.5, null, MIN_DIST, 500).target).toEqual(pose.target);
  });

  test("zooming out never drags the view: it eases from the view centre to the galaxy centre", () => {
    const home = { x: 0, y: -5, z: 0 };
    const near: Pose = { target: { x: 60, y: 0, z: 30 }, dist: 10, az: 0, elev: 1 };
    // Each step moves the target only toward home, never away (the old bug scaled it outward).
    let p = near;
    let prev = Math.hypot(p.target.x - home.x, p.target.y - home.y, p.target.z - home.z);
    for (let i = 0; i < 40; i++) {
      p = zoomOutToward(p, 1.15, home, 400, 640);
      const d = Math.hypot(p.target.x - home.x, p.target.y - home.y, p.target.z - home.z);
      expect(d).toBeLessThanOrEqual(prev + 1e-9);
      prev = d;
    }
    // Home exactly once the whole galaxy is in view; the steps add up the same however split.
    expect(p.target).toEqual(home);
    const one = zoomOutToward(near, 4, home, 400, 640);
    const two = zoomOutToward(zoomOutToward(near, 2, home, 400, 640), 2, home, 400, 640);
    expect(two.target.x).toBeCloseTo(one.target.x);
    expect(two.target.z).toBeCloseTo(one.target.z);
    expect(one.target.x).toBeCloseTo(60 * (1 - Math.log(4) / Math.log(40)));
    // Capped at the far limit; a factor at or below 1 is not a zoom out.
    expect(zoomOutToward(near, 1e9, home, 400, 640).dist).toBe(640);
    expect(zoomOutToward(near, 0.5, home, 400, 640)).toEqual(near);
  });

  test("damping eases toward the goal independent of frame rate", () => {
    expect(damp(0, 10, 9, 0)).toBe(0);
    const twoSteps = damp(damp(0, 10, 9, 1 / 120), 10, 9, 1 / 120);
    expect(twoSteps).toBeCloseTo(damp(0, 10, 9, 1 / 60));
    expect(damp(0, 10, 9, 10)).toBeCloseTo(10);
    const goal: Pose = { target: { x: 5, y: 0, z: 0 }, dist: 10, az: -3, elev: 1 };
    const half = dampPose({ ...pose, az: 3 }, goal, 0.05);
    expect(half.dist).toBeLessThan(100);
    expect(half.dist).toBeGreaterThan(10);
    // The short way round: from 3 to -3 radians crosses pi, not zero.
    expect(Math.abs(half.az)).toBeGreaterThan(3);
  });

  test("entering and leaving a project by distance, with hysteresis", () => {
    const zones = [{ repo: "o/a", x: 50, y: 0, z: 0, R: 8 }];
    const at = (dist: number, x = 50): Pose => ({ ...pose, target: { x, y: 0, z: 0 }, dist });
    expect(zoneStep(null, at(ENTER * 8 + 1), zones)).toBeNull();
    expect(zoneStep(null, at(ENTER * 8 - 1), zones)).toEqual({ enter: "o/a" });
    expect(zoneStep(null, at(ENTER * 8 - 1, 0), zones)).toBeNull(); // looking elsewhere
    expect(zoneStep("o/a", at((ENTER + EXIT) * 4), zones)).toBeNull(); // between the two: stay
    expect(zoneStep("o/a", at(EXIT * 8 + 1), zones)).toEqual({ leave: true });
    expect(zoneStep("o/a", at(10, 50 + 8 * 3), zones)).toEqual({ leave: true }); // panned away
    expect(zoneStep("o/gone", at(10), zones)).toEqual({ leave: true });
  });

  test("detail fades in smoothly before the entry point, never pops", () => {
    expect(detailFade(100, 8)).toBe(0);
    expect(detailFade(ENTER * 8, 8)).toBe(1);
    const mid = detailFade(4.25 * 8, 8);
    expect(mid).toBeGreaterThan(0);
    expect(mid).toBeLessThan(1);
    for (let d = 20; d < 50; d += 0.5) expect(detailFade(d, 8)).toBeGreaterThanOrEqual(detailFade(d + 0.5, 8));
  });

  test("more names show as the camera comes in", () => {
    expect(labelBudget(100, 100, 52)).toBe(14);
    expect(labelBudget(50, 100, 52)).toBe(52);
    expect(labelBudget(80, 100, 52)).toBeGreaterThan(14);
    expect(labelBudget(200, 100, 5)).toBe(5);
  });

  test("a click flight takes about half a second, none with reduced motion", () => {
    const far: Pose = { ...pose, dist: 300 };
    const near: Pose = { ...pose, target: { x: 80, y: 0, z: 0 }, dist: 20 };
    const ms = flightMs(far, near, false);
    expect(ms).toBeGreaterThanOrEqual(FLIGHT_MIN_MS);
    expect(ms).toBeLessThanOrEqual(FLIGHT_MAX_MS);
    expect(flightMs(far, near, true)).toBe(0);
  });
});

describe("one galaxy: stellar physics", () => {
  test("magnitude from the size label: hard is a giant, unsized is small", () => {
    expect(magnitude("easy")).toBeLessThan(magnitude("medium"));
    expect(magnitude("medium")).toBeLessThan(magnitude("hard"));
    expect(magnitude(null)).toBeLessThan(magnitude("easy"));
    expect(magnitude("weird")).toBe(magnitude(null));
  });

  test("closed burns, open is a protostar, in flight is a pulsar; the verdict is a corona", () => {
    expect(starPhysics(star(1, { state: "closed" })).kind).toBe("closed");
    expect(starPhysics(star(1)).kind).toBe("open");
    expect(starPhysics(star(1, { state: "closed", inFlight: true })).kind).toBe("pulsar");
    const pr = (verdict: "PASS" | "FAIL" | null) => ({ number: 2, url: "", state: "open", verdict });
    expect(starPhysics(star(1, { pr: pr("PASS") })).corona).toBe("pass");
    expect(starPhysics(star(1, { pr: pr("FAIL") })).corona).toBe("fail");
    expect(starPhysics(star(1, { pr: pr(null) })).corona).toBeNull();
    expect(starPhysics(star(1)).neutral).toBe(true);
    expect(starPhysics(star(1, { size: "hard" })).neutral).toBe(false);
  });

  test("debris grows with drift and is capped", () => {
    expect(debrisCount(0)).toBe(0);
    expect(debrisCount(-2)).toBe(0);
    expect(debrisCount(1)).toBeGreaterThan(0);
    expect(debrisCount(10)).toBeGreaterThan(debrisCount(1));
    expect(debrisCount(101)).toBeGreaterThan(debrisCount(50));
    expect(debrisCount(100000)).toBe(DEBRIS_MAX);
  });

  test("each lens lights what answers its question", () => {
    const base = { condensed: 0, drift: 0, flying: 0, blocked: 0, charted: false };
    expect(lensSystem("natural", base)).toBe(1);
    expect(lensSystem("flight", { ...base, flying: 1 })).toBeGreaterThan(lensSystem("flight", base));
    expect(lensSystem("drift", { ...base, drift: 50 })).toBeGreaterThan(lensSystem("drift", base));
    expect(lensSystem("progress", { ...base, charted: true, condensed: 1 })).toBeGreaterThan(lensSystem("progress", { ...base, charted: true, condensed: 0.1 }));
    expect(lensStar("flight", { inFlight: true, state: "open", edged: false })).toBeGreaterThan(lensStar("flight", { inFlight: false, state: "closed", edged: true }));
    expect(lensStar("blocked", { inFlight: false, state: "open", edged: true })).toBeGreaterThan(lensStar("blocked", { inFlight: false, state: "open", edged: false }));
  });
});

describe("one galaxy: wayfinding", () => {
  test("a figure joins every star with n-1 short lines", () => {
    const pts = [
      { x: 0, y: 0, z: 0 },
      { x: 1, y: 0, z: 0 },
      { x: 5, y: 0, z: 0 },
      { x: 1, y: 0, z: 1 },
    ];
    const t = spanningTree(pts);
    expect(t).toHaveLength(3);
    const key = t.map(([a, b]) => [Math.min(a, b), Math.max(a, b)].join("-")).sort();
    expect(key).toEqual(["0-1", "1-2", "1-3"]);
    expect(spanningTree([pts[0]])).toEqual([]);
    expect(spanningTree([])).toEqual([]);
  });

  const sky: SkyModel = {
    ageMinutes: 1,
    priorityAgeDays: 0,
    constellations: [
      con("o/switchboard", {
        features: [
          { id: "core", title: "Settings view", release: "1.0", stars: [star(8, { title: "CORE-5: settings view in the app" }), star(12, { title: "Budget panel" })] },
          { id: "sky", title: "The sky", release: "after-1.0", stars: [star(81, { title: "Draw the sky in 3D" })] },
        ],
      }),
      con("o/wavcraft"),
      con("o/skyline"),
    ],
  };

  test("the breadcrumb is as deep as the view", () => {
    expect(breadcrumbs(sky, null, null, null).map((c) => c.level)).toEqual(["galaxy"]);
    expect(breadcrumbs(sky, "o/switchboard", null, null).map((c) => c.label)).toEqual(["Galaxy", "switchboard"]);
    expect(breadcrumbs(sky, "o/switchboard", "sky", null).map((c) => c.label)).toEqual(["Galaxy", "switchboard", "The sky"]);
    const s8 = sky.constellations[0].features[0].stars[0];
    expect(breadcrumbs(sky, "o/switchboard", "sky", s8).map((c) => c.label)).toEqual(["Galaxy", "switchboard", "Settings view", "#8"]);
    expect(breadcrumbs(sky, "o/missing", null, null)).toHaveLength(1);
  });

  test("search finds projects, features and issues, best first", () => {
    expect(searchSky(sky, "")).toEqual([]);
    expect(searchSky(sky, "wav")[0]).toMatchObject({ kind: "project", repo: "o/wavcraft" });
    const sk = searchSky(sky, "sky");
    expect(sk[0]).toMatchObject({ kind: "project", repo: "o/skyline" });
    expect(sk.some((h) => h.kind === "feature" && h.feature === "sky")).toBe(true);
    expect(searchSky(sky, "#12")).toEqual([expect.objectContaining({ kind: "issue", repo: "o/switchboard" })]);
    expect(searchSky(sky, "81")[0]).toMatchObject({ kind: "issue" });
    expect(searchSky(sky, "settings").map((h) => h.kind)).toEqual(["feature", "issue"]);
    expect(searchSky(sky, "o", 2)).toHaveLength(2);
  });
});

describe("one galaxy: lenses and labels", () => {
  const f = (o: Partial<SystemFacts>): SystemFacts => ({ condensed: 0, drift: 0, flying: 0, blocked: 0, charted: false, ...o });
  test("a lens counts the projects it lights, and can honestly light none", () => {
    const facts = [f({ drift: 3 }), f({ charted: true, condensed: 0.5 }), f({ blocked: 2, drift: 1 })];
    expect(lensMatches("natural", facts)).toBe(3);
    expect(lensMatches("drift", facts)).toBe(2);
    expect(lensMatches("progress", facts)).toBe(1);
    expect(lensMatches("blocked", facts)).toBe(1);
    expect(lensMatches("flight", facts)).toBe(0);
  });

  test("a feature name sits on the outer side of its figure", () => {
    // Right of the hub: the label starts clear of the object to its right.
    expect(outsideX({ x: 500, r: 20 }, { x: 400 }, 100, 6)).toBe(500 + 20 + 6 + 50);
    // Left of the hub: mirrored, so it never lies across the bright core.
    expect(outsideX({ x: 300, r: 20 }, { x: 400 }, 100, 6)).toBe(300 - 20 - 6 - 50);
  });
});
