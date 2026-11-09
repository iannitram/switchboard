/// <reference types="vite/client" />
// WAIT-1 (#60): the Waiting on you view and the item ported from BroCrew. Locked tests (R28):
// the builder never edits this file.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { vi } from "vitest";
import codeMap from "../../../docs/code-map.md?raw";
import notice from "../../../NOTICE?raw";
import itemSource from "../brocrew/WaitingItem.tsx?raw";
import { call } from "../core";
import { WaitingItem } from "../brocrew/WaitingItem";
import { NOTHING_WAITING, Waiting, WaitingList, type WaitingItemData } from "./Waiting";

vi.mock("../core", () => ({ call: vi.fn() }));

function deepFreeze<T>(v: T): T {
  if (v && typeof v === "object") {
    Object.values(v as object).forEach(deepFreeze);
    Object.freeze(v);
  }
  return v;
}

const ITEMS: WaitingItemData[] = deepFreeze([
  {
    what: "PR #12: Add the settings screen",
    why: "the review ladder reached Fable",
    source: "pr",
    link: "https://github.com/owner/app-a/pull/12",
    action: "Open PR",
    since: 1762447500,
  },
  {
    what: "Book the dentist",
    why: "CalDAV task due today",
    source: "caldav",
    link: "caldav:3f1c-dentist",
    action: "Mark done",
    since: 1762360000,
  },
]);

function part(root: HTMLElement, name: string): HTMLElement {
  const el = root.querySelector<HTMLElement>(`[data-part="${name}"]`);
  if (!el) throw new Error(`no ${name} in ${root.getAttribute("aria-label")}`);
  return el;
}

beforeEach(() => {
  vi.mocked(call).mockReset();
});

test("an empty list says nothing is waiting", () => {
  render(<WaitingList items={[]} />);
  const section = screen.getByRole("region", { name: "Waiting on you" });
  expect(within(section).getByText("Nothing is waiting on you.")).toBeTruthy();
  expect(NOTHING_WAITING).toBe("Nothing is waiting on you.");
  expect(section.querySelector("ol, ul")).toBeNull();
});

test("one item per entry, in the order given", () => {
  render(<WaitingList items={ITEMS} />);
  const section = screen.getByRole("region", { name: "Waiting on you" });
  const entries = within(section).getAllByRole("article");
  expect(entries.map((e) => e.getAttribute("aria-label"))).toEqual(["PR #12: Add the settings screen", "Book the dentist"]);
  expect(entries.map((e) => part(e, "why").textContent)).toEqual(["the review ladder reached Fable", "CalDAV task due today"]);
  expect(screen.queryByText("Nothing is waiting on you.")).toBeNull();
});

test("an item shows its what, why, a link to its source and its one action", () => {
  const { container } = render(<WaitingItem item={ITEMS[0]} />);
  const item = container.querySelector<HTMLElement>("article.waiting-item")!;
  expect(part(item, "what").textContent).toBe("PR #12: Add the settings screen");
  expect(part(item, "why").textContent).toBe("the review ladder reached Fable");
  expect(part(item, "since").textContent).not.toBe("");
  expect(within(item).getByRole("link").getAttribute("href")).toBe("https://github.com/owner/app-a/pull/12");
  expect(within(item).getAllByRole("button").map((b) => b.textContent)).toEqual(["Open PR"]);
});

test("the link points at the item's link, whatever its scheme", () => {
  render(<WaitingList items={ITEMS} />);
  const hrefs = screen.getAllByRole("link").map((a) => a.getAttribute("href"));
  expect(hrefs).toEqual(["https://github.com/owner/app-a/pull/12", "caldav:3f1c-dentist"]);
});

test("the action button calls onAction with its item", () => {
  const onAction = vi.fn();
  render(<WaitingList items={ITEMS} onAction={onAction} />);
  fireEvent.click(screen.getByRole("button", { name: "Mark done" }));
  expect(onAction).toHaveBeenCalledTimes(1);
  expect(onAction).toHaveBeenCalledWith(ITEMS[1]);
});

test("the action button without onAction is harmless", () => {
  render(<WaitingItem item={ITEMS[1]} />);
  fireEvent.click(screen.getByRole("button", { name: "Mark done" }));
});

test("the view asks the core once and lists what it sends, in order", async () => {
  vi.mocked(call).mockResolvedValue({ ok: ITEMS } as never);
  render(<Waiting />);
  const entries = await screen.findAllByRole("article");
  expect(entries.map((e) => e.getAttribute("aria-label"))).toEqual(["PR #12: Add the settings screen", "Book the dentist"]);
  expect(vi.mocked(call)).toHaveBeenCalledTimes(1);
  expect(vi.mocked(call).mock.calls[0][0]).toBe("waiting");
});

test("the view with nothing from the core says nothing is waiting", async () => {
  vi.mocked(call).mockResolvedValue({ ok: [] } as never);
  render(<Waiting />);
  expect(await screen.findByText("Nothing is waiting on you.")).toBeTruthy();
});

test("a rejected call shows its error text and no list", async () => {
  vi.mocked(call).mockResolvedValue({ error: { command: "waiting", lines: ["no snapshot", "exit 2"] } } as never);
  render(<Waiting />);
  const alert = await screen.findByRole("alert");
  expect(within(alert).getByText("waiting failed")).toBeTruthy();
  expect(within(alert).getByText("no snapshot")).toBeTruthy();
  expect(within(alert).getByText("exit 2")).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Waiting on you" })).toBeNull();
  expect(screen.queryByRole("article")).toBeNull();
  expect(screen.queryByText("Nothing is waiting on you.")).toBeNull();
});

test("the ported item carries its notice and is listed in NOTICE", () => {
  const head = itemSource.split("\n").slice(0, 15).join("\n");
  expect(head).toContain("SPDX-License-Identifier: Apache-2.0");
  expect(head).toContain("https://github.com/brocrew/brocrew");
  expect(head).toContain("Lifted from BroCrew apps/desktop/src/renderer/src/components/");
  expect(head).toContain("at ebde9907");
  expect(head).toContain("Changed: ");
  expect(notice.split("\n")).toContain("- app/src/brocrew/WaitingItem.tsx");
});

test("the code map marks the view present", () => {
  expect(codeMap).toContain("- WAIT-1 view (`WaitingItem`, `WaitingList`, `Waiting`): present.");
});
