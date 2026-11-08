// Locked tests for #43 (SKY-8: the star panel). Written before the build (R28); the builder
// never edits this file.
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { Constellation, SkyModel, Star } from "./model";
import { StarPanel } from "./StarPanel";
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

const ISSUE = "https://github.com/iannitram/switchboard/issues/30";
const PR = "https://github.com/iannitram/switchboard/pull/51";

const FULL: Star = deepFreeze({
  number: 30,
  title: "SKY-1: place each constellation by rank",
  state: "open",
  url: ISSUE,
  labels: ["feature:sky", "size:medium", "kind:pure-fn"],
  size: "medium",
  milestone: "1.0",
  inFlight: true,
  pr: { number: 51, url: PR, state: "open", verdict: "PASS" },
  job: "switchboard-30",
});

const BARE: Star = deepFreeze({
  number: 31,
  title: "SKY-2: brightness",
  state: "open",
  url: "https://github.com/iannitram/switchboard/issues/31",
  labels: [],
  size: null,
  milestone: null,
  inFlight: false,
  pr: null,
  job: null,
});

const C: Constellation = {
  repo: "iannitram/switchboard",
  url: "https://github.com/iannitram/switchboard",
  rank: 0,
  radius: 0,
  size: 1,
  angle: 0,
  brightness: 0,
  features: [{ id: "sky", title: "Constellation", release: "1.0", stars: [FULL, BARE] }],
  edges: [],
  chain: [],
  drift: [],
  reuses: [],
};
const SKY: SkyModel = deepFreeze({ ageMinutes: 1, priorityAgeDays: 0, constellations: [C] });

afterEach(() => call.mockReset());

test("the panel names the star and lists its labels in order", () => {
  render(<StarPanel star={FULL} onClose={() => {}} />);
  const panel = screen.getByRole("complementary", { name: "Star #30" });
  expect(within(panel).getByRole("heading", { name: "#30 SKY-1: place each constellation by rank" })).toBeTruthy();
  const labels = within(panel).getByRole("list", { name: "Labels" });
  expect(within(labels).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "feature:sky",
    "size:medium",
    "kind:pure-fn",
  ]);
});

test("the panel shows size, PR, verdict and job", () => {
  render(<StarPanel star={FULL} onClose={() => {}} />);
  expect(screen.getByText("Size: medium")).toBeTruthy();
  expect(screen.getByRole("link", { name: "PR #51 (open)" }).getAttribute("href")).toBe(PR);
  expect(screen.getByText("Review: PASS")).toBeTruthy();
  expect(screen.getByText("Job: switchboard-30")).toBeTruthy();
  expect(screen.getByRole("link", { name: "Open on GitHub" }).getAttribute("href")).toBe(ISSUE);
});

test("a bare star says what it lacks", () => {
  render(<StarPanel star={BARE} onClose={() => {}} />);
  expect(screen.getByText("Size: not sized")).toBeTruthy();
  expect(screen.getByText("No PR yet")).toBeTruthy();
  expect(screen.getByText("Review: not reviewed")).toBeTruthy();
  expect(screen.getByText("Job: none")).toBeTruthy();
  expect(screen.queryAllByRole("listitem")).toHaveLength(0);
});

test("a PR with no verdict yet is not reviewed", () => {
  render(<StarPanel star={{ ...FULL, pr: { ...FULL.pr!, verdict: null } }} onClose={() => {}} />);
  expect(screen.getByText("Review: not reviewed")).toBeTruthy();
});

test("Close calls onClose", () => {
  const onClose = vi.fn();
  render(<StarPanel star={FULL} onClose={onClose} />);
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("in the Sky view, clicking a star opens its panel and Close shuts it", async () => {
  call.mockResolvedValue({ ok: SKY });
  render(<Sky />);
  fireEvent.click(await screen.findByTestId("constellation"));
  expect(screen.queryByRole("complementary")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "#31 SKY-2: brightness" }));
  expect(screen.getByRole("complementary", { name: "Star #31" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(screen.queryByRole("complementary")).toBeNull();
});
