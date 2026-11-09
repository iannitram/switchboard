// IT-1 (#102): the Loadout panel. The bridge is stubbed: the panel shows only what the core's
// `loadout` command returns (#205).
import { act, render, screen, within } from "@testing-library/react";
import { vi } from "vitest";
import { call } from "../core";
import { Loadout, LoadoutPanel, type Loadout as LoadoutData } from "./Loadout";

vi.mock("../core", () => ({ call: vi.fn() }));
const mockCall = vi.mocked(call);

const FULL: LoadoutData = {
  read_at: 1_762_700_000,
  clis: [["claude", true], ["codex", false], ["gemini", false], ["gh", true]],
  models_loaded: ["qwen3-coder-30b"],
  toolchains: ["cargo", "python3"],
  busy: true,
  recent_errors: ["push-failed: remote hung up", "timeout after 30 min"],
};

const NO_PROBES: LoadoutData = {
  read_at: 1_762_700_000, clis: [], models_loaded: [], toolchains: [], busy: false, recent_errors: [],
};

const row = (name: string) => screen.getByRole("listitem", { name });
const state = (name: string) => row(name).querySelector('[data-part="state"]')?.textContent;
const detail = (name: string) => row(name).querySelector('[data-part="detail"]')?.textContent ?? "";

beforeEach(() => {
  mockCall.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

test("one row per probe, in the core's order", () => {
  render(<LoadoutPanel loadout={FULL} />);
  const names = within(screen.getByRole("region", { name: "Loadout" }))
    .getAllByRole("listitem")
    .map((li) => li.getAttribute("aria-label"));
  expect(names).toEqual([
    "claude", "codex", "gemini", "gh", "cargo", "node", "python3", "llama-swap", "fleet status", "fleet errors",
  ]);
});

test("each row shows its name and state", () => {
  render(<LoadoutPanel loadout={FULL} />);
  expect(row("claude").querySelector("strong")?.textContent).toBe("claude");
  expect(state("claude")).toBe("signed in");
  expect(state("cargo")).toBe("found");
  expect(state("llama-swap")).toBe("1 loaded");
  expect(detail("llama-swap")).toBe("qwen3-coder-30b");
  expect(state("fleet status")).toBe("busy");
});

test("a failed probe shows as failed, never a blank row", () => {
  render(<LoadoutPanel loadout={FULL} />);
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
  render(<LoadoutPanel loadout={{ ...FULL, busy: false, models_loaded: [], recent_errors: [] }} />);
  expect(state("fleet status")).toBe("idle");
  expect(state("llama-swap")).toBe("no models loaded");
  expect(state("fleet errors")).toBe("none");
  expect(row("fleet errors").querySelector('[data-part="detail"]')).toBeNull();
});

test("recent errors are counted and listed", () => {
  render(<LoadoutPanel loadout={FULL} />);
  expect(state("fleet errors")).toBe("2 recent");
  expect(detail("fleet errors")).toBe("push-failed: remote hung up\ntimeout after 30 min");
});

test("no probes shows the empty state", () => {
  render(<LoadoutPanel loadout={NO_PROBES} />);
  expect(screen.getByText("no data yet")).toBeTruthy();
  expect(screen.queryByRole("listitem")).toBeNull();
});

test("before the first read the panel is empty", () => {
  render(<LoadoutPanel loadout={null} />);
  expect(screen.getByText("no data yet")).toBeTruthy();
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

test("a loadout on the screen stays when a later read fails", async () => {
  vi.useFakeTimers();
  mockCall.mockResolvedValueOnce({ ok: FULL });
  mockCall.mockResolvedValueOnce({ error: { command: "loadout", lines: ["busy"] } });
  render(<Loadout />);
  await act(async () => {});
  await act(async () => { vi.advanceTimersByTime(60_000); });
  expect(screen.getByRole("alert").textContent).toContain("loadout failed");
  expect(screen.getByRole("listitem", { name: "claude" })).toBeTruthy();
});

test("the view reads the loadout again every minute", async () => {
  vi.useFakeTimers();
  mockCall.mockResolvedValue({ ok: NO_PROBES });
  render(<Loadout />);
  await act(async () => {});
  expect(mockCall).toHaveBeenCalledTimes(1);
  await act(async () => { vi.advanceTimersByTime(60_000); });
  expect(mockCall).toHaveBeenCalledTimes(2);
});
