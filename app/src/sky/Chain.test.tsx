// Locked tests for #42 (SKY-7: light a star's chain and show the longest one). Written before
// the build (R28); the builder never edits this file.
import { fireEvent, render, screen } from "@testing-library/react";
import type { Constellation, Star } from "./model";
import { highlightChain } from "./chain";
import { ConstellationView } from "./ConstellationView";

function deepFreeze<T>(o: T): T {
  if (o && typeof o === "object" && !Object.isFrozen(o)) {
    Object.freeze(o);
    for (const v of Object.values(o as object)) deepFreeze(v);
  }
  return o;
}

function star(number: number): Star {
  return {
    number,
    title: `issue ${number}`,
    state: "open",
    url: `https://github.com/iannitram/switchboard/issues/${number}`,
    labels: [],
    size: null,
    milestone: "1.0",
    inFlight: false,
    pr: null,
    job: null,
  };
}

// 1 blocks 2, 2 blocks 3, 4 blocks 3, 5 stands alone. [blocker, blocked].
const EDGES: [number, number][] = deepFreeze([
  [1, 2],
  [2, 3],
  [4, 3],
]);

const C: Constellation = deepFreeze({
  repo: "iannitram/switchboard",
  url: "https://github.com/iannitram/switchboard",
  rank: 0,
  radius: 0,
  size: 1,
  angle: 0,
  brightness: 0,
  features: [{ id: "sky", title: "Constellation", release: "1.0", stars: [1, 2, 3, 4, 5].map(star) }],
  edges: EDGES,
  chain: [1, 2, 3],
  drift: [],
  reuses: [],
});

const sorted = (s: Set<number>) => [...s].sort((a, b) => a - b);
const flagged = (attr: string) =>
  screen
    .getAllByTestId("star")
    .filter((s) => s.getAttribute(attr) === "true")
    .map((s) => Number(s.getAttribute("data-number")));

test("focus on the middle lights everything up and down from it", () => {
  expect(sorted(highlightChain(EDGES, 2))).toEqual([1, 2, 3]);
});

test("focus on the end lights both of its branches", () => {
  expect(sorted(highlightChain(EDGES, 3))).toEqual([1, 2, 3, 4]);
});

test("focus on a start lights only what it leads to", () => {
  expect(sorted(highlightChain(EDGES, 4))).toEqual([3, 4]);
});

test("a star with no edges lights only itself", () => {
  expect(sorted(highlightChain(EDGES, 5))).toEqual([5]);
});

test("a cycle ends", () => {
  expect(
    sorted(
      highlightChain(
        [
          [10, 11],
          [11, 12],
          [12, 10],
        ],
        11,
      ),
    ),
  ).toEqual([10, 11, 12]);
});

test("hovering a star lights its chain and leaving clears it", () => {
  render(<ConstellationView c={C} onBack={() => {}} />);
  expect(flagged("data-highlight")).toEqual([]);
  fireEvent.mouseEnter(screen.getByRole("button", { name: "#3 issue 3" }));
  expect(flagged("data-highlight")).toEqual([1, 2, 3, 4]);
  fireEvent.mouseLeave(screen.getByRole("button", { name: "#3 issue 3" }));
  expect(flagged("data-highlight")).toEqual([]);
});

test("the longest chain is marked and counted", () => {
  render(<ConstellationView c={C} onBack={() => {}} />);
  expect(flagged("data-chain")).toEqual([1, 2, 3]);
  expect(screen.getByTestId("chain-count").textContent).toBe("Longest chain: 3 stars");
});

test("no chain shows no count", () => {
  render(<ConstellationView c={{ ...C, chain: [] }} onBack={() => {}} />);
  expect(screen.queryByTestId("chain-count")).toBeNull();
  expect(flagged("data-chain")).toEqual([]);
});
