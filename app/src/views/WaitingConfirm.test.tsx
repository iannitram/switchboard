// CORE-3, BAK-3 (#347): the confirm step in the Waiting on you view. A guarded request (a repo
// delete, which archives, or a restore over a live repo, asked for from the CLI or MCP) runs
// only after its Confirm button, which is enabled only while the repo's `owner/name` is typed
// letter for letter; nothing else sends `confirmed: true`.
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { vi } from "vitest";
import { call } from "../core";
import { confirmLine, guardedRequest, guardedText, HELD_ACTION, Waiting, type WaitingItemData } from "./Waiting";

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
const DELETE = "I was asked from the CLI to archive owner/alpha";
const GUARDED_DELETE: WaitingItemData = { ...GUARDED, what: DELETE, source: { GuardedAction: { text: DELETE } } };
const HELD: WaitingItemData = { ...GUARDED, action: HELD_ACTION, why: "The restore over owner/alpha pushed the backup's refs; 4 comments and 0 closes or body changes wait." };
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

// Type `name` in the confirm step's box.
const type = (name: string) => fireEvent.change(screen.getByRole("textbox", { name: "Type owner/alpha to confirm" }), { target: { value: name } });
const confirmButton = () => screen.getByRole("button", { name: "Confirm" }) as HTMLButtonElement;

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

test("Confirm sends the request's text with confirmed true and the name typed, shows what was done and reads the list again", async () => {
  core([[GUARDED, TASK], [TASK]], { ok: { what: "restore over owner/alpha", lines: ["3 refs pushed"], cleared: true } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  fireEvent.click(screen.getByRole("button", { name: "Confirm in the app" }));
  type("owner/alpha");
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  const status = await screen.findByRole("status");
  expect(within(status).getByText("restore over owner/alpha: done")).toBeTruthy();
  expect(within(status).getByText("3 refs pushed")).toBeTruthy();
  expect(confirmCalls()).toEqual([["confirm_guarded", { text: RESTORE, confirmed: true, typed: "owner/alpha" }]]);
  await waitFor(() => expect(screen.getAllByRole("article").map((a) => a.getAttribute("aria-label"))).toEqual(["Book the dentist"]));
  expect(screen.queryByRole("group", { name: "Confirm" })).toBeNull();
});

test("a refusal shows the core's text and the request stays listed", async () => {
  core([[GUARDED]], { error: { command: "confirm_guarded", lines: ["refused: restore over owner/alpha: refused inside a job worktree"] } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  fireEvent.click(screen.getByRole("button", { name: "Confirm in the app" }));
  type("owner/alpha");
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
  type("owner/alpha");
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  const status = await screen.findByRole("status");
  expect(within(status).getByText("The request stays in Waiting on you.")).toBeTruthy();
});

test("a guarded request is read back as the core reads it", () => {
  expect(guardedRequest(GUARDED)).toEqual({ kind: "restore", repo: "owner/alpha" });
  expect(guardedRequest(GUARDED_DELETE)).toEqual({ kind: "archive", repo: "owner/alpha" });
  expect(guardedRequest(HELD)).toEqual({ kind: "held", repo: "owner/alpha" });
  // A request filed with the old wording is the same archive request.
  const old = "I was asked from the CLI to delete owner/alpha";
  expect(guardedRequest({ ...GUARDED, what: old, source: { GuardedAction: { text: old } } })).toEqual({ kind: "archive", repo: "owner/alpha" });
  // A held delete is still an archive: only a restore holds anything back.
  expect(guardedRequest({ ...GUARDED_DELETE, action: HELD_ACTION })).toEqual({ kind: "archive", repo: "owner/alpha" });
  expect(guardedRequest(TASK)).toBeNull();
  expect(guardedRequest({ ...GUARDED, link: "https://github.com/owner/beta" })).toBeNull();
  expect(guardedRequest({ ...GUARDED, link: "" })).toBeNull();
  for (const text of ["delete owner/alpha", "archive owner/alpha", "x to archive alpha", "I was asked to merge owner/alpha#4", "x to delete alpha", "x to delete a/b/c", "x to delete owner/al pha"]) {
    expect(guardedRequest({ ...GUARDED, source: { GuardedAction: { text } } })).toBeNull();
  }
});

test("Confirm is enabled only while the repo's name is typed letter for letter", async () => {
  core([[GUARDED]], { ok: { what: "restore over owner/alpha", lines: [], cleared: true } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  fireEvent.click(screen.getByRole("button", { name: "Confirm in the app" }));
  expect(confirmButton().disabled).toBe(true);
  for (const wrong of ["owner/alph", "Owner/Alpha", "owner/alpha ", " owner/alpha", "alpha", "owner/alpha/x"]) {
    type(wrong);
    expect(confirmButton().disabled).toBe(true);
    fireEvent.click(confirmButton());
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter", code: "Enter" });
  }
  expect(confirmCalls()).toEqual([]);
  type("owner/alpha");
  expect(confirmButton().disabled).toBe(false);
  // Enter in the box confirms nothing: only the button does.
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter", code: "Enter" });
  expect(confirmCalls()).toEqual([]);
});

test("the name typed for one request does not carry over to the next", async () => {
  core([[GUARDED, GUARDED_DELETE]], { ok: { what: "", lines: [], cleared: true } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  fireEvent.click(screen.getAllByRole("button", { name: "Confirm in the app" })[0]);
  type("owner/alpha");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  fireEvent.click(screen.getAllByRole("button", { name: "Confirm in the app" })[1]);
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("");
  expect(confirmButton().disabled).toBe(true);
  expect(confirmCalls()).toEqual([]);
});

test("a delete request says it archives, and Confirm sends it with the name typed", async () => {
  core([[GUARDED_DELETE], []], { ok: { what: "archive owner/alpha", lines: ["owner/alpha is archived on GitHub: read-only, nothing was deleted"], cleared: true } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  fireEvent.click(screen.getByRole("button", { name: "Confirm in the app" }));
  const step = screen.getByRole("group", { name: "Confirm" });
  expect(within(step).getByText(confirmLine({ kind: "archive", repo: "owner/alpha" }))).toBeTruthy();
  expect(confirmLine({ kind: "archive", repo: "owner/alpha" })).toBe("Confirm archives owner/alpha on GitHub: it becomes read-only. Nothing is deleted.");
  type("owner/alpha");
  fireEvent.click(confirmButton());
  const status = await screen.findByRole("status");
  expect(within(status).getByText("archive owner/alpha: done")).toBeTruthy();
  expect(within(status).getByText("owner/alpha is archived on GitHub: read-only, nothing was deleted")).toBeTruthy();
  expect(confirmCalls()).toEqual([["confirm_guarded", { text: DELETE, confirmed: true, typed: "owner/alpha" }]]);
});

test("a restore that held comments back says its confirm pushes nothing again", async () => {
  core([[HELD]], { ok: { what: "restore over owner/alpha (what was held back)", lines: ["no ref was pushed again"], cleared: true } });
  render(<Waiting />);
  await screen.findAllByRole("article");
  fireEvent.click(screen.getByRole("button", { name: HELD_ACTION }));
  const step = screen.getByRole("group", { name: "Confirm" });
  expect(within(step).getByText("Confirm only queues what the restore over owner/alpha held back (comments, closes). Nothing is pushed again.")).toBeTruthy();
  expect(within(step).queryByText(/pushed over the live ones/)).toBeNull();
  expect(confirmButton().disabled).toBe(true);
});
