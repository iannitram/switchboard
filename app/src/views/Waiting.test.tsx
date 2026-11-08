import { render, screen, fireEvent, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { call } from "../core";
import { Waiting, WaitingList } from "./Waiting";
import type { WaitingItemData } from "../brocrew/WaitingItem";

vi.mock("../core", () => ({ call: vi.fn() }));

beforeEach(() => {
  vi.mocked(call).mockClear();
});

const SINCE = 1_700_000_000;
const newer: WaitingItemData = {
  what: "PR #7 needs your review",
  why: "labelled needs-me",
  source: { kind: "github", repo: "a/b", number: 7 },
  link: "https://github.com/a/b/pull/7",
  action: "Open PR",
  since: SINCE + 60,
};
const older: WaitingItemData = {
  what: "Ticket IT-3 waits for approval",
  why: "approval asked",
  source: { kind: "it" },
  link: "caldav:uid-3",
  action: "Mark done",
  since: SINCE,
};

test("the list renders the items in the order given", () => {
  render(<WaitingList items={[newer, older]} />);
  const rows = screen.getAllByRole("listitem");
  expect(rows).toHaveLength(2);
  expect(within(rows[0]).getByText("PR #7 needs your review")).toBeTruthy();
  expect(within(rows[1]).getByText("Ticket IT-3 waits for approval")).toBeTruthy();
});

test("an empty list says nothing is waiting on you", () => {
  render(<WaitingList items={[]} />);
  expect(screen.getByText("Nothing is waiting on you.")).toBeTruthy();
});

test("a rejected call shows its error and no list", async () => {
  vi.mocked(call).mockResolvedValueOnce({ error: { command: "waiting", lines: ["core is down"] } });
  render(<Waiting />);
  expect(await screen.findByText("core is down")).toBeTruthy();
  expect(screen.queryByRole("list")).toBeNull();
});

test("a call that throws shows its message and no list", async () => {
  vi.mocked(call).mockRejectedValueOnce(new Error("boom"));
  render(<Waiting />);
  expect(await screen.findByText("boom")).toBeTruthy();
  expect(screen.queryByRole("list")).toBeNull();
});

test("the action button calls onAction with its item", () => {
  const onAction = vi.fn();
  render(<WaitingList items={[newer]} onAction={onAction} />);
  fireEvent.click(screen.getByRole("button", { name: "Open PR" }));
  expect(onAction).toHaveBeenCalledTimes(1);
  expect(onAction).toHaveBeenCalledWith(newer);
});

test("the source link points at link", () => {
  render(<WaitingList items={[older]} />);
  const link = screen.getByRole("link", { name: "source" });
  expect(link.getAttribute("href")).toBe("caldav:uid-3");
});

test("the view asks the core once for waiting and lists its items", async () => {
  vi.mocked(call).mockResolvedValueOnce({ ok: [newer, older] });
  render(<Waiting />);
  expect(await screen.findByText("PR #7 needs your review")).toBeTruthy();
  expect(call).toHaveBeenCalledWith("waiting");
  expect(vi.mocked(call).mock.calls.filter(([cmd]) => cmd === "waiting")).toHaveLength(1);
});
