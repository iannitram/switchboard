import { describe, expect, test } from "vitest";
import type { Constellation, Star } from "../sky/model";
import { lit, matchSky, NO_FILTER, WINK_MS, WINK_STAGGER_MS, winkDelay, winkedOut, winkLook, winkSeed, winkStep } from "./wink";

const star = (number: number, title = `Issue ${number}`): Star => ({ number, title, state: "open", url: "", labels: [], size: null, milestone: null, inFlight: false, pr: null, job: null });
const con = (repo: string, features: Constellation["features"] = []): Constellation => ({
  repo,
  url: "",
  rank: 0,
  radius: 0.5,
  size: 0.5,
  angle: 0,
  brightness: 0,
  features,
  edges: [],
  chain: [],
  drift: [],
  reuses: [],
});

const sky = [
  con("iannitram/switchboard", [
    { id: "f1", title: "The front door chat", release: "1.0", stars: [star(133, "Chat input box"), star(140)] },
    { id: "f2", title: "Reports", release: "1.0", stars: [star(12, "Weekly report"), star(13, "Front page")] },
  ]),
  con("iannitram/wordcraft"),
  con("iannitram/wavcraft"),
  con("other/AnyPS5"),
];

describe("live sky search: what stays lit", () => {
  test("an empty or blank query filters nothing", () => {
    expect(matchSky(sky, "")).toBe(NO_FILTER);
    expect(matchSky(sky, "   ")).toBe(NO_FILTER);
    expect(lit(NO_FILTER.repos, "anything")).toBe(true);
  });

  test("project names match case-insensitively on a substring, owner or not", () => {
    const f = matchSky(sky, "CRAFT");
    expect([...f.repos!].sort()).toEqual(["iannitram/wavcraft", "iannitram/wordcraft"]);
    expect(f.count).toBe(2);
    expect(matchSky(sky, "other/any").repos).toEqual(new Set(["other/AnyPS5"]));
    expect(matchSky(sky, "switch").repos).toEqual(new Set(["iannitram/switchboard"]));
    const none = matchSky(sky, "zzz");
    expect(none.repos!.size).toBe(0);
    expect(none.count).toBe(0);
    expect(lit(none.repos, "iannitram/wordcraft")).toBe(false);
  });

  test("inside a project: feature titles and issue titles or numbers, the project stays lit", () => {
    const open = sky[0];
    const byTitle = matchSky(sky, "front", open);
    expect(byTitle.repos!.has(open.repo)).toBe(true);
    // "The front door chat" lights its whole figure; "Front page" lights its star and feature.
    expect([...byTitle.stars!].sort((a, b) => a - b)).toEqual([13, 133, 140]);
    expect([...byTitle.features!].sort()).toEqual(["f1", "f2"]);
    const byNum = matchSky(sky, "#13", open);
    expect([...byNum.stars!].sort((a, b) => a - b)).toEqual([13, 133]);
    expect(byNum.features).toEqual(new Set(["f1", "f2"]));
    const one = matchSky(sky, "weekly", open);
    expect(one.stars).toEqual(new Set([12]));
    expect(one.features).toEqual(new Set(["f2"]));
    expect(matchSky(sky, "nothing like it", open).count).toBe(0);
  });
});

describe("the wink: a flicker and a shrink, staggered", () => {
  test("the stagger is seeded: the same id waits the same time, ids differ, all within bounds", () => {
    expect(winkSeed("a/b")).toBe(winkSeed("a/b"));
    const delays = sky.map((c) => winkDelay(c.repo));
    for (const d of delays) {
      expect(d).toBeGreaterThanOrEqual(0);
      expect(d).toBeLessThan(WINK_STAGGER_MS);
    }
    expect(new Set(delays).size).toBe(delays.length);
  });

  test("waits out its stagger, then goes out in about 300 ms, and comes back the same way", () => {
    expect(winkStep(0, true, 16, 50, 80)).toBe(0);
    let p = 0;
    let held = 0;
    for (let i = 0; i < 200 && p < 1; i++) {
      held += 16;
      p = winkStep(p, true, 16, held, 40);
    }
    expect(p).toBe(1);
    expect(held).toBeGreaterThanOrEqual(40 + WINK_MS);
    expect(held).toBeLessThanOrEqual(40 + WINK_MS + 32);
    held = 0;
    for (let i = 0; i < 200 && p > 0; i++) {
      held += 16;
      p = winkStep(p, false, 16, held, 0);
    }
    expect(p).toBe(0);
    expect(held).toBeLessThanOrEqual(WINK_MS + 16);
  });

  test("reduced motion: no tween, no flicker, just there or not", () => {
    expect(winkStep(0, true, 16, 0, 100, true)).toBe(1);
    expect(winkStep(1, false, 16, 0, 100, true)).toBe(0);
    expect(winkLook(0.3, 0.2, true)).toEqual({ alpha: 1, scale: 1 });
    expect(winkLook(0.7, 0.2, true)).toEqual({ alpha: 0, scale: 0 });
  });

  test("it flickers as it shrinks: not a uniform dissolve, gone at the end", () => {
    expect(winkLook(0)).toEqual({ alpha: 1, scale: 1 });
    expect(winkLook(1)).toEqual({ alpha: 0, scale: 0 });
    const a = Array.from({ length: 41 }, (_, i) => winkLook(i / 40, 0.37).alpha);
    // Somewhere it dips and recovers: the twinkle.
    expect(a.some((v, i) => i > 0 && i < 40 && v > a[i - 1] + 0.01)).toBe(true);
    // The size only ever falls.
    const s = Array.from({ length: 41 }, (_, i) => winkLook(i / 40, 0.37).scale);
    expect(s.every((v, i) => i === 0 || v <= s[i - 1])).toBe(true);
    expect(winkLook(NaN)).toEqual({ alpha: 1, scale: 1 });
    expect(winkedOut(0.49)).toBe(false);
    expect(winkedOut(0.5)).toBe(true);
  });
});
