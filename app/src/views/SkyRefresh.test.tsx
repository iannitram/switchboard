/// <reference types="vite/client" />
// GH-1 (#240): the Sky's Refresh button calls `refresh_all` and reads the sky again; the view
// reads the sky again on each `snapshot` event from the poll thread. Locked tests (R28): the
// builder never edits this file.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import codeMap from "../../../docs/code-map.md?raw";
import type { Constellation, SkyModel } from "../sky/model";
import { Sky } from "./Sky";

const { call, listen, unlisten } = vi.hoisted(() => ({ call: vi.fn(), listen: vi.fn(), unlisten: vi.fn() }));
vi.mock("../core", () => ({ call }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

function deepFreeze<T>(v: T): T {
  if (v && typeof v === "object") {
    Object.values(v as object).forEach(deepFreeze);
    Object.freeze(v);
  }
  return v;
}

function constellation(repo: string, rank: number): Constellation {
  return {
    repo,
    url: `https://github.com/${repo}`,
    rank,
    radius: 0,
    size: 1,
    angle: 0,
    brightness: 0.5,
    features: [],
    edges: [],
    chain: [],
    drift: [],
    reuses: [],
  };
}

const OLD: SkyModel = deepFreeze({
  ageMinutes: 12,
  priorityAgeDays: 0,
  constellations: [constellation("owner/one", 0), constellation("owner/two", 1)],
});
const FRESH: SkyModel = deepFreeze({ ...OLD, ageMinutes: 0 });
const NO_SNAPSHOT = deepFreeze({ error: { command: "sky", lines: ["GitHub: no snapshot yet"] } });
const NO_TOKEN = deepFreeze({ error: { command: "refresh_all", lines: ["GitHub: no GitHub token: run `gh auth login`"] } });

// `sky` answers each of `skies` in turn (the last one again after that); `refresh_all` answers
// `refresh` (a value, or a function returning the answer's promise).
function answer(skies: unknown[], refresh: unknown = { ok: ["owner/one", "owner/two"] }) {
  let n = 0;
  call.mockImplementation(async (command: string) => {
    if (command === "sky") {
      const a = skies[Math.min(n, skies.length - 1)];
      n += 1;
      return "error" in (a as object) ? a : { ok: a };
    }
    if (command === "refresh_all") return typeof refresh === "function" ? refresh() : refresh;
    return { error: { command, lines: ["not mocked"] } };
  });
}

const skyCalls = () => call.mock.calls.filter((c) => c[0] === "sky").length;
const ages = () => screen.getByTestId("ages").textContent;

beforeEach(() => {
  call.mockReset();
  listen.mockReset();
  unlisten.mockReset();
  listen.mockResolvedValue(unlisten);
});

test("the Refresh button calls refresh_all with no arguments, then reads the sky again and shows its new age", async () => {
  answer([OLD, FRESH]);
  render(<Sky />);
  await screen.findByTestId("ages");
  expect(ages()).toBe("snapshot 12 min old · priority copy 0 days old");
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(ages()).toBe("snapshot 0 min old · priority copy 0 days old"));
  expect(call).toHaveBeenCalledWith("refresh_all");
  expect(call.mock.calls.find((c) => c[0] === "refresh_all")).toEqual(["refresh_all"]);
  expect(skyCalls()).toBe(2);
});

test("while it refreshes the button is disabled and says Refreshing…", async () => {
  let done: (v: unknown) => void = () => {};
  answer([OLD, FRESH], () => new Promise((r) => (done = r)));
  render(<Sky />);
  const button = await screen.findByRole("button", { name: "Refresh" });
  fireEvent.click(button);
  expect((await screen.findByRole("status")).textContent).toBe("Refreshing…");
  expect((button as HTMLButtonElement).disabled).toBe(true);
  await act(async () => done({ ok: [] }));
  await waitFor(() => expect(screen.queryByText("Refreshing…")).toBeNull());
  expect((screen.getByRole("button", { name: "Refresh" }) as HTMLButtonElement).disabled).toBe(false);
});

test("a failed refresh shows its error in place and still reads the sky again", async () => {
  answer([OLD, FRESH], NO_TOKEN);
  render(<Sky />);
  fireEvent.click(await screen.findByRole("button", { name: "Refresh" }));
  const alert = await screen.findByRole("alert", { name: "Refresh outcome" });
  expect(alert.textContent).toBe("refresh_all failed: GitHub: no GitHub token: run `gh auth login`");
  await waitFor(() => expect(skyCalls()).toBe(2));
  expect(ages()).toBe("snapshot 0 min old · priority copy 0 days old");
});

test("a refresh that works after a failed one clears the error", async () => {
  let n = 0;
  answer([OLD], () => Promise.resolve(n++ === 0 ? NO_TOKEN : { ok: ["owner/one"] }));
  render(<Sky />);
  fireEvent.click(await screen.findByRole("button", { name: "Refresh" }));
  await screen.findByRole("alert", { name: "Refresh outcome" });
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(screen.queryByRole("alert", { name: "Refresh outcome" })).toBeNull());
  await waitFor(() => expect(skyCalls()).toBe(3));
});

test("the view listens for snapshot and reads the sky again on each event", async () => {
  answer([OLD, FRESH]);
  render(<Sky />);
  await screen.findByTestId("ages");
  expect(listen).toHaveBeenCalledTimes(1);
  expect(listen.mock.calls[0][0]).toBe("snapshot");
  const onEvent = listen.mock.calls[0][1] as (e: { payload: string[] }) => void;
  await act(async () => onEvent({ payload: ["owner/one"] }));
  await waitFor(() => expect(ages()).toBe("snapshot 0 min old · priority copy 0 days old"));
  expect(skyCalls()).toBe(2);
  await act(async () => onEvent({ payload: [] }));
  await waitFor(() => expect(skyCalls()).toBe(3));
});

test("leaving the view stops listening", async () => {
  answer([OLD]);
  const view = render(<Sky />);
  await screen.findByTestId("ages");
  view.unmount();
  await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
});

test("with no snapshot yet the Refresh button is there, and a refresh that works paints the sky", async () => {
  answer([NO_SNAPSHOT, FRESH]);
  render(<Sky />);
  expect((await screen.findByRole("alert")).textContent).toBe("sky failed: GitHub: no snapshot yet");
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  expect(await screen.findAllByTestId("constellation")).toHaveLength(2);
  expect(ages()).toBe("snapshot 0 min old · priority copy 0 days old");
});

test("an empty sky has the Refresh button too", async () => {
  answer([{ ...OLD, constellations: [] }]);
  render(<Sky />);
  expect(await screen.findByRole("button", { name: "Refresh" })).toBeTruthy();
});

test("without the Tauri runtime listen rejects and the view still paints with its Refresh button", async () => {
  listen.mockReset();
  listen.mockRejectedValue(new Error("no Tauri runtime"));
  answer([OLD]);
  render(<Sky />);
  expect(await screen.findAllByTestId("constellation")).toHaveLength(2);
  expect(screen.getByRole("button", { name: "Refresh" })).toBeTruthy();
});

test("the code map has github-poll present, with live.rs", () => {
  const start = codeMap.indexOf("### github-poll");
  expect(start).toBeGreaterThan(-1);
  const end = codeMap.indexOf("\n### ", start + 1);
  const section = codeMap.slice(start, end);
  expect(section).toContain("`crates/core/src/github/live.rs`");
  expect(section).toContain("· present.");
  expect(section).not.toContain("planned");
});
