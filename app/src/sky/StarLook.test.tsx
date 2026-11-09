// Locked tests for #39 (SKY-4: open, closed and in-flight stars). Written before the build
// (R28); the builder never edits this file.
import { render, screen } from "@testing-library/react";
import type { Constellation, Star } from "./model";
import { CLOSED_OPACITY, OPEN_OPACITY, starLook } from "./stars";
import { ConstellationView } from "./ConstellationView";

function deepFreeze<T>(o: T): T {
  if (o && typeof o === "object" && !Object.isFrozen(o)) {
    Object.freeze(o);
    for (const v of Object.values(o as object)) deepFreeze(v);
  }
  return o;
}

function star(number: number, state: "open" | "closed", inFlight: boolean): Star {
  return {
    number,
    title: `issue ${number}`,
    state,
    url: `https://github.com/iannitram/switchboard/issues/${number}`,
    labels: [],
    size: null,
    milestone: "1.0",
    inFlight,
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
  brightness: 0,
  features: [
    {
      id: "sky",
      title: "Constellation",
      release: "1.0",
      stars: [star(1, "open", false), star(2, "closed", false), star(3, "open", true)],
    },
  ],
  edges: [],
  chain: [],
  drift: [],
  reuses: [],
});

function drawn(n: number): Element {
  return screen.getAllByTestId("star").find((s) => s.getAttribute("data-number") === String(n))!;
}

test("open is 0.35 and closed is 1", () => {
  expect(OPEN_OPACITY).toBe(0.35);
  expect(CLOSED_OPACITY).toBe(1);
});

test("an open star is dim and still", () => {
  expect(starLook({ state: "open", inFlight: false })).toEqual({ opacity: 0.35, pulse: false });
});

test("a closed star is full and still", () => {
  expect(starLook({ state: "closed", inFlight: false })).toEqual({ opacity: 1, pulse: false });
});

test("a star with a job in flight pulses, open or closed", () => {
  expect(starLook({ state: "open", inFlight: true })).toEqual({ opacity: 0.35, pulse: true });
  expect(starLook({ state: "closed", inFlight: true })).toEqual({ opacity: 1, pulse: true });
});

test("the zoomed view draws each star at its look", () => {
  render(<ConstellationView c={C} onBack={() => {}} />);
  expect(drawn(1).getAttribute("fill-opacity")).toBe("0.35");
  expect(drawn(2).getAttribute("fill-opacity")).toBe("1");
  expect(drawn(1).getAttribute("data-state")).toBe("open");
  expect(drawn(2).getAttribute("data-state")).toBe("closed");
});

test("only the in-flight star pulses", () => {
  render(<ConstellationView c={C} onBack={() => {}} />);
  expect(drawn(3).getAttribute("data-pulse")).toBe("true");
  expect(drawn(1).getAttribute("data-pulse")).toBeNull();
  expect(drawn(2).getAttribute("data-pulse")).toBeNull();
  const pulses = screen.getAllByTestId("pulse");
  expect(pulses).toHaveLength(1);
  expect(drawn(3).contains(pulses[0])).toBe(true);
  expect(pulses[0].getAttribute("attributeName")).toBe("r");
  expect(pulses[0].getAttribute("values")).toBe("10;14;10");
  expect(pulses[0].getAttribute("dur")).toBe("1.5s");
  expect(pulses[0].getAttribute("repeatCount")).toBe("indefinite");
});
