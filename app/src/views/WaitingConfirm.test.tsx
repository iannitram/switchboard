// CORE-3, BAK-3 (#347): the confirm step in the Waiting on you view. A guarded request (a repo
// delete or a restore over a live repo asked for from the CLI or MCP) runs only after its
// Confirm button; nothing else sends `confirmed: true`.
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { vi } from "vitest";
import { call } from "../core";
import { guardedText, Waiting, type WaitingItemData } from "./Waiting";

vi.mock("../core", () => ({ call: vi.fn() }));

const RESTORE = "I was asked from the CLI to restore over owner/alpha";
const GUARDED: WaitingItemData = {
  what: RESTORE,
  why: "Asked for from the CLI or MCP; only the app can do it, after I confirm (CORE-3).",
  source: { GuardedAction: { text: RESTORE } },
  link: "https://github.com/owner/alpha",
  action: "Confirm in the app",
  since: 1762447500,
};
const TASK: WaitingItemData = {
  what: "Book the dentist",
  why: "A task of mine in CalDAV (U7).",
  source: { CalDavTask: { uid: "3f1c" } },
  link: "caldav:3f1c",
  action: "Mark done",
  since: 1762360000,
};

type Answer = { ok: unknown } | { error: { command: string; lines: string[] } };

// The core as the view sees it: `waiting` answers `lists` in turn (the last one again after
// that) and `confirm_guarded` answers `confirm`.
function core(lists: WaitingItemData[][], confirm: Answer) {
  let reads = 0;
  vi.mocked(call).mockImplementation((async (command: string) => {
    if (command === "waiting") return { ok: lists[Math.min(reads++, lists.length - 1)] };
    if (command === "confirm_guarded") return confirm;
    throw new Error(`unexpected command ${command}`);
  }) as never);
}

const confirmCalls = () => vi.mocked(call).mock.calls.filter(([command]) => command === "confirm_guarded");

beforeEach(() => {
  vi.mocked(call).mockReset();
});

test("only a guarded request has a text to confirm", () => {
  expect(guardedText(GUARDED)).toBe(RESTORE);
  expect(guardedText(TASK)).toBeNull();
  expect(guardedText({ ...GUARDED, source: "pr" })).toBeNull();
  expect(guardedText({ ...GUARDED, source: null })).toBeNull();
  expect(guardedText({ ...GUARDED, source: { GuardedAction: { text: 7 } } })).toBeNull();
});

test("listing the request confirms nothing, and its action only opens the confirm step", async () => {
  core([[GUARDED, TASK]], { ok: { what: "restore over owner/alpha", lines: [], cleared: true } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  expect(screen.queryByRole("group", { name: "Confirm" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Confirm in the app" }));
  const step = screen.getByRole("group", { name: "Confirm" });
  expect(within(step).getByText(RESTORE)).toBeTruthy();
  expect(within(step).getAllByRole("button").map((b) => b.textContent)).toEqual(["Cancel", "Confirm"]);
  expect(confirmCalls()).toEqual([]);
});

test("Cancel closes the step and asks the core for nothing", async () => {
  core([[GUARDED]], { ok: { what: "restore over owner/alpha", lines: [], cleared: true } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  fireEvent.click(screen.getByRole("button", { name: "Confirm in the app" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("group", { name: "Confirm" })).toBeNull();
  expect(confirmCalls()).toEqual([]);
  expect(screen.getAllByRole("article")).toHaveLength(1);
});

test("the action of an item that is not a guarded request opens no confirm step", async () => {
  core([[TASK]], { ok: { what: "", lines: [], cleared: true } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  fireEvent.click(screen.getByRole("button", { name: "Mark done" }));
  expect(screen.queryByRole("group", { name: "Confirm" })).toBeNull();
  expect(confirmCalls()).toEqual([]);
});

test("Confirm sends the request's text with confirmed true, shows what was done and reads the list again", async () => {
  core([[GUARDED, TASK], [TASK]], { ok: { what: "restore over owner/alpha", lines: ["3 refs pushed"], cleared: true } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  fireEvent.click(screen.getByRole("button", { name: "Confirm in the app" }));
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  const status = await screen.findByRole("status");
  expect(within(status).getByText("restore over owner/alpha: done")).toBeTruthy();
  expect(within(status).getByText("3 refs pushed")).toBeTruthy();
  expect(confirmCalls()).toEqual([["confirm_guarded", { text: RESTORE, confirmed: true }]]);
  await waitFor(() => expect(screen.getAllByRole("article").map((a) => a.getAttribute("aria-label"))).toEqual(["Book the dentist"]));
  expect(screen.queryByRole("group", { name: "Confirm" })).toBeNull();
});

test("a refusal shows the core's text and the request stays listed", async () => {
  core([[GUARDED]], { error: { command: "confirm_guarded", lines: ["refused: restore over owner/alpha: refused inside a job worktree"] } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  fireEvent.click(screen.getByRole("button", { name: "Confirm in the app" }));
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  const alert = await screen.findByRole("alert");
  expect(within(alert).getByText("confirm_guarded failed")).toBeTruthy();
  expect(within(alert).getByText("refused: restore over owner/alpha: refused inside a job worktree")).toBeTruthy();
  expect(screen.queryByRole("status")).toBeNull();
  expect(screen.getAllByRole("article").map((a) => a.getAttribute("aria-label"))).toEqual([RESTORE]);
});

test("a restore that holds comments back says the request stays", async () => {
  core([[GUARDED]], { ok: { what: "restore over owner/alpha", lines: ["4 comments wait for their issues"], cleared: false } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  fireEvent.click(screen.getByRole("button", { name: "Confirm in the app" }));
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  const status = await screen.findByRole("status");
  expect(within(status).getByText("The request stays in Waiting on you.")).toBeTruthy();
});
