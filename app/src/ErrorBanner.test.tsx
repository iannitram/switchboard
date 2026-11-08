// SHELL-3 (#13): the error banner shows the newest core or fleet error in place. Locked test (R28).
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { App } from "./App";
import { ErrorBanner, fromFleetError } from "./ErrorBanner";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

function deepFreeze<T>(value: T): T {
  if (value && typeof value === "object") {
    Object.values(value).forEach(deepFreeze);
    Object.freeze(value);
  }
  return value;
}

function answer(ok: Record<string, unknown>, fail: Record<string, unknown> = {}) {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd in fail) throw fail[cmd];
    if (cmd in ok) return ok[cmd];
    return [];
  });
}

const fleetCalls = () => mockInvoke.mock.calls.filter(([cmd]) => cmd === "fleet_errors").length;

const BUDGET_ROW = deepFreeze({
  command: ["python3", "-I", "/p/tools/fleet/fleet.py", "budget", "--json"],
  exit_code: 2,
  stderr: "Traceback (most recent call last):\nKeyError: 'pools'\n",
});
const SPAWN_ROW = deepFreeze({ command: ["python3", "-I", "/p/tools/fleet/fleet.py", "ledger", "--json"], exit_code: null, stderr: "" });

beforeEach(() => {
  mockInvoke.mockReset();
});

// fromFleetError
test("a fleet row becomes the command joined by spaces, its exit code, then its stderr lines", () => {
  expect(fromFleetError(BUDGET_ROW)).toEqual({
    command: "python3 -I /p/tools/fleet/fleet.py budget --json",
    lines: ["exit code 2", "Traceback (most recent call last):", "KeyError: 'pools'"],
  });
});

test("a fleet row with no exit code says it did not start, and empty stderr adds no lines", () => {
  expect(fromFleetError(SPAWN_ROW)).toEqual({
    command: "python3 -I /p/tools/fleet/fleet.py ledger --json",
    lines: ["did not start"],
  });
});

test("only trailing newlines are dropped from stderr; blank lines inside stay", () => {
  const row = deepFreeze({ command: ["x"], exit_code: 1, stderr: "a\n\nb\n\n\n" });
  expect(fromFleetError(row).lines).toEqual(["exit code 1", "a", "", "b"]);
});

// ErrorBanner
test("no errors renders nothing", () => {
  const { container } = render(<ErrorBanner errors={deepFreeze([])} />);
  expect(container.firstChild).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
});

test("the banner shows the first error's command and its first five lines", () => {
  const errors = deepFreeze([
    { command: "python3 -I fleet.py budget --json", lines: ["l1", "l2", "l3", "l4", "l5", "l6", "l7"] },
  ]);
  render(<ErrorBanner errors={errors} />);
  const alert = screen.getByRole("alert", { name: "Errors" });
  expect(alert.querySelector("strong")?.textContent).toBe("python3 -I fleet.py budget --json");
  expect(alert.querySelector("pre")?.textContent).toBe("l1\nl2\nl3\nl4\nl5");
});

test("an error with no lines says (no output)", () => {
  render(<ErrorBanner errors={deepFreeze([{ command: "fleet_errors", lines: [] }])} />);
  expect(screen.getByRole("alert").querySelector("pre")?.textContent).toBe("(no output)");
});

test("the first error in the list is the one shown (the list is newest first)", () => {
  const errors = deepFreeze([
    { command: "newest", lines: ["n"] },
    { command: "older", lines: ["o"] },
  ]);
  render(<ErrorBanner errors={errors} />);
  expect(screen.getByRole("alert").querySelector("strong")?.textContent).toBe("newest");
});

test("one error has no list button", () => {
  render(<ErrorBanner errors={deepFreeze([{ command: "only", lines: ["x"] }])} />);
  expect(screen.queryByRole("button")).toBeNull();
});

test("several errors give a button that opens and closes the full list", () => {
  const errors = deepFreeze([
    { command: "c1", lines: ["first of c1", "second"] },
    { command: "c2", lines: [] },
    { command: "c3", lines: ["first of c3"] },
  ]);
  render(<ErrorBanner errors={errors} />);
  expect(screen.queryByRole("list", { name: "All errors" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Show all 3 errors" }));
  const list = screen.getByRole("list", { name: "All errors" });
  expect(within(list).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "c1: first of c1",
    "c2: (no output)",
    "c3: first of c3",
  ]);
  fireEvent.click(screen.getByRole("button", { name: "Hide the list" }));
  expect(screen.queryByRole("list", { name: "All errors" })).toBeNull();
  expect(screen.getByRole("button", { name: "Show all 3 errors" })).toBeTruthy();
});

// App
test("the window shows the newest fleet error from the core", async () => {
  answer({ settings_missing: [], fleet_errors: deepFreeze([BUDGET_ROW, SPAWN_ROW]) });
  render(<App />);
  const alert = await screen.findByRole("alert", { name: "Errors" });
  expect(alert.querySelector("strong")?.textContent).toBe("python3 -I /p/tools/fleet/fleet.py budget --json");
  expect(alert.querySelector("pre")?.textContent).toBe("exit code 2\nTraceback (most recent call last):\nKeyError: 'pools'");
});

test("no banner when the fleet error log is empty", async () => {
  answer({ settings_missing: [], fleet_errors: [] });
  render(<App />);
  await waitFor(() => expect(fleetCalls()).toBe(1));
  await Promise.resolve();
  expect(screen.queryByRole("alert")).toBeNull();
});

test("when the fleet_errors command itself fails, the banner shows that error", async () => {
  answer({ settings_missing: [] }, deepFreeze({ fleet_errors: { command: "fleet_errors", lines: ["state not managed"] } }));
  render(<App />);
  const alert = await screen.findByRole("alert", { name: "Errors" });
  expect(alert.querySelector("strong")?.textContent).toBe("fleet_errors");
  expect(alert.querySelector("pre")?.textContent).toBe("state not managed");
});

test("the error log is read again each time the view changes", async () => {
  answer({ settings_missing: [], fleet_errors: [] });
  render(<App />);
  await waitFor(() => expect(fleetCalls()).toBe(1));
  answer({ settings_missing: [], fleet_errors: deepFreeze([BUDGET_ROW]) });
  fireEvent.click(screen.getByRole("button", { name: "Budget" }));
  await waitFor(() => expect(fleetCalls()).toBe(2));
  expect(await screen.findByRole("alert", { name: "Errors" })).toBeTruthy();
});
