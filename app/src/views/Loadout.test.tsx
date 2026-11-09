/// <reference types="vite/client" />
// IT-1 (#102, #205, #244): the Loadout panel. Locked tests (R28): the builder never edits this file.
// The bridge and the event bus are stubbed: the panel shows only what the core's `loadout`
// command returns and what the core's minute timer pushes on the `loadout` event.
import { act, render, screen, within } from "@testing-library/react";
import { vi } from "vitest";
import codeMap from "../../../docs/code-map.md?raw";
import { listen } from "@tauri-apps/api/event";
import { call } from "../core";
import { Loadout, LoadoutPanel, ageText, type Loadout as LoadoutData } from "./Loadout";

vi.mock("../core", () => ({ call: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
const mockCall = vi.mocked(call);
const mockListen = vi.mocked(listen);

function deepFreeze<T>(v: T): T {
  if (v && typeof v === "object") {
    Object.values(v as object).forEach(deepFreeze);
    Object.freeze(v);
  }
  return v;
}

const FULL: LoadoutData = deepFreeze({
  read_at: 1_762_700_000,
  clis: [["claude", true], ["codex", false], ["gemini", false], ["gh", true]],
  models_loaded: ["qwen3-coder-30b"],
  toolchains: ["cargo", "python3"],
  busy: true,
  recent_errors: ["push-failed: remote hung up", "timeout after 30 min"],
});

const NO_PROBES: LoadoutData = deepFreeze({
  read_at: 1_762_700_000, clis: [], models_loaded: [], toolchains: [], busy: false, recent_errors: [],
});

// FULL read a minute later, with codex signed in and fleet idle.
const LATER: LoadoutData = deepFreeze({
  ...FULL,
  read_at: FULL.read_at + 60,
  clis: [["claude", true], ["codex", true], ["gemini", false], ["gh", true]],
  busy: false,
});

const row = (name: string) => screen.getByRole("listitem", { name });
const state = (name: string) => row(name).querySelector('[data-part="state"]')?.textContent;
const detail = (name: string) => row(name).querySelector('[data-part="detail"]')?.textContent ?? "";
const age = () => document.querySelector('[data-part="age"]')?.textContent;

// The handler the view gave `listen` for `loadout`.
function pushed(): (payload: LoadoutData) => void {
  const found = mockListen.mock.calls.find(([name]) => name === "loadout");
  if (!found) throw new Error("the view did not listen for `loadout`");
  const handler = found[1] as (e: { event: string; id: number; payload: LoadoutData }) => void;
  return (payload) => handler({ event: "loadout", id: 1, payload });
}

beforeEach(() => {
  mockCall.mockReset();
  mockListen.mockReset();
  mockListen.mockResolvedValue(() => {});
});

afterEach(() => {
  vi.useRealTimers();
});

test("one row per probe, in the core's order", () => {
  render(<LoadoutPanel loadout={FULL} now={FULL.read_at} />);
  const names = within(screen.getByRole("region", { name: "Loadout" }))
    .getAllByRole("listitem")
    .map((li) => li.getAttribute("aria-label"));
  expect(names).toEqual([
    "claude", "codex", "gemini", "gh", "cargo", "node", "python3", "llama-swap", "fleet status", "fleet errors",
  ]);
});

test("each row shows its name and state", () => {
  render(<LoadoutPanel loadout={FULL} now={FULL.read_at} />);
  expect(row("claude").querySelector("strong")?.textContent).toBe("claude");
  expect(state("claude")).toBe("signed in");
  expect(state("cargo")).toBe("found");
  expect(state("llama-swap")).toBe("1 loaded");
  expect(detail("llama-swap")).toBe("qwen3-coder-30b");
  expect(state("fleet status")).toBe("busy");
});

test("a failed probe shows as failed, never a blank row", () => {
  render(<LoadoutPanel loadout={FULL} now={FULL.read_at} />);
  expect(state("codex")).toBe("not signed in");
  expect(row("codex").className).toBe("failed");
  expect(state("node")).toBe("not found");
  expect(row("node").className).toBe("failed");
  expect(row("claude").className).toBe("");
  for (const li of screen.getAllByRole("listitem")) {
    expect(li.querySelector('[data-part="state"]')?.textContent).not.toBe("");
  }
});

test("idle, no models and no errors each say so", () => {
  render(<LoadoutPanel loadout={{ ...FULL, busy: false, models_loaded: [], recent_errors: [] }} now={FULL.read_at} />);
  expect(state("fleet status")).toBe("idle");
  expect(state("llama-swap")).toBe("no models loaded");
  expect(state("fleet errors")).toBe("none");
  expect(row("fleet errors").querySelector('[data-part="detail"]')).toBeNull();
});

test("recent errors are counted and listed", () => {
  render(<LoadoutPanel loadout={FULL} now={FULL.read_at} />);
  expect(state("fleet errors")).toBe("2 recent");
  expect(detail("fleet errors")).toBe("push-failed: remote hung up\ntimeout after 30 min");
});

test("no probes shows the empty state and no age", () => {
  render(<LoadoutPanel loadout={NO_PROBES} now={NO_PROBES.read_at} />);
  expect(screen.getByText("no data yet")).toBeTruthy();
  expect(screen.queryByRole("listitem")).toBeNull();
  expect(age()).toBeUndefined();
});

test("before the first read the panel is empty", () => {
  render(<LoadoutPanel loadout={null} />);
  expect(screen.getByText("no data yet")).toBeTruthy();
});

test("the age of a read, in whole minutes and hours rounded down", () => {
  const t = 1_762_700_000;
  expect(ageText(t, t)).toBe("read just now");
  expect(ageText(t, t + 59)).toBe("read just now");
  expect(ageText(t, t + 60)).toBe("read 1 min ago");
  expect(ageText(t, t + 125)).toBe("read 2 min ago");
  expect(ageText(t, t + 3599)).toBe("read 59 min ago");
  expect(ageText(t, t + 3600)).toBe("read 1 h ago");
  expect(ageText(t, t + 7300)).toBe("read 2 h ago");
  expect(ageText(t, t - 30)).toBe("read just now");
});

test("the panel shows how long ago the loadout was read, above the rows", () => {
  render(<LoadoutPanel loadout={FULL} now={FULL.read_at + 125} />);
  const section = screen.getByRole("region", { name: "Loadout" });
  const shown = section.querySelector('[data-part="age"]');
  expect(shown?.textContent).toBe("read 2 min ago");
  expect(shown?.compareDocumentPosition(section.querySelector("ul")!)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
});

test("the view reads the core's loadout command and shows its rows", async () => {
  mockCall.mockResolvedValue({ ok: FULL });
  render(<Loadout />);
  expect(mockCall).toHaveBeenCalledWith("loadout");
  expect(await screen.findByRole("listitem", { name: "gh" })).toBeTruthy();
});

test("a core error is shown in place", async () => {
  mockCall.mockResolvedValue({ error: { command: "loadout", lines: ["probe runner gone"] } });
  render(<Loadout />);
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("loadout failed");
  expect(alert.textContent).toContain("probe runner gone");
});

test("the view listens for the loadout event", async () => {
  mockCall.mockResolvedValue({ ok: FULL });
  render(<Loadout />);
  await act(async () => {});
  expect(mockListen).toHaveBeenCalledTimes(1);
  expect(mockListen.mock.calls[0][0]).toBe("loadout");
});

test("a pushed loadout redraws the panel", async () => {
  mockCall.mockResolvedValue({ ok: FULL });
  render(<Loadout />);
  await screen.findByRole("listitem", { name: "gh" });
  expect(state("codex")).toBe("not signed in");
  act(() => pushed()(LATER));
  expect(state("codex")).toBe("signed in");
  expect(state("fleet status")).toBe("idle");
});

test("a pushed loadout clears an error shown in place", async () => {
  mockCall.mockResolvedValue({ error: { command: "loadout", lines: ["probe runner gone"] } });
  render(<Loadout />);
  await screen.findByRole("alert");
  act(() => pushed()(FULL));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(row("claude")).toBeTruthy();
});

test("the view runs no read timer of its own: the core's minute timer pushes", async () => {
  vi.useFakeTimers();
  mockCall.mockResolvedValue({ ok: NO_PROBES });
  render(<Loadout />);
  await act(async () => {});
  expect(mockCall).toHaveBeenCalledTimes(1);
  await act(async () => { vi.advanceTimersByTime(5 * 60_000); });
  expect(mockCall).toHaveBeenCalledTimes(1);
});

test("the age on the view moves on with the clock between pushes", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(FULL.read_at * 1000 + 10_000);
  mockCall.mockResolvedValue({ ok: FULL });
  render(<Loadout />);
  await act(async () => {});
  expect(age()).toBe("read just now");
  await act(async () => { vi.advanceTimersByTime(5 * 60_000); });
  expect(age()).toBe("read 5 min ago");
  act(() => pushed()({ ...FULL, read_at: FULL.read_at + 300 }));
  expect(age()).toBe("read just now");
});

test("leaving the view stops listening", async () => {
  const unlisten = vi.fn();
  mockListen.mockResolvedValue(unlisten);
  mockCall.mockResolvedValue({ ok: FULL });
  const { unmount } = render(<Loadout />);
  await act(async () => {});
  unmount();
  await act(async () => {});
  expect(unlisten).toHaveBeenCalledTimes(1);
});

test("without the Tauri runtime listen rejects and the view still shows its read", async () => {
  mockListen.mockRejectedValue(new Error("no Tauri runtime"));
  mockCall.mockResolvedValue({ ok: FULL });
  render(<Loadout />);
  expect(await screen.findByRole("listitem", { name: "gh" })).toBeTruthy();
});

test("the code map marks IT-1's timer present", () => {
  const it = codeMap.split("\n### ").find((s) => s.startsWith("it\n"));
  expect(it).toBeDefined();
  expect(it!.split("\n")).toContain(
    "- IT-1 loadout every minute (`read_and_store`, `latest_loadout`, `current_loadout`, the `loadout` event): present.",
  );
});
