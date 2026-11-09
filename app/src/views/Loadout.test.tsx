/// <reference types="vite/client" />
// IT-1 (#93): the Loadout panel. The core is stubbed: the panel shows what it returns.
import { act, render, screen, within } from "@testing-library/react";
import { vi } from "vitest";
import { call } from "../core";
import { Loadout, LoadoutPanel, type Loadout as LoadoutData } from "./Loadout";

vi.mock("../core", () => ({ call: vi.fn() }));

const FULL: LoadoutData = {
  read_at: 1_700_000_000,
  clis: [["claude", true], ["codex", false]],
  models_loaded: ["qwen-coder"],
  toolchains: ["cargo", "node"],
  busy: true,
  recent_errors: ["fleet: build failed", "timeout after 30s"],
};

const EMPTY: LoadoutData = {
  read_at: 1_700_000_000, clis: [], models_loaded: [], toolchains: [], busy: false, recent_errors: [],
};

beforeEach(() => {
  vi.mocked(call).mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

test("no loadout yet shows the empty state", () => {
  render(<LoadoutPanel loadout={null} />);
  expect(screen.getByText("no data yet")).toBeTruthy();
});

test("a loadout with no probes shows the empty state", () => {
  render(<LoadoutPanel loadout={EMPTY} />);
  expect(screen.getByText("no data yet")).toBeTruthy();
  expect(screen.queryByRole("list")).toBeNull();
});

test("one row per probe with its name, state and detail", () => {
  render(<LoadoutPanel loadout={FULL} />);
  const list = screen.getByRole("list");
  const rows = within(list).getAllByRole("listitem");
  expect(rows).toHaveLength(8);

  const claude = within(list).getByLabelText("claude");
  expect(claude.textContent).toContain("signed in");
  const codex = within(list).getByLabelText("codex");
  expect(codex.textContent).toContain("not signed in");
  const model = within(list).getByLabelText("qwen-coder");
  expect(model.textContent).toContain("loaded");
  expect(within(list).getByLabelText("cargo").textContent).toContain("installed");
  expect(within(list).getByLabelText("node").textContent).toContain("installed");
  expect(within(list).getByLabelText("fleet status").textContent).toContain("busy");
});

test("an idle machine shows idle", () => {
  render(<LoadoutPanel loadout={{ ...FULL, busy: false }} />);
  expect(screen.getByLabelText("fleet status").textContent).toContain("idle");
});

test("each recent error shows its text, not a blank row", () => {
  render(<LoadoutPanel loadout={FULL} />);
  const rows = screen.getAllByLabelText("recent error");
  expect(rows).toHaveLength(2);
  expect(rows[0].textContent).toContain("fleet: build failed");
  expect(rows[1].textContent).toContain("timeout after 30s");
  rows.forEach((row) => expect(row.textContent?.trim()).not.toBe(""));
});

test("the view loads it_loadout and shows the rows", async () => {
  vi.mocked(call).mockResolvedValue({ ok: FULL });
  render(<Loadout />);
  expect(call).toHaveBeenCalledWith("it_loadout");
  expect(await screen.findByLabelText("claude")).toBeTruthy();
});

test("a core error is shown in place", async () => {
  vi.mocked(call).mockResolvedValue({ error: { command: "it_loadout", lines: ["probe runner gone"] } });
  render(<Loadout />);
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("it_loadout failed");
  expect(alert.textContent).toContain("probe runner gone");
  expect(screen.getByText("no data yet")).toBeTruthy();
});

test("the view reads the loadout again every minute", async () => {
  vi.useFakeTimers();
  vi.mocked(call).mockResolvedValue({ ok: EMPTY });
  render(<Loadout />);
  await act(async () => {});
  expect(call).toHaveBeenCalledTimes(1);
  await act(async () => { vi.advanceTimersByTime(60_000); });
  expect(call).toHaveBeenCalledTimes(2);
});
