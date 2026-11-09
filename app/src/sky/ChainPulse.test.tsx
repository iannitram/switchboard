// SKY-7 on a pulsing star (SKY-4): hovering a star with a job in flight lights its chain too.
// Added in review of #42; the locked Chain.test.tsx is not touched.
import { fireEvent, render, screen } from "@testing-library/react";
import type { Constellation, Star } from "./model";
import { ConstellationView } from "./ConstellationView";

function star(number: number, inFlight = false): Star {
  return {
    number,
    title: `issue ${number}`,
    state: "open",
    url: `https://github.com/iannitram/switchboard/issues/${number}`,
    labels: [],
    size: null,
    milestone: "1.0",
    inFlight,
    pr: null,
    job: null,
  };
}

const C: Constellation = {
  repo: "iannitram/switchboard",
  url: "https://github.com/iannitram/switchboard",
  rank: 0,
  radius: 0,
  size: 1,
  angle: 0,
  brightness: 0,
  features: [{ id: "sky", title: "Constellation", release: "1.0", stars: [star(1), star(2, true), star(3)] }],
  edges: [
    [1, 2],
    [2, 3],
  ],
  chain: [1, 2, 3],
  drift: [],
  reuses: [],
};

const flagged = (attr: string) =>
  screen
    .getAllByTestId("star")
    .filter((s) => s.getAttribute(attr) === "true")
    .map((s) => Number(s.getAttribute("data-number")))
    .sort((a, b) => a - b);

test("hovering a pulsing star lights its chain and leaving clears it", () => {
  render(<ConstellationView c={C} onBack={() => {}} />);
  const pulsing = screen.getByRole("button", { name: "#2 issue 2" });
  expect(pulsing.getAttribute("data-pulse")).toBe("true");
  fireEvent.mouseEnter(pulsing);
  expect(flagged("data-highlight")).toEqual([1, 2, 3]);
  expect(flagged("data-chain")).toEqual([1, 2, 3]);
  fireEvent.mouseLeave(pulsing);
  expect(flagged("data-highlight")).toEqual([]);
});

test("hovering a plain star next to a pulsing one lights the same chain", () => {
  render(<ConstellationView c={C} onBack={() => {}} />);
  fireEvent.mouseEnter(screen.getByRole("button", { name: "#3 issue 3" }));
  expect(flagged("data-highlight")).toEqual([1, 2, 3]);
});
