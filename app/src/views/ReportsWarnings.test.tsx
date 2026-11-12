// #331: partial report answers use the existing error banner.
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { vi } from "vitest";
import { call } from "../core";
import { Reports, type Report } from "./Reports";

vi.mock("../core", () => ({ call: vi.fn() }));

const REPORT: Report = {
  id: "chat-good", title: null, project: null, at: null, outcome: "passed", failure_mode: null,
  changed: { pr: null, files: null, lines: null }, checked: [], not_checked: [],
  cost: { pool: null, tokens: null, usd: null, seconds: null }, moves: [],
};
const WARNING = "Skipped unreadable thread chat-bad: parse: chat-bad.jsonl: line 1: expected value";

beforeEach(() => {
  vi.mocked(call).mockReset();
});

test("an unreadable thread's id and reason appear in the banner beside readable reports", async () => {
  vi.mocked(call).mockResolvedValue({ ok: { reports: [REPORT], warnings: [WARNING] } });
  render(<Reports />);
  const banner = await screen.findByRole("alert");
  expect(within(banner).getByText(WARNING)).toBeTruthy();
  expect(screen.getAllByRole("alert")).toHaveLength(1);
  expect(screen.getByRole("group", { name: "chat-good" })).toBeTruthy();
});

test("readable threads have no warning banner", async () => {
  vi.mocked(call).mockResolvedValue({ ok: { reports: [REPORT], warnings: [] } });
  render(<Reports />);
  await screen.findByRole("group", { name: "chat-good" });
  expect(screen.queryByRole("alert")).toBeNull();
});

test("warnings show even when every thread is unreadable", async () => {
  vi.mocked(call).mockResolvedValue({ ok: { reports: [], warnings: [WARNING] } });
  render(<Reports />);
  expect(within(await screen.findByRole("alert")).getByText(WARNING)).toBeTruthy();
  expect(screen.getByText("no data yet")).toBeTruthy();
});

test("filter answers update warnings and All restores the original warnings", async () => {
  vi.mocked(call)
    .mockResolvedValueOnce({ ok: { reports: [REPORT], warnings: [WARNING] } })
    .mockResolvedValueOnce({ ok: { reports: [REPORT], warnings: [] } });
  render(<Reports />);
  await screen.findByRole("alert");
  await act(async () => fireEvent.change(screen.getByRole("combobox", { name: "Outcome" }), { target: { value: "passed" } }));
  expect(screen.queryByRole("alert")).toBeNull();
  fireEvent.change(screen.getByRole("combobox", { name: "Outcome" }), { target: { value: "" } });
  expect(within(screen.getByRole("alert")).getByText(WARNING)).toBeTruthy();
  expect(call).toHaveBeenCalledTimes(2);
});

test("an older filter answer cannot replace the current warning", async () => {
  const replies: ((value: unknown) => void)[] = [];
  vi.mocked(call).mockImplementation(() => new Promise((resolve) => replies.push(resolve)) as never);
  render(<Reports />);
  await act(async () => replies[0]({ ok: { reports: [REPORT], warnings: [] } }));
  const outcome = screen.getByRole("combobox", { name: "Outcome" });
  fireEvent.change(outcome, { target: { value: "passed" } });
  fireEvent.change(outcome, { target: { value: "" } });
  await act(async () => replies[1]({ ok: { reports: [], warnings: [WARNING] } }));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByRole("group", { name: "chat-good" })).toBeTruthy();
});
