/// <reference types="vite/client" />
// #331: a chat thread the core could not read is named in the Reports view's error banner.
import { act, render, screen, within } from "@testing-library/react";
import { vi } from "vitest";
import { call } from "../core";
import { Reports, answerOf, type Report } from "./Reports";

vi.mock("../core", () => ({ call: vi.fn() }));

const REPORT: Report = {
  id: "job-a1",
  title: "Add the settings screen",
  project: "owner/app-a",
  at: null,
  outcome: "passed",
  failure_mode: null,
  changed: { pr: null, files: null, lines: null },
  checked: [],
  not_checked: [],
  cost: { pool: null, tokens: null, usd: null, seconds: null },
  moves: [],
};

beforeEach(() => {
  vi.mocked(call).mockReset();
});

test("an unreadable thread is named in the error banner and the other report still shows", async () => {
  vi.mocked(call).mockResolvedValue({
    ok: { reports: [REPORT], warnings: [{ thread_id: "chat-bad", reason: "line 1: not a job row" }] },
  } as never);
  render(<Reports />);
  await act(async () => {});
  const banner = screen.getByRole("alert");
  expect(banner.textContent).toContain("chat-bad");
  expect(banner.textContent).toContain("line 1: not a job row");
  expect(within(screen.getByRole("region", { name: "Reports" })).getByRole("group", { name: "Add the settings screen" })).toBeTruthy();
});

test("no warnings means no banner", async () => {
  vi.mocked(call).mockResolvedValue({ ok: { reports: [REPORT], warnings: [] } } as never);
  render(<Reports />);
  await act(async () => {});
  expect(screen.queryByRole("alert")).toBeNull();
});

test("a bare list from before the answer had warnings still shows its reports", async () => {
  vi.mocked(call).mockResolvedValue({ ok: [REPORT] } as never);
  render(<Reports />);
  await act(async () => {});
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByRole("group", { name: "Add the settings screen" })).toBeTruthy();
});

test("answerOf reads both the object and the bare list", () => {
  expect(answerOf([REPORT])).toEqual({ reports: [REPORT], warnings: [] });
  expect(answerOf({ reports: [REPORT], warnings: [] })).toEqual({ reports: [REPORT], warnings: [] });
  expect(answerOf({})).toEqual({ reports: [], warnings: [] });
});
