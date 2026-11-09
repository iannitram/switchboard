// SHELL-1 (#9): the window is wired to the core through Tauri commands. Locked test (R28);
// CORE-5 added Settings, last; IT-1 added Loadout just before it, the sixth of seven tabs.
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { App } from "./App";
import { fleetErrors, settingsMissing } from "./core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

function deepFreeze<T>(value: T): T {
  if (value && typeof value === "object") {
    Object.values(value).forEach(deepFreeze);
    Object.freeze(value);
  }
  return value;
}

// Answers by command name; a value under `fail` is thrown the way Tauri rejects.
function answer(ok: Record<string, unknown>, fail: Record<string, unknown> = {}) {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd in fail) throw fail[cmd];
    if (cmd in ok) return ok[cmd];
    return [];
  });
}

const settingsCalls = () => mockInvoke.mock.calls.filter(([cmd]) => cmd === "settings_missing").length;

beforeEach(() => {
  mockInvoke.mockReset();
});

test("the tabs are the seven views in order, Loadout before Settings", () => {
  answer({ settings_missing: [] });
  render(<App />);
  const nav = screen.getByRole("navigation", { name: "Views" });
  const names = within(nav).getAllByRole("button").map((b) => b.textContent);
  expect(names).toEqual(["Sky", "Waiting on you", "Reports", "Budget", "Chat", "Loadout", "Settings"]);
});

test("Sky is the view shown first", () => {
  answer({ settings_missing: [] });
  render(<App />);
  expect(screen.getByRole("button", { name: "Sky" }).getAttribute("aria-pressed")).toBe("true");
  expect(screen.getByRole("region", { name: "Sky" })).toBeTruthy();
});

test("pressing a tab shows that view, empty, and marks only that tab pressed", () => {
  answer({ settings_missing: [] });
  render(<App />);
  fireEvent.click(screen.getByRole("button", { name: "Budget" }));
  expect(screen.getByRole("region", { name: "Budget" })).toBeTruthy();
  expect(screen.getByText("no data yet")).toBeTruthy();
  const pressed = screen.getAllByRole("button").filter((b) => b.getAttribute("aria-pressed") === "true");
  expect(pressed.map((b) => b.textContent)).toEqual(["Budget"]);
});

test("missing settings are named in one status line, in the core's order", async () => {
  answer(deepFreeze({ settings_missing: ["pipeline_repo", "vault_agents"] }));
  render(<App />);
  const status = await screen.findByRole("status");
  expect(status.textContent).toBe("Settings needed: pipeline_repo, vault_agents");
});

test("no status line when nothing is missing", async () => {
  answer({ settings_missing: [] });
  render(<App />);
  await waitFor(() => expect(settingsCalls()).toBe(1));
  await Promise.resolve();
  expect(screen.queryByRole("status")).toBeNull();
});

test("a settings error from the core is shown in place, not dropped", async () => {
  answer(
    {},
    deepFreeze({ settings_missing: { command: "settings_missing", lines: ["parse: expected value at line 1", "second"] } }),
  );
  render(<App />);
  const status = await screen.findByRole("status");
  expect(status.textContent).toBe("Settings could not be read: parse: expected value at line 1");
});

test("a settings error with no output says so", async () => {
  answer({}, { settings_missing: { command: "settings_missing", lines: [] } });
  render(<App />);
  const status = await screen.findByRole("status");
  expect(status.textContent).toBe("Settings could not be read: no output");
});

test("the settings are asked for once, when the window opens, not on every tab", async () => {
  answer({ settings_missing: [] });
  render(<App />);
  await waitFor(() => expect(settingsCalls()).toBe(1));
  fireEvent.click(screen.getByRole("button", { name: "Reports" }));
  fireEvent.click(screen.getByRole("button", { name: "Chat" }));
  await Promise.resolve();
  expect(settingsCalls()).toBe(1);
});

test("settingsMissing calls the settings_missing command", async () => {
  answer(deepFreeze({ settings_missing: ["nextcloud_backups"] }));
  expect(await settingsMissing()).toEqual({ ok: ["nextcloud_backups"] });
  expect(mockInvoke).toHaveBeenCalledWith("settings_missing", undefined);
});

test("fleetErrors returns the rows the fleet_errors command gives", async () => {
  const rows = deepFreeze([{ command: ["python3", "-I", "fleet.py", "budget", "--json"], exit_code: 2, stderr: "boom\n" }]);
  answer({ fleet_errors: rows });
  expect(await fleetErrors()).toEqual({ ok: rows });
  expect(mockInvoke).toHaveBeenCalledWith("fleet_errors", undefined);
});

test("a rejected command comes back as an error with its name and first lines", async () => {
  answer({}, { fleet_errors: "line 1\nline 2\nline 3\nline 4\nline 5\nline 6" });
  expect(await fleetErrors()).toEqual({
    error: { command: "fleet_errors", lines: ["line 1", "line 2", "line 3", "line 4", "line 5"] },
  });
});
