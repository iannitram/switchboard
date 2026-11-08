/// <reference types="vite/client" />
// REP-1 (#45): the Reports view and the report entry ported from BroCrew. Locked tests (R28):
// the builder never edits this file.
import { act, render, screen, within } from "@testing-library/react";
import { vi } from "vitest";
import codeMap from "../../../docs/code-map.md?raw";
import notice from "../../../NOTICE?raw";
import entrySource from "../brocrew/ReportEntry.tsx?raw";
import { call } from "../core";
import { REPORT_MARK, ReportEntry, duration, reportTime } from "../brocrew/ReportEntry";
import { ReportList, Reports, type Report } from "./Reports";

vi.mock("../core", () => ({ call: vi.fn() }));

function deepFreeze<T>(v: T): T {
  if (v && typeof v === "object") {
    Object.values(v as object).forEach(deepFreeze);
    Object.freeze(v);
  }
  return v;
}

/** Unix seconds for a LOCAL wall-clock time, so the test reads the same in every time zone. */
const local = (y: number, mo: number, d: number, h: number, mi: number) => new Date(y, mo - 1, d, h, mi).getTime() / 1000;

const REPORTS: Report[] = deepFreeze([
  {
    id: "job-a1",
    title: "Add the settings screen",
    project: "owner/app-a",
    at: local(2026, 11, 6, 16, 45),
    outcome: "failed",
    failure_mode: "tests",
    changed: { pr: "https://github.com/owner/app-a/pull/12", files: 3, lines: 64 },
    checked: ["build: passed", "test: failed (2 of 9 failed)", "lint: warn"],
    not_checked: ["e2e: skipped (no display)"],
    cost: { pool: "qwen", tokens: 48211, usd: 0, seconds: 931.4 },
    moves: [],
  },
  {
    id: "thread-7",
    title: null,
    project: null,
    at: null,
    outcome: "passed",
    failure_mode: null,
    changed: { pr: null, files: null, lines: null },
    checked: [],
    not_checked: [],
    cost: { pool: null, tokens: null, usd: null, seconds: null },
    moves: [],
  },
]);

function part(root: HTMLElement, name: string): HTMLElement {
  const el = root.querySelector<HTMLElement>(`[data-part="${name}"]`);
  if (!el) throw new Error(`no ${name} in ${root.getAttribute("aria-label")}`);
  return el;
}

const items = (list: HTMLElement) => Array.from(list.querySelectorAll("li")).map((li) => li.textContent);

beforeEach(() => {
  vi.mocked(call).mockReset();
});

test("no reports shows the empty state", () => {
  render(<ReportList reports={[]} />);
  expect(screen.getByText("no data yet")).toBeTruthy();
});

test("one report entry per report, in the order given", () => {
  render(<ReportList reports={REPORTS} />);
  const section = screen.getByRole("region", { name: "Reports" });
  const entries = within(section).getAllByRole("group");
  expect(entries.map((e) => e.getAttribute("aria-label"))).toEqual(["Add the settings screen", "thread-7"]);
  expect(entries.every((e) => e.tagName === "DETAILS" && e.classList.contains("report"))).toBe(true);
  expect(entries.map((e) => e.getAttribute("data-outcome"))).toEqual(["failed", "passed"]);
});

test("an entry is open unless its outcome is passed", () => {
  render(<ReportList reports={REPORTS} />);
  expect(screen.getByRole("group", { name: "Add the settings screen" }).hasAttribute("open")).toBe(true);
  expect(screen.getByRole("group", { name: "thread-7" }).hasAttribute("open")).toBe(false);
});

test("the head puts the outcome first, then heading, project, mark and time", () => {
  render(<ReportList reports={REPORTS} />);
  const a = screen.getByRole("group", { name: "Add the settings screen" });
  const summary = a.querySelector("summary")!;
  const parts = Array.from(summary.querySelectorAll("[data-part]")).map((el) => el.getAttribute("data-part"));
  expect(parts).toEqual(["outcome", "heading", "project", "mark", "time"]);
  expect(part(a, "outcome").textContent).toBe("failed (tests)");
  expect(part(a, "heading").textContent).toBe("Add the settings screen");
  expect(part(a, "project").textContent).toBe("owner/app-a");
  expect(part(a, "mark").textContent).toBe("Switchboard · work you gave out");
  expect(REPORT_MARK).toBe("Switchboard · work you gave out");
  expect(part(a, "time").textContent).toBe("Nov 6, 4:45pm");
});

test("a bare report heads with its id, its outcome alone, and no project or time", () => {
  render(<ReportList reports={REPORTS} />);
  const t = screen.getByRole("group", { name: "thread-7" });
  expect(part(t, "outcome").textContent).toBe("passed");
  expect(part(t, "heading").textContent).toBe("thread-7");
  expect(t.querySelector('[data-part="project"]')).toBeNull();
  expect(t.querySelector('[data-part="time"]')).toBeNull();
});

test("the time is local, with minutes left out on the hour", () => {
  expect(reportTime(local(2026, 11, 6, 16, 45))).toBe("Nov 6, 4:45pm");
  expect(reportTime(local(2026, 11, 7, 7, 0))).toBe("Nov 7, 7am");
  expect(reportTime(local(2026, 1, 1, 0, 5))).toBe("Jan 1, 12:05am");
  expect(reportTime(local(2026, 12, 31, 12, 0))).toBe("Dec 31, 12pm");
});

test("what changed: the PR as a link, files and lines", () => {
  render(<ReportList reports={REPORTS} />);
  const a = screen.getByRole("group", { name: "Add the settings screen" });
  const pr = part(a, "pr");
  expect(pr.tagName).toBe("A");
  expect(pr.getAttribute("href")).toBe("https://github.com/owner/app-a/pull/12");
  expect(pr.textContent).toBe("https://github.com/owner/app-a/pull/12");
  expect(part(a, "files").textContent).toBe("files 3");
  expect(part(a, "lines").textContent).toBe("lines 64");
  const t = screen.getByRole("group", { name: "thread-7" });
  expect(part(t, "pr").tagName).not.toBe("A");
  expect(part(t, "pr").textContent).toBe("no PR");
  expect(part(t, "files").textContent).toBe("files not recorded");
  expect(part(t, "lines").textContent).toBe("lines not recorded");
});

test("what was checked and what wasn't, one line each, nothing when empty", () => {
  render(<ReportList reports={REPORTS} />);
  const a = screen.getByRole("group", { name: "Add the settings screen" });
  expect(items(part(a, "checked"))).toEqual(["build: passed", "test: failed (2 of 9 failed)", "lint: warn"]);
  expect(items(part(a, "not-checked"))).toEqual(["e2e: skipped (no display)"]);
  const t = screen.getByRole("group", { name: "thread-7" });
  expect(items(part(t, "checked"))).toEqual(["nothing"]);
  expect(items(part(t, "not-checked"))).toEqual(["nothing"]);
});

test("cost: pool, tokens, dollars and time", () => {
  render(<ReportList reports={REPORTS} />);
  const a = screen.getByRole("group", { name: "Add the settings screen" });
  expect(part(a, "pool").textContent).toBe("qwen");
  expect(part(a, "tokens").textContent).toBe("48211 tokens");
  expect(part(a, "usd").textContent).toBe("$0.00");
  expect(part(a, "seconds").textContent).toBe("15m 31s");
  const t = screen.getByRole("group", { name: "thread-7" });
  expect(part(t, "pool").textContent).toBe("pool not recorded");
  expect(part(t, "tokens").textContent).toBe("tokens not recorded");
  expect(part(t, "usd").textContent).toBe("cost not recorded");
  expect(part(t, "seconds").textContent).toBe("time not recorded");
});

test("the five parts come in REP-1's order", () => {
  render(<ReportList reports={REPORTS} />);
  const a = screen.getByRole("group", { name: "Add the settings screen" });
  const top = Array.from(a.children).map((el) => el.tagName === "SUMMARY" ? "summary" : el.getAttribute("data-part"));
  // Later issues may add children (the work tree, REP-2) after the cost; the five parts lead.
  expect(top.slice(0, 5)).toEqual(["summary", "changed", "checked", "not-checked", "cost"]);
});

test("duration rounds to the second", () => {
  expect(duration(44.6)).toBe("45s");
  expect(duration(931.4)).toBe("15m 31s");
  expect(duration(3725)).toBe("1h 2m");
  expect(duration(0)).toBe("0s");
});

test("children go last inside the entry", () => {
  render(
    <ReportEntry report={REPORTS[1]}>
      <p>after the cost</p>
    </ReportEntry>,
  );
  const t = screen.getByRole("group", { name: "thread-7" });
  expect(t.lastElementChild?.textContent).toBe("after the cost");
});

test("Reports loads the reports command once and lists what it gives", async () => {
  let resolve!: (v: unknown) => void;
  vi.mocked(call).mockReturnValue(new Promise((r) => (resolve = r)) as never);
  render(<Reports />);
  expect(screen.getByText("no data yet")).toBeTruthy();
  await act(async () => resolve({ ok: REPORTS }));
  expect(call).toHaveBeenCalledTimes(1);
  expect(call).toHaveBeenCalledWith("reports");
  expect(screen.getAllByRole("group").map((e) => e.getAttribute("aria-label"))).toEqual(["Add the settings screen", "thread-7"]);
});

test("a core error shows in place", async () => {
  vi.mocked(call).mockResolvedValue({ error: { command: "reports", lines: ["no done cards", "exit 2"] } } as never);
  render(<Reports />);
  const alert = await screen.findByRole("alert");
  expect(within(alert).getByText("reports failed")).toBeTruthy();
  expect(within(alert).getByText("no done cards")).toBeTruthy();
  expect(within(alert).getByText("exit 2")).toBeTruthy();
});

test("the ported entry carries its notice and is listed in NOTICE", () => {
  const head = entrySource.split("\n").slice(0, 15).join("\n");
  expect(head).toContain("SPDX-License-Identifier: Apache-2.0");
  expect(head).toContain("https://github.com/brocrew/brocrew");
  expect(head).toContain("Lifted from BroCrew apps/desktop/src/renderer/src/components/WorkReportEntry.vue");
  expect(head).toContain("Changed: ");
  expect(notice.split("\n")).toContain("- app/src/brocrew/ReportEntry.tsx");
});

test("the code map marks the view present", () => {
  expect(codeMap).toContain("- REP-1 view (`ReportEntry`, `ReportList`, `Reports`): present.");
});
