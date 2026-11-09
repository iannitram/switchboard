/// <reference types="vite/client" />
// REP-3 (#48): filter reports by project, outcome and pool. Locked tests (R28): the builder
// never edits this file.
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { vi } from "vitest";
import codeMap from "../../../docs/code-map.md?raw";
import { call } from "../core";
import { FilterBar, NO_FILTER, Reports, type Filter, type Report } from "./Reports";

vi.mock("../core", () => ({ call: vi.fn() }));

function deepFreeze<T>(v: T): T {
  if (v && typeof v === "object") {
    Object.values(v as object).forEach(deepFreeze);
    Object.freeze(v);
  }
  return v;
}

const report = (id: string, project: string | null, outcome: string, pool: string | null): Report => ({
  id,
  title: null,
  project,
  at: null,
  outcome,
  failure_mode: null,
  changed: { pr: null, files: null, lines: null },
  checked: [],
  not_checked: [],
  cost: { pool, tokens: null, usd: null, seconds: null },
  moves: [],
});

const REPORTS: Report[] = deepFreeze([
  report("a1", "owner/app-b", "passed", "qwen"),
  report("a2", "owner/app-a", "failed", "gemini"),
  report("t1", null, "down", null),
]);

const options = (select: HTMLElement) =>
  Array.from((select as HTMLSelectElement).options).map((o) => [o.value, o.textContent]);

const shownIds = () => screen.queryAllByRole("group").map((g) => g.getAttribute("aria-label"));

/** A `call` mock whose answers the test hands back one by one, in any order. */
function pending() {
  const answers: ((v: unknown) => void)[] = [];
  vi.mocked(call).mockImplementation((() => new Promise((r) => answers.push(r))) as never);
  return answers;
}

beforeEach(() => {
  vi.mocked(call).mockReset();
});

test("three labelled selects, each All then the reports' values sorted", () => {
  render(<FilterBar reports={REPORTS} filter={NO_FILTER} onChange={() => {}} />);
  const form = screen.getByRole("form", { name: "Filter reports" });
  expect(options(within(form).getByRole("combobox", { name: "Project" }))).toEqual([
    ["", "All"],
    ["owner/app-a", "owner/app-a"],
    ["owner/app-b", "owner/app-b"],
  ]);
  expect(options(within(form).getByRole("combobox", { name: "Outcome" }))).toEqual([
    ["", "All"],
    ["down", "down"],
    ["failed", "failed"],
    ["passed", "passed"],
  ]);
  expect(options(within(form).getByRole("combobox", { name: "Pool" }))).toEqual([
    ["", "All"],
    ["gemini", "gemini"],
    ["qwen", "qwen"],
  ]);
});

test("each select shows the filter it is given", () => {
  const f: Filter = deepFreeze({ project: "owner/app-a", outcome: null, pool: "qwen" });
  render(<FilterBar reports={REPORTS} filter={f} onChange={() => {}} />);
  expect((screen.getByRole("combobox", { name: "Project" }) as HTMLSelectElement).value).toBe("owner/app-a");
  expect((screen.getByRole("combobox", { name: "Outcome" }) as HTMLSelectElement).value).toBe("");
  expect((screen.getByRole("combobox", { name: "Pool" }) as HTMLSelectElement).value).toBe("qwen");
});

test("choosing a value sets that field only, and All sets it to null", () => {
  const onChange = vi.fn();
  const f: Filter = deepFreeze({ project: "owner/app-a", outcome: null, pool: "qwen" });
  render(<FilterBar reports={REPORTS} filter={f} onChange={onChange} />);
  fireEvent.change(screen.getByRole("combobox", { name: "Outcome" }), { target: { value: "failed" } });
  expect(onChange).toHaveBeenLastCalledWith({ project: "owner/app-a", outcome: "failed", pool: "qwen" });
  fireEvent.change(screen.getByRole("combobox", { name: "Pool" }), { target: { value: "" } });
  expect(onChange).toHaveBeenLastCalledWith({ project: "owner/app-a", outcome: null, pool: null });
});

test("the filter bar shows once there are reports, and not before", async () => {
  vi.mocked(call).mockResolvedValue({ ok: [] } as never);
  const empty = render(<Reports />);
  await act(async () => {});
  expect(screen.queryByRole("form", { name: "Filter reports" })).toBeNull();
  expect(screen.getByText("no data yet")).toBeTruthy();
  empty.unmount();
  const answers = pending();
  render(<Reports />);
  expect(screen.queryByRole("form", { name: "Filter reports" })).toBeNull();
  await act(async () => answers[0]({ ok: REPORTS }));
  expect(screen.getByRole("form", { name: "Filter reports" })).toBeTruthy();
});

test("a chosen filter asks the core and shows what it gives", async () => {
  const answers = pending();
  render(<Reports />);
  await act(async () => answers[0]({ ok: REPORTS }));
  expect(shownIds()).toEqual(["a1", "a2", "t1"]);
  fireEvent.change(screen.getByRole("combobox", { name: "Outcome" }), { target: { value: "failed" } });
  expect(call).toHaveBeenCalledTimes(2);
  expect(call).toHaveBeenLastCalledWith("reports", { filter: { project: null, outcome: "failed", pool: null } });
  await act(async () => answers[1]({ ok: [REPORTS[1]] }));
  expect(shownIds()).toEqual(["a2"]);
});

test("back to All shows every report again without asking", async () => {
  const answers = pending();
  render(<Reports />);
  await act(async () => answers[0]({ ok: REPORTS }));
  fireEvent.change(screen.getByRole("combobox", { name: "Pool" }), { target: { value: "qwen" } });
  await act(async () => answers[1]({ ok: [REPORTS[0]] }));
  expect(shownIds()).toEqual(["a1"]);
  fireEvent.change(screen.getByRole("combobox", { name: "Pool" }), { target: { value: "" } });
  expect(call).toHaveBeenCalledTimes(2);
  expect(shownIds()).toEqual(["a1", "a2", "t1"]);
});

test("a filter nothing matches says so", async () => {
  const answers = pending();
  render(<Reports />);
  await act(async () => answers[0]({ ok: REPORTS }));
  fireEvent.change(screen.getByRole("combobox", { name: "Project" }), { target: { value: "owner/app-a" } });
  await act(async () => answers[1]({ ok: [] }));
  expect(screen.getByText("no reports match this filter")).toBeTruthy();
  expect(screen.queryByText("no data yet")).toBeNull();
  expect(screen.getByRole("form", { name: "Filter reports" })).toBeTruthy();
});

test("an answer for an older filter is ignored", async () => {
  const answers = pending();
  render(<Reports />);
  await act(async () => answers[0]({ ok: REPORTS }));
  fireEvent.change(screen.getByRole("combobox", { name: "Outcome" }), { target: { value: "failed" } });
  fireEvent.change(screen.getByRole("combobox", { name: "Pool" }), { target: { value: "qwen" } });
  expect(call).toHaveBeenLastCalledWith("reports", { filter: { project: null, outcome: "failed", pool: "qwen" } });
  await act(async () => answers[2]({ ok: [] }));
  await act(async () => answers[1]({ ok: [REPORTS[1]] }));
  expect(screen.getByText("no reports match this filter")).toBeTruthy();
  expect(shownIds()).toEqual([]);
});

test("a core error while filtering shows in place", async () => {
  const answers = pending();
  render(<Reports />);
  await act(async () => answers[0]({ ok: REPORTS }));
  fireEvent.change(screen.getByRole("combobox", { name: "Outcome" }), { target: { value: "down" } });
  await act(async () => answers[1]({ error: { command: "reports", lines: ["bad filter"] } }));
  const alert = screen.getByRole("alert");
  expect(within(alert).getByText("reports failed")).toBeTruthy();
  expect(within(alert).getByText("bad filter")).toBeTruthy();
});

test("the code map marks the filter bar present", () => {
  expect(codeMap).toContain("- REP-3 filter bar (`FilterBar`, `Reports`): present.");
});
